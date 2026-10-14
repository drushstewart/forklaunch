import assert from 'node:assert/strict';
import {createHmac,randomUUID} from 'node:crypto';
import http from 'node:http';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
import {compareCheckout} from './fixtures/rust_ecommerce_checkout_reference.mjs';
import {compareWebhooks} from './fixtures/rust_ecommerce_webhook_reference.mjs';
const [binary,sdkPath,bridgeBinary]=process.argv.slice(2);
const role=process.env.FORKLAUNCH_TEST_ROLE,secret='ecommerce-generated-hmac-secret';
const children=[];let stripeCalls=0,paypalCalls=0,providerFailure,checkoutTaxDelay=0,captures=0,captureFailure=false,beforeIntentRead;
const provider=http.createServer(async(req,res)=>{
  let raw='';for await(const chunk of req)raw+=chunk;
  res.setHeader('content-type','application/json');
  if(req.url==='/v1/payment_intents'){
    stripeCalls++;assert.equal(req.headers.authorization,'Bearer sk_fixture');
    const body=new URLSearchParams(raw);assert.ok(Number(body.get('amount'))>0);assert.equal(body.get('currency'),'usd');
    if(providerFailure==='payment'){providerFailure=undefined;res.writeHead(503);res.end('{}');return;}
    res.end(JSON.stringify({id:`pi_fixture_${stripeCalls}`,client_secret:'fixture-client-secret'}));
  }else if(req.url.startsWith('/v1/payment_intents/')){
    if(beforeIntentRead){const callback=beforeIntentRead;beforeIntentRead=undefined;await callback();}
    res.end(JSON.stringify({id:req.url.split('/').at(-1),client_secret:'fixture-client-secret'}));
  }else if(req.url==='/v1/tax/calculations'){
    if(checkoutTaxDelay)await new Promise(resolve=>setTimeout(resolve,checkoutTaxDelay));
    assert.equal(req.headers.authorization,'Bearer sk_fixture');
    if(providerFailure==='tax'){providerFailure=undefined;res.writeHead(503);res.end('{}');return;}
    res.end(JSON.stringify({tax_amount_exclusive:14,tax_breakdown:[{amount:14,tax_rate_details:{state:'CA',country:'US'}}]}));
  }else if(req.url==='/v1/oauth2/token'){
    assert.equal(req.headers.authorization,`Basic ${Buffer.from('paypal-id:paypal-secret').toString('base64')}`);
    res.end(JSON.stringify({access_token:'fixture-access-token',token_type:'Bearer',expires_in:3600}));
  }else if(req.url==='/v2/checkout/orders'){
    paypalCalls++;assert.equal(req.headers.authorization,'Bearer fixture-access-token');
    assert.ok(Number(JSON.parse(raw).purchase_units[0].amount.value)>0);
    res.end(JSON.stringify({id:`paypal_fixture_${paypalCalls}`,status:'CREATED'}));
  }else if(/^\/v2\/checkout\/orders\/[^/]+\/capture$/.test(req.url)){
    captures++;assert.equal(req.headers.authorization,'Bearer fixture-access-token');
    if(captureFailure){res.writeHead(503);res.end('{}');return;}
    res.end(JSON.stringify({id:req.url.split('/').at(-2),status:'COMPLETED'}));
  }else if(req.url==='/v1/notifications/verify-webhook-signature'){
    assert.equal(req.headers.authorization,'Bearer fixture-access-token');const body=JSON.parse(raw);
    assert.equal(body.webhook_id,'paypal-webhook-fixture');
    for(const key of ['transmission_id','transmission_time','transmission_sig','cert_url','auth_algo'])assert.ok(body[key]);
    assert.ok(body.webhook_event.create_time);assert.ok(Array.isArray(body.webhook_event.links));
    res.end(JSON.stringify({verification_status:body.transmission_sig==='valid'?'SUCCESS':'FAILURE'}));
  }else{assert.notEqual(req.url,'/must-not-fetch-certificate');res.writeHead(404);res.end('{}');}
});
await new Promise(resolve=>provider.listen(0,'127.0.0.1',resolve));
const providerBase=`http://127.0.0.1:${provider.address().port}`;
function hmac(path,method='GET',body){
  const ts=new Date().toISOString(),nonce=randomUUID();
  const serialized=body===undefined?'undefined':`${body}\n`;
  const signature=createHmac('sha256',secret).update(`${method}\n${path}\n${serialized}${ts}\n${nonce}`).digest('base64');
  return `HMAC keyId=default ts=${ts} nonce=${nonce} signature=${signature}`;
}
async function freePort(){const server=net.createServer();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const port=server.address().port;await new Promise(resolve=>server.close(resolve));return port;}
async function start({tenant='a',worker=false,extra={}}={}){
  const db=new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);db.username=role;db.password='parity-test-only';
  const env={...Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.startsWith('OTEL_'))),DATABASE_URL:db.toString(),TENANT_ID:`${role}-${tenant}`,ENCRYPTION_KEY:'ecommerce-fixture-key',HOST:'127.0.0.1',SERVICE_PORT:'0',HMAC_SECRET_KEY:secret,JWKS_PUBLIC_KEY_URL:`${providerBase}/jwks`,STRIPE_API_KEY:'sk_fixture',STRIPE_WEBHOOK_SECRET:'whsec_fixture',STRIPE_API_BASE:providerBase,PAYPAL_CLIENT_ID:'paypal-id',PAYPAL_CLIENT_SECRET:'paypal-secret',PAYPAL_BASE_URL:providerBase,PAYPAL_WEBHOOK_ID:'paypal-webhook-fixture',WORKER_POLL_MS:'20',...extra};
  if(worker)for(const key of ['HMAC_SECRET_KEY','JWKS_PUBLIC_KEY_URL','STRIPE_API_KEY','STRIPE_WEBHOOK_SECRET','STRIPE_API_BASE','PAYPAL_CLIENT_ID','PAYPAL_CLIENT_SECRET','PAYPAL_BASE_URL','PAYPAL_WEBHOOK_ID'])delete env[key];
  const child=spawn(binary,worker?['--worker']:[],{env,stdio:['ignore','pipe','pipe']});let stdout='',stderr='';
  child.stdout.on('data',data=>stdout+=data);child.stderr.on('data',data=>stderr+=data);
  const exited=new Promise(resolve=>child.once('close',resolve));const instance={child,exited,stderr:()=>stderr,stdout:()=>stdout};children.push(instance);
  for(let i=0;i<400;i++){
    const base=stdout.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
    if(worker?stdout.includes('inventory worker ready'):base)return{...instance,base};
    assert.equal(child.exitCode,null,stderr);await new Promise(resolve=>setTimeout(resolve,25));
  }throw new Error(`Generated ecommerce startup timeout: ${stderr}`);
}
async function stop(instance){instance.child.kill('SIGTERM');const timer=setTimeout(()=>instance.child.kill('SIGKILL'),15000);try{assert.equal(await instance.exited,0,instance.stderr());}finally{clearTimeout(timer);}}
async function call(base,path,{method='GET',body,authorization}={}){
  const raw=body===undefined?undefined:JSON.stringify(body);
  return fetch(base+path,{method,body:raw,headers:{authorization:authorization??hmac(path.split('?')[0],method,raw),...(raw===undefined?{}:{'content-type':'application/json'})}});
}
async function value(response){assert.equal(response.status,200,await response.clone().text());return response.json();}
async function eventually(check){for(let i=0;i<300;i++){if(await check())return;await new Promise(resolve=>setTimeout(resolve,25));}throw new Error('Inventory worker did not apply the order event');}
try{
  let service=await start();const other=await start({tenant:'b'});
  for(const instance of [service,other])assert.equal((await probeMcp(instance.base,['createProduct','listCatalog','createCart','transitionOrder','createPayment','importCatalog','checkout','handleStripeWebhook','handlePaypalWebhook'])).length,32);
  assert.equal((await fetch(service.base+'/product')).status,401);
  const denied=await mcpRpc(service.base,'tools/call',{name:'getProduct',arguments:{params:{id:'missing'}}});assert.equal(denied.isError,true);
  const sdk=await import(pathToFileURL(sdkPath));const client=sdk.createForkLaunchSdk({serviceBaseUrls:{ecommerce:service.base}});
  const product={id:'product',externalId:'external',handle:'beans',title:'Beans'};
  const created=await client.ecommerce.product.createProduct({body:product,headers:{authorization:hmac('/product','POST',JSON.stringify(product))}});assert.equal(created.code,200);assert.equal(created.response.id,'product');
  await value(await call(service.base,'/variant',{method:'POST',body:{id:'variant',productId:'product',externalId:'external-variant',title:'Large',priceCents:100,optionValues:{Size:'Large'}}}));
  const catalog=await client.ecommerce.product.listCatalog({query:{ids:['product'],limit:1},headers:{authorization:hmac('/product/catalog')}});
  assert.equal(catalog.code,200);assert.equal(catalog.response.total,1);assert.equal(catalog.response.products[0].variants[0].stock,8);
  assert.deepEqual(await value(await call(other.base,'/product')),[]);
  assert.equal((await call(service.base,'/product?ids=null')).status,400);
  assert.equal((await call(service.base,'/product?inStock=yes')).status,400);
  assert.equal((await call(service.base,'/cart/items',{method:'POST',body:{cartId:'cart',variantId:'variant',quantity:1.5}})).status,400);
  const mcpCatalog=await mcpRpc(service.base,'tools/call',{name:'listCatalog',arguments:{query:{ids:['product'],limit:1},headers:{authorization:hmac('/product/catalog')}}});
  assert.equal(mcpCatalog.isError,false,JSON.stringify(mcpCatalog));assert.deepEqual(JSON.parse(mcpCatalog.content[0].text),catalog.response);
  const importedBody={products:[{externalId:'imported',handle:'imported',title:'Imported',variants:[{externalId:'imported-variant',title:'Box',priceCents:75,initialStock:12}]}]};
  const imported=await client.ecommerce.catalogImport.importCatalog({body:importedBody,headers:{authorization:hmac('/catalog-import','POST',JSON.stringify(importedBody))}});
  assert.equal(imported.code,200);assert.deepEqual(imported.response,{productsImported:1,variantsImported:1});
  const importedProduct=await value(await call(service.base,'/product/handle/imported'));
  const importedVariant=(await value(await call(service.base,`/variant/product/${importedProduct.id}`)))[0];
  await value(await call(service.base,'/inventory/adjust',{method:'PUT',body:{variantId:importedVariant.id,delta:-2}}));
  const reimport=await mcpRpc(service.base,'tools/call',{name:'importCatalog',arguments:{body:importedBody,headers:{authorization:hmac('/catalog-import','POST',JSON.stringify(importedBody))}}});
  assert.equal(reimport.isError,false);assert.deepEqual(JSON.parse(reimport.content[0].text),{productsImported:1,variantsImported:1});
  assert.equal((await value(await call(service.base,`/inventory/${importedVariant.id}`))).stock,10);
  assert.equal((await value(await call(service.base,`/variant/product/${importedProduct.id}`))).length,1);
  assert.equal((await call(service.base,'/catalog-import',{method:'POST',body:{products:[{...importedBody.products[0],variants:[{...importedBody.products[0].variants[0],initialStock:null}]}]}})).status,400);
  const cart={id:'cart'};
  const mcpCart=await mcpRpc(service.base,'tools/call',{name:'createCart',arguments:{body:cart,headers:{authorization:hmac('/cart','POST',JSON.stringify(cart))}}});assert.equal(mcpCart.isError,false);
  await value(await call(service.base,'/cart/items',{method:'POST',body:{cartId:'cart',variantId:'variant',quantity:2}}));
  assert.equal((await value(await call(service.base,'/cart/cart'))).items[0].quantity,2);
  const order={id:'order',items:[{variantId:'variant',quantity:2,unitPriceCents:100}],shippingAddress:{name:'Buyer',line1:'Private Street',city:'Oakland',state:'CA',postalCode:'94601',country:'US'},subtotalCents:200,discountCents:0,taxCents:0,taxBreakdown:[],shippingCents:0,giftCardCents:0,totalCents:200};
  await value(await call(service.base,'/order',{method:'POST',body:order}));
  for(const providerName of ['stripe','paypal']){
    const payment=await value(await call(service.base,'/payment',{method:'POST',body:{orderId:providerName==='stripe'?'order':'paypal-order',amountCents:200,currency:'usd',provider:providerName}}));
    if(providerName==='stripe'){assert.equal(payment.clientSecret,'fixture-client-secret');
      assert.equal((await call(service.base,'/payment',{method:'POST',body:{orderId:'order',amountCents:200,currency:'usd',provider:'paypal'}})).status,409);
    }
    const read=await value(await call(service.base,`/payment/${payment.id}`));assert.ok(!('clientSecret'in read));
  }
  assert.equal(stripeCalls,1);assert.equal(paypalCalls,1);
  const port=await freePort(),controlToken='fixture-worker-control-token-32-characters';
  const worker=await start({worker:true,extra:{WORKER_HTTP_ADDR:`127.0.0.1:${port}`,WORKER_CONTROL_TOKEN:controlToken}});
  const workerBase=`http://127.0.0.1:${port}`;
  await eventually(async()=>{try{return(await fetch(workerBase+'/ready')).status===200;}catch{return false;}});
  assert.equal((await fetch(workerBase+'/control/pause',{method:'POST'})).status,401);
  const control=async action=>{const response=await fetch(workerBase+`/control/${action}`,{method:'POST',headers:{authorization:`Bearer ${controlToken}`}});assert.equal(response.status,202,await response.text());};
  await control('pause');assert.equal((await fetch(workerBase+'/ready')).status,503);
  await value(await call(service.base,'/order/order/transition',{method:'PUT',body:{to:'paid'}}));
  assert.equal((await value(await call(service.base,'/inventory/variant'))).stock,8);
  await control('resume');
  await eventually(async()=>(await value(await call(service.base,'/inventory/variant'))).stock===6);
  await value(await call(service.base,'/order/order/transition',{method:'PUT',body:{to:'cancelled'}}));
  await eventually(async()=>(await value(await call(service.base,'/inventory/variant'))).stock===8);
  const sdkTest=spawn('cargo',['test','-p','ecommerce-sdk','--test','runtime'],{env:{...process.env,ECOMMERCE_BASE_URL:service.base,ECOMMERCE_CATALOG_HEADER:hmac('/product/catalog'),ECOMMERCE_ORDER_HEADER:hmac('/order/order')},stdio:'inherit'});
  assert.equal(await new Promise(resolve=>sdkTest.once('close',resolve)),0);
  await stop(service);service=await start();
  assert.equal((await value(await call(service.base,'/order/order'))).status,'cancelled');
  assert.equal((await value(await call(service.base,'/inventory/variant'))).stock,8);
  await stop(worker);assert.match(worker.stdout(),/completed=2/);
  await compareCheckout({base:service.base,call,providerBase,setProviderFailure(value){providerFailure=value;}});
  const peer=await start();
  const shippingAddress={name:'Buyer',line1:'Private Street',city:'Oakland',state:'CA',postalCode:'94601',country:'US'};
  async function checkoutCart(id){await value(await call(service.base,'/cart',{method:'POST',body:{id}}));await value(await call(service.base,'/cart/items',{method:'POST',body:{cartId:id,variantId:'variant',quantity:2}}));return{cartId:id,shippingAddress};}
  const raceBody=await checkoutCart('cross-process-race'),beforeRace=stripeCalls;checkoutTaxDelay=150;
  const raced=await Promise.all([service,peer].map(instance=>call(instance.base,'/checkout',{method:'POST',body:raceBody}).then(value)));
  assert.equal(raced[0].order.id,raced[1].order.id);assert.equal(raced[0].payment.id,raced[1].payment.id);assert.equal(stripeCalls-beforeRace,1);checkoutTaxDelay=0;
  const parallelBodies=await Promise.all(Array.from({length:10},(_,index)=>checkoutCart(`parallel-checkout-${index}`)));
  const parallel=await Promise.all(parallelBodies.map((body,index)=>call(index%2?peer.base:service.base,'/checkout',{method:'POST',body}).then(value)));
  assert.equal(new Set(parallel.map(result=>result.order.id)).size,10);
  const mcpCheckoutBody=await checkoutCart('mcp-checkout');
  const mcpCheckout=await mcpRpc(service.base,'tools/call',{name:'checkout',arguments:{body:mcpCheckoutBody,headers:{authorization:hmac('/checkout','POST',JSON.stringify(mcpCheckoutBody))}}});
  assert.equal(mcpCheckout.isError,false,JSON.stringify(mcpCheckout));assert.equal(JSON.parse(mcpCheckout.content[0].text).order.totalCents,813);
  const liveSdk=sdk.createForkLaunchSdk({serviceBaseUrls:{ecommerce:service.base}});
  const sdkCheckoutBody=await checkoutCart('sdk-checkout');
  const sdkCheckout=await liveSdk.ecommerce.checkout.checkout({body:sdkCheckoutBody,headers:{authorization:hmac('/checkout','POST',JSON.stringify(sdkCheckoutBody))}});
  assert.equal(sdkCheckout.code,200);assert.equal(sdkCheckout.response.order.totalCents,813);
  console.log('PASS generated checkout cross-process race: one order/payment/provider call, and ten concurrent carts without application-pool starvation');

  // A concurrent cancellation while Stripe is being read must withhold a new
  // secret in both generated transports, while retaining the durable reference.
  for(const transport of ['http','mcp']){
    const body=await checkoutCart(`cancel-during-resume-${transport}`);
    const initial=await value(await call(service.base,'/checkout',{method:'POST',body}));
    const cleared=await call(service.base,`/cart/${body.cartId}`,{method:'DELETE'});
    assert.equal(cleared.status,200);assert.equal(await cleared.text(),'Cleared cart');
    const beforeCalls=stripeCalls;
    beforeIntentRead=async()=>{
      await value(await call(peer.base,`/order/${initial.order.id}/transition`,{method:'PUT',body:{to:'cancelled'}}));
    };
    if(transport==='http'){
      const rejected=await call(service.base,'/checkout',{method:'POST',body});
      assert.equal(rejected.status,502);assert.ok(!(await rejected.text()).includes('fixture-client-secret'));
    }else{
      const rejected=await mcpRpc(service.base,'tools/call',{name:'checkout',arguments:{body,headers:{authorization:hmac('/checkout','POST',JSON.stringify(body))}}});
      assert.equal(rejected.isError,true,JSON.stringify(rejected));assert.ok(!JSON.stringify(rejected).includes('fixture-client-secret'));
    }
    assert.equal(beforeIntentRead,undefined,'provider read was reached');
    assert.equal(stripeCalls,beforeCalls,'resume never creates another provider payment');
    const durable=await value(await call(service.base,`/payment/${initial.payment.id}`));
    assert.equal(durable.providerRef,initial.payment.providerRef);assert.equal(durable.orderId,initial.order.id);
    assert.equal((await value(await call(peer.base,`/order/${initial.order.id}`))).status,'cancelled');
  }
  console.log('PASS generated HTTP/MCP checkout denies secrets on cancellation during provider read and retains payment references');

  const webhookInventory=await compareWebhooks({base:service.base,otherBase:other.base,peerBase:peer.base,call,providerBase,bridgeBinary,setCaptureFailure(value){captureFailure=value;},captureCount:()=>captures,mcpRpc,sdk});
  const webhookWorker=await start({worker:true});
  await eventually(async()=>(await value(await call(service.base,`/inventory/${webhookInventory.variantId}`))).stock===webhookInventory.expectedStock);
  await stop(webhookWorker);
  console.log('PASS verified provider callbacks deliver durable inventory events through the generated worker');
  await stop(peer);await stop(service);await stop(other);
  console.log('PASS generated ecommerce HTTP/MCP, both SDKs, Stripe/PayPal, tenant isolation, encrypted orders, separate inventory worker and restart persistence');
}finally{
  for(const instance of children)if(instance.child.exitCode===null)instance.child.kill('SIGKILL');
  await Promise.all(children.map(instance=>instance.exited));
  await new Promise(resolve=>provider.close(resolve));
}
