import assert from 'node:assert/strict';
import {createHmac,randomUUID} from 'node:crypto';
import http from 'node:http';
import net from 'node:net';
import {spawn} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
const [binary,sdkPath]=process.argv.slice(2);
const role=process.env.FORKLAUNCH_TEST_ROLE,secret='ecommerce-generated-hmac-secret';
const children=[];let stripeCalls=0,paypalCalls=0;
const provider=http.createServer(async(req,res)=>{
  let raw='';for await(const chunk of req)raw+=chunk;
  res.setHeader('content-type','application/json');
  if(req.url==='/v1/payment_intents'){
    stripeCalls++;assert.equal(req.headers.authorization,'Bearer sk_fixture');
    const body=new URLSearchParams(raw);assert.equal(body.get('amount'),'200');assert.equal(body.get('currency'),'usd');
    res.end(JSON.stringify({id:`pi_fixture_${stripeCalls}`,client_secret:'fixture-client-secret'}));
  }else if(req.url==='/v1/oauth2/token'){
    assert.equal(req.headers.authorization,`Basic ${Buffer.from('paypal-id:paypal-secret').toString('base64')}`);
    res.end(JSON.stringify({access_token:'fixture-access-token',token_type:'Bearer',expires_in:3600}));
  }else if(req.url==='/v2/checkout/orders'){
    paypalCalls++;assert.equal(req.headers.authorization,'Bearer fixture-access-token');
    assert.equal(JSON.parse(raw).purchase_units[0].amount.value,'2.00');
    res.end(JSON.stringify({id:`paypal_fixture_${paypalCalls}`,status:'CREATED'}));
  }else{res.writeHead(404);res.end('{}');}
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
  const env={...Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.startsWith('OTEL_'))),DATABASE_URL:db.toString(),TENANT_ID:`${role}-${tenant}`,ENCRYPTION_KEY:'ecommerce-fixture-key',HOST:'127.0.0.1',SERVICE_PORT:'0',HMAC_SECRET_KEY:secret,JWKS_PUBLIC_KEY_URL:`${providerBase}/jwks`,STRIPE_API_KEY:'sk_fixture',STRIPE_API_BASE:providerBase,PAYPAL_CLIENT_ID:'paypal-id',PAYPAL_CLIENT_SECRET:'paypal-secret',PAYPAL_BASE_URL:providerBase,WORKER_POLL_MS:'20',...extra};
  if(worker)for(const key of ['HMAC_SECRET_KEY','JWKS_PUBLIC_KEY_URL','STRIPE_API_KEY','STRIPE_API_BASE','PAYPAL_CLIENT_ID','PAYPAL_CLIENT_SECRET','PAYPAL_BASE_URL'])delete env[key];
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
  for(const instance of [service,other])assert.equal((await probeMcp(instance.base,['createProduct','listCatalog','createCart','transitionOrder','createPayment'])).length,28);
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
  const cart={id:'cart'};
  const mcpCart=await mcpRpc(service.base,'tools/call',{name:'createCart',arguments:{body:cart,headers:{authorization:hmac('/cart','POST',JSON.stringify(cart))}}});assert.equal(mcpCart.isError,false);
  await value(await call(service.base,'/cart/items',{method:'POST',body:{cartId:'cart',variantId:'variant',quantity:2}}));
  assert.equal((await value(await call(service.base,'/cart/cart'))).items[0].quantity,2);
  const order={id:'order',items:[{variantId:'variant',quantity:2,unitPriceCents:100}],shippingAddress:{name:'Buyer',line1:'Private Street',city:'Oakland',state:'CA',postalCode:'94601',country:'US'},subtotalCents:200,discountCents:0,taxCents:0,taxBreakdown:[],shippingCents:0,giftCardCents:0,totalCents:200};
  await value(await call(service.base,'/order',{method:'POST',body:order}));
  for(const providerName of ['stripe','paypal']){
    const payment=await value(await call(service.base,'/payment',{method:'POST',body:{orderId:'order',amountCents:200,currency:'usd',provider:providerName}}));
    if(providerName==='stripe')assert.equal(payment.clientSecret,'fixture-client-secret');
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
  await stop(service);await stop(other);
  console.log('PASS generated ecommerce HTTP/MCP, both SDKs, Stripe/PayPal, tenant isolation, encrypted orders, separate inventory worker and restart persistence');
}finally{
  for(const instance of children)if(instance.child.exitCode===null)instance.child.kill('SIGKILL');
  await Promise.all(children.map(instance=>instance.exited));
  await new Promise(resolve=>provider.close(resolve));
}
