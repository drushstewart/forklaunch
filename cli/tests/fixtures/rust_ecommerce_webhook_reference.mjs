// Actual TypeScript webhook controller over owned native persistence/provider boundaries.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,realpath} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
export async function compareWebhooks({base,otherBase,peerBase,call,providerBase,bridgeBinary,setCaptureFailure,captureCount,mcpRpc,sdk}){
 const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..'),require=createRequire(import.meta.url);
 const tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
 const ts=require(require.resolve('typescript',{paths:[path.dirname(await realpath(tsc))]}));
 const Stripe=require(process.env.STRIPE_SDK_PATH??require.resolve('stripe',{paths:[path.dirname(await realpath(tsc)),path.join(root,'blueprint/ecommerce-stripe')]}));
 const db=new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);db.username=process.env.FORKLAUNCH_TEST_ROLE;db.password='parity-test-only';
 const bridgeEnv={...process.env,DATABASE_URL:db.toString(),TENANT_ID:`${process.env.FORKLAUNCH_TEST_ROLE}-a`,ENCRYPTION_KEY:'ecommerce-fixture-key',FORKLAUNCH_FIXTURE_PROVIDER_BASE:providerBase};
 // The ledger boundary is in memory here; the actual ledger has its own conformance suite.
 async function rpc(provider,operation,body){
  const {spawn}=await import('node:child_process');const child=spawn(bridgeBinary,[],{env:bridgeEnv,stdio:['pipe','pipe','pipe']});let stdout='',stderr='';
  child.stdout.on('data',data=>stdout+=data);child.stderr.on('data',data=>stderr+=data);child.stdin.end(JSON.stringify({provider,operation,body}));
  const timer=setTimeout(()=>child.kill('SIGKILL'),15000);
  try{assert.equal(await new Promise(resolve=>child.once('close',resolve)),0,stderr);const result=JSON.parse(stdout);if(result.error)throw Object.assign(new Error(result.error),{code:result.error});return result.ok;}finally{clearTimeout(timer);}
 }
 async function load(relative,imports={}){const exports={};vm.runInNewContext(ts.transpileModule(await readFile(path.join(root,relative),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,Buffer,Date,require(spec){if(spec in imports)return imports[spec];throw Error('unexpected runtime import '+spec);}});return exports;}
 async function value(response){assert.equal(response.status,200,await response.clone().text());return response.json();}
 async function json(endpoint,options){return value(await call(base,endpoint,options));}
 const schema={array:value=>value,optional:value=>value,string:{},unknown:{},schemaValidator:{},handlers:{}};
 const webhookSchema=await load('blueprint/ecommerce-stripe/domain/schemas/webhook.schema.ts',{'../../schema':schema});
 const rows=new Map();
 const ledger={async beginProcessing(event){const key=JSON.stringify([event.provider,event.providerEventId]);if(rows.has(key))return rows.get(key)?'already-processed':'retry';rows.set(key,false);return'new';},async markProcessed(event){rows.set(JSON.stringify([event.provider,event.providerEventId]),true);}};
 let activeProvider='stripe';
 const services={
  PaymentService:{confirmPayment:body=>rpc('stripe','confirm',body),failPayment:body=>rpc('stripe','fail',body)},
  PaypalPaymentService:{confirmPayment:body=>rpc('paypal','confirm',body),failPayment:body=>rpc('paypal','fail',body)},
  OrderService:{getOrder:({id})=>json(`/order/${id}`),transitionOrder:body=>rpc(activeProvider,'transition',body)},
  CartService:{async clearCart({id}){const response=await call(base,`/cart/${id}`,{method:'DELETE'});assert.equal(response.status,200);}},
  OrderCartLookupService:{async findCartIdByOrderId(id){return(await json(`/order/${id}`)).cartId??null;}},
  OrderEventProducer:{enqueueJob:body=>rpc(activeProvider,'enqueue',body)},WebhookEventService:ledger,
 };
 const paypalClient={async verifyWebhookSignature(params){
  const body={transmission_id:params.transmissionId,transmission_time:params.transmissionTime,transmission_sig:params.transmissionSig,cert_url:params.certUrl,auth_algo:params.authAlgo,webhook_id:params.webhookId,webhook_event:params.webhookEvent};
  const response=await fetch(providerBase+'/v1/notifications/verify-webhook-signature',{method:'POST',headers:{'content-type':'application/json',authorization:'Bearer fixture-access-token'},body:JSON.stringify(body)});
  return response.ok&&(await response.json()).verification_status==='SUCCESS';
 }};
 const tokens=new Proxy({}, {get:(_,key)=>key}),telemetry={info(){},warn(){},error(){}};
 const ci={resolve:token=>({PaypalClient:paypalClient,STRIPE_WEBHOOK_SECRET:'whsec_fixture',PAYPAL_WEBHOOK_ID:'paypal-webhook-fixture'}[token]??telemetry),scopedResolver:token=>()=>services[token]};
 const handlers={};schema.handlers.post=(_validator,route,_options,callback)=>{handlers[route]=callback;return callback;};
 await load('blueprint/ecommerce-stripe/api/controllers/webhook.controller.ts',{'node:crypto':require('node:crypto'),'../../schema':schema,'@forklaunch/interfaces-ecommerce/types':{OrderStatus:{PENDING:'pending',PAID:'paid'}},stripe:{default:Stripe},'../../domain/schemas/webhook.schema':webhookSchema,'../../bootstrapper':{ci,tokens}});
 const created=[];let serial=0,cases=0;
 const address={name:'Buyer',line1:'Private Street',city:'Oakland',state:'CA',postalCode:'94601',country:'US'};
 await json('/catalog-import',{method:'POST',body:{products:[{externalId:'webhook-product',handle:'webhook-owned',title:'Callback fixture',variants:[{externalId:'webhook-variant',title:'Box',priceCents:100,initialStock:100}]}]}});
 const product=await json('/product/handle/webhook-owned'),variant=(await json(`/variant/product/${product.id}`))[0];
 async function checkout(provider){const cartId=`webhook-cart-${++serial}`;await json('/cart',{method:'POST',body:{id:cartId}});await json('/cart/items',{method:'POST',body:{cartId,variantId:variant.id,quantity:1}});const result=await json('/checkout',{method:'POST',body:{cartId,shippingAddress:address,provider}});created.push({...result,cartId});return{...result,cartId};}
 function event(provider,purchase,label,changes={}){
  if(provider==='stripe')return{id:`evt-${label}`,type:'payment_intent.succeeded',data:{object:{id:purchase.payment.providerRef,amount:purchase.payment.amountCents,currency:'usd',...changes}},extra:{untouched:'💬'}};
  return{id:`evt-${label}`,event_type:'CHECKOUT.ORDER.APPROVED',resource:{id:purchase.payment.providerRef,purchase_units:[{amount:{value:(purchase.payment.amountCents/100).toFixed(2),currency_code:'USD'}}],...changes},create_time:'2026-10-11T00:00:00Z',links:[{href:'https://provider.example/original'}]};
 }
 function headers(provider,raw,bad=false){return provider==='stripe'?{'stripe-signature':bad?'invalid':Stripe.webhooks.generateTestHeaderString({payload:raw,secret:'whsec_fixture'})}:{'paypal-transmission-id':'fixture-transmission','paypal-transmission-time':new Date().toISOString(),'paypal-transmission-sig':bad?'invalid':'valid','paypal-cert-url':providerBase+'/must-not-fetch-certificate','paypal-auth-algo':'SHA256withRSA'};}
 async function send(provider,payload,{reference=false,bad=false,target=base}={}){
  const raw=JSON.stringify(payload),requestHeaders=headers(provider,raw,bad);
  if(reference){let code,response;activeProvider=provider;
   try{await handlers[`/${provider}`]({body:provider==='paypal'?{id:payload.id,event_type:payload.event_type,resource:payload.resource}:payload,rawBody:Buffer.from(raw),headers:requestHeaders},{status(value){code=value;return this;},send(value){response=value;}});}
   catch{code=500;response='Webhook handling failed';}
   return{code,response};
  }
  const response=await fetch(target+`/webhook/${provider}`,{method:'POST',headers:{'content-type':'application/json',...requestHeaders},body:raw});return{code:response.status,response:await response.text()};
 }
 async function state(purchase){return{payment:(await json(`/payment/${purchase.payment.id}`)).status,order:(await json(`/order/${purchase.order.id}`)).status,cart:(await json(`/cart/${purchase.cartId}`)).items.length};}
 async function pair(provider,label,modify=payload=>payload){const purchases=[await checkout(provider),await checkout(provider)],events=purchases.map((purchase,index)=>modify(event(provider,purchase,`${label}-${index}`)));return{purchases,events};}
 const successes={};
 for(const provider of ['stripe','paypal']){
  const paired=await pair(provider,`${provider}-paid`);successes[provider]=paired;
  for(let i=0;i<2;i++){const result=await send(provider,paired.events[i],{reference:i===1});assert.equal(result.code,200,JSON.stringify(result));assert.deepEqual(await state(paired.purchases[i]),{payment:'succeeded',order:'paid',cart:0});}
  cases++;
  const before=captureCount();for(let i=0;i<2;i++)assert.equal((await send(provider,paired.events[i],{reference:i===1})).response,'Webhook event already processed');assert.equal(captureCount(),before);cases++;
  const denied=await pair(provider,`${provider}-failed`,payload=>provider==='stripe'?{...payload,type:'payment_intent.payment_failed'}:{...payload,event_type:'PAYMENT.CAPTURE.DENIED',resource:{id:'capture-is-not-the-order',supplementary_data:{related_ids:{order_id:payload.resource.id}}}});
  for(let i=0;i<2;i++){assert.equal((await send(provider,denied.events[i],{reference:i===1})).code,200);assert.deepEqual(await state(denied.purchases[i]),{payment:'failed',order:'pending',cart:1});}cases++;
  const mismatch=await pair(provider,`${provider}-mismatch`,payload=>provider==='stripe'?{...payload,data:{object:{...payload.data.object,amount:1}}}:{...payload,resource:{...payload.resource,purchase_units:[{amount:{value:'0.01',currency_code:'USD'}}]}});
  for(let i=0;i<2;i++){assert.equal((await send(provider,mismatch.events[i],{reference:i===1})).code,200);assert.deepEqual(await state(mismatch.purchases[i]),{payment:'succeeded',order:'pending',cart:1});}cases++;
  const invalid=await pair(provider,`${provider}-invalid`);
  for(let i=0;i<2;i++){assert.equal((await send(provider,invalid.events[i],{reference:i===1,bad:true})).code,400);assert.deepEqual(await state(invalid.purchases[i]),{payment:'pending',order:'pending',cart:1});}cases++;
 }
 const retry=await pair('paypal','paypal-retry');
 for(let i=0;i<2;i++){
  setCaptureFailure(true);assert.equal((await send('paypal',retry.events[i],{reference:i===1})).code,500);assert.deepEqual(await state(retry.purchases[i]),{payment:'pending',order:'pending',cart:1});
  setCaptureFailure(false);assert.equal((await send('paypal',retry.events[i],{reference:i===1})).code,200);assert.deepEqual(await state(retry.purchases[i]),{payment:'succeeded',order:'paid',cart:0});
 }cases++;
 for(const provider of ['stripe','paypal']){
  const unknown=provider==='stripe'?{id:'evt-shared-unknown',type:'unknown',data:{object:{}}}:{id:'evt-shared-unknown',event_type:'UNKNOWN.EVENT',resource:{},create_time:'2026-10-11T00:00:00Z',links:[]};
  assert.equal((await send(provider,unknown)).response,'Webhook event processed');assert.equal((await send(provider,unknown)).response,'Webhook event already processed');cases++;
 }
 assert.equal((await send('stripe',successes.stripe.events[0],{target:otherBase})).code,500,'Other tenant must not reuse receipt/payment');
 const racedPurchase=await checkout('paypal'),racedEvent=event('paypal',racedPurchase,'cross-process');
 const beforeRace=captureCount();const raced=await Promise.all(Array.from({length:8},(_,index)=>send('paypal',racedEvent,{target:index%2?peerBase:base})));
 assert.ok(raced.every(result=>result.code===200),JSON.stringify(raced));assert.equal(captureCount()-beforeRace,1);
 assert.equal(raced.filter(result=>result.response==='Webhook event processed').length,1);
 assert.deepEqual(await state(racedPurchase),{payment:'succeeded',order:'paid',cart:0});
 const distinctPurchase=await checkout('paypal'),distinctEvent=event('paypal',distinctPurchase,'different-events');const beforeDistinct=captureCount();
 const distinct=await Promise.all([send('paypal',distinctEvent),send('paypal',{...distinctEvent,id:distinctEvent.id+'-second'},{target:peerBase})]);
 assert.ok(distinct.every(result=>result.code===200),JSON.stringify(distinct));assert.equal(captureCount()-beforeDistinct,1);
 assert.deepEqual(await state(distinctPurchase),{payment:'succeeded',order:'paid',cart:0});
 cases+=2;
 const sdkPurchase=await checkout('stripe'),sdkEvent=event('stripe',sdkPurchase,'sdk-webhook');
 const sdkResult=await sdk.createForkLaunchSdk({serviceBaseUrls:{ecommerce:base}}).ecommerce.webhook.handleStripeWebhook({body:sdkEvent,headers:headers('stripe',JSON.stringify(sdkEvent))});assert.equal(sdkResult.code,200);assert.equal(sdkResult.response,'Webhook event processed');
 const mcpDenied=await mcpRpc(base,'tools/call',{name:'handleStripeWebhook',arguments:{body:sdkEvent,headers:{'stripe-signature':'invalid'}}});assert.equal(mcpDenied.isError,true);
 let paid=0;for(const purchase of created)if((await state(purchase)).order==='paid')paid++;
 console.log(`PASS ${cases-4} actual TypeScript/Rust provider webhook controller comparisons and 4 native replay/concurrency scenarios over restricted PostgreSQL/provider boundaries: verification, exact PayPal event preservation, paid/failed/mismatch, capture rollback/retry, replay, tenant isolation, SDK and MCP guards`);
 return{variantId:variant.id,expectedStock:100-paid};
}
