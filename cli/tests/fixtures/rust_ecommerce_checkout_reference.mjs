// Runs the actual TS checkout controller against owned native service/provider boundaries.
// Base-service, Stripe/PayPal and pricing implementations have separate TS conformance suites.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,realpath} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
export async function compareCheckout({base,call,providerBase,setProviderFailure}){
 const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
 const require=createRequire(import.meta.url),tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
 const ts=require(require.resolve('typescript',{paths:[path.dirname(await realpath(tsc))]}));
 async function load(relative,imports={}){
  const exports={};vm.runInNewContext(ts.transpileModule(await readFile(path.join(root,relative),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,{exports,require(spec){if(spec in imports)return imports[spec];throw Error('unexpected runtime import '+spec);}});return exports;
 }
 class UniqueConstraintViolationException extends Error{}
 async function json(endpoint,options){const response=await call(base,endpoint,options);if(response.status===409)throw new UniqueConstraintViolationException();assert.equal(response.status,200,await response.clone().text());return response.json();}
 const carts=new Map(),payments=new Map();
 const services={
  CartService:{getCart:({id})=>json(`/cart/${id}`)},
  VariantService:{getVariant:({id})=>json(`/variant/${id}`)},
  InventoryService:{checkStock:body=>json('/inventory/check',{method:'POST',body})},
  OrderService:{getOrder:({id})=>json(`/order/${id}`),async createOrder(body,_context,cartId){const order=await json('/order',{method:'POST',body});carts.set(order.id,cartId);return order;},transitionOrder:({id,to})=>json(`/order/${id}/transition`,{method:'PUT',body:{to}})},
  OrderCartLookupService:{async findPendingOrderIdByCartId(cartId){for(const [id,owner]of carts)if(owner===cartId&&(await json(`/order/${id}`)).status==='pending')return id;return null;}},
  PaymentOrderLookupService:{async findPendingPaymentByOrderId(orderId){for(const [id,owner]of payments)if(owner===orderId){const payment=await json(`/payment/${id}`);if(payment.status==='pending'&&payment.providerRef)return payment;}return null;}},
 };
 for(const [token,provider]of [['PaymentService','stripe'],['PaypalPaymentService','paypal']])services[token]={
  async createPayment(body){const payment=await json('/payment',{method:'POST',body:{...body,provider}});payments.set(payment.id,body.orderId);return payment;},
  getPayment:({id})=>json(`/payment/${id}`),
  async resumePayment({id}){const payment=await json(`/payment/${id}`);const response=await fetch(providerBase+`/v1/payment_intents/${payment.providerRef}`);assert.equal(response.status,200);return{...payment,clientSecret:(await response.json()).client_secret};},
 };
 const {FlatRateShippingService}=await load('blueprint/ecommerce-stripe/domain/services/shipping.service.ts');
 const {StripeTaxService}=await load('blueprint/ecommerce-stripe/domain/services/tax.service.ts');
 const telemetry={info(){},warn(){},error(){}};
 services.ShippingService=new FlatRateShippingService();
 services.TaxService=new StripeTaxService({tax:{calculations:{async create(params){
  const form=new URLSearchParams({currency:params.currency,'customer_details[address_source]':params.customer_details.address_source});
  for(let index=0;index<params.line_items.length;index++)for(const[key,value]of Object.entries(params.line_items[index]))form.set(`line_items[${index}][${key}]`,String(value));
  for(const[key,value]of Object.entries(params.customer_details.address))if(value!==undefined)form.set(`customer_details[address][${key}]`,value);
  const response=await fetch(providerBase+'/v1/tax/calculations',{method:'POST',headers:{authorization:'Bearer sk_fixture'},body:form});if(!response.ok)throw Error('Tax unavailable');return response.json();
 }}}},telemetry);
 const tokens=new Proxy({}, {get:(_,key)=>key}),ci={resolve:token=>token==='HMAC_SECRET_KEY'?'fixture':telemetry,scopedResolver:token=>()=>services[token]};let handler;
 await load('blueprint/ecommerce-stripe/api/controllers/checkout.controller.ts',{
  '@forklaunch/interfaces-ecommerce/types':{OrderStatus:{PENDING:'pending',CANCELLED:'cancelled'}},
  '@mikro-orm/core':{UniqueConstraintViolationException},
  '../../schema':{enum_:value=>value,optional:value=>value,string:{},schemaValidator:{},handlers:{post(_validator,_path,_options,callback){handler=callback;}}},
  '../../domain/schemas/checkout.schema':{CheckoutResultSchema:{},ShippingAddressSchema:{}},
  '../../domain/enum/paymentProvider.enum':{PaymentProvider:{STRIPE:'stripe',PAYPAL:'paypal'}},'../../bootstrapper':{ci,tokens},
 });
 async function reference(body){let code,result;await handler({body},{status(value){code=value;return this;},json(value){result=JSON.parse(JSON.stringify(value));},send(value){result=value;}});return{code,response:result};}
 async function native(body){const response=await call(base,'/checkout',{method:'POST',body});return{code:response.status,response:response.status===200?await response.json():await response.text()};}
 function normalize(result){
  if(result.code!==200)return{code:result.code};
  const {order,payment,clientSecret}=result.response;
  const fields=['status','items','shippingAddress','subtotalCents','discountCents','taxCents','taxBreakdown','shippingCents','giftCardCents','totalCents'];
  return{code:200,order:Object.fromEntries(fields.map(field=>[field,order[field]])),payment:{amountCents:payment.amountCents,currency:payment.currency,status:payment.status,provider:payment.providerRef.startsWith('pi_')?'stripe':'paypal'},...(clientSecret===undefined?{}:{clientSecret})};
 }
 const shippingAddress={name:'Buyer',line1:'Private Street',city:'Oakland',state:'CA',postalCode:'94601',country:'US'};
 let cases=0;
 async function pair(label,{quantity=2,provider='stripe',country='US',failure}={}){
  const bodies=[];
  for(const suffix of ['native','ts']){
   const cartId=`checkout-${label}-${suffix}`;await json('/cart',{method:'POST',body:{id:cartId}});
   if(quantity)await json('/cart/items',{method:'POST',body:{cartId,variantId:'variant',quantity}});
   bodies.push({cartId,shippingAddress:{...shippingAddress,country},provider});
  }
  setProviderFailure(failure);const actual=await native(bodies[0]);setProviderFailure(failure);const expected=await reference(bodies[1]);
  assert.deepEqual(normalize(actual),normalize(expected),label);cases++;
  return{bodies,actual,expected};
 }
 await pair('empty',{quantity:0});await pair('insufficient',{quantity:99});
 await pair('international',{provider:'paypal',country:'CA'});
 await pair('estimated',{failure:'tax'});
 const success=await pair('success');assert.equal(success.actual.response.order.totalCents,813);
 // Pending empty-cart retries resume both order and provider payment, even if a
 // different provider is requested. This is the current TS controller contract.
 for(const body of success.bodies)await call(base,`/cart/${body.cartId}`,{method:'DELETE'});
 const resumeBodies=success.bodies.map(body=>({...body,provider:'paypal'}));
 const resumed=[await native(resumeBodies[0]),await reference(resumeBodies[1])];
 assert.deepEqual(normalize(resumed[0]),normalize(resumed[1]));
 assert.equal(resumed[0].response.order.id,success.actual.response.order.id);assert.equal(resumed[0].response.payment.id,success.actual.response.payment.id);cases++;
 // A nonempty cart with a pending order is replaced, preserving the historical order.
 for(const body of success.bodies)await json('/cart/items',{method:'POST',body:{cartId:body.cartId,variantId:'variant',quantity:1}});
 const replaced=[await native(success.bodies[0]),await reference(success.bodies[1])];assert.deepEqual(normalize(replaced[0]),normalize(replaced[1]));
 assert.notEqual(replaced[0].response.order.id,success.actual.response.order.id);
 assert.equal((await json(`/order/${success.actual.response.order.id}`)).status,'cancelled');cases++;
 // External payment initiation failure leaves an honest pending order and the basket intact.
 const failed=await pair('payment-failure',{failure:'payment'});assert.equal(failed.actual.code,502);
 assert.equal((await json(`/cart/${failed.bodies[0].cartId}`)).items.length,1);
 console.log(`PASS ${cases} actual TypeScript/Rust checkout controller cases over owned PostgreSQL/provider boundaries: empty/stock guards, pricing, estimated tax, PayPal, pending resume/provider switch, stale replacement and provider failure`);
}
