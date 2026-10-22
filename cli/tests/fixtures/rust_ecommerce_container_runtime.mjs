import assert from 'node:assert/strict';
import {createHmac,randomUUID} from 'node:crypto';
import {probeMcp,mcpRpc} from '../rust_mcp_probe.mjs';
const [base]=process.argv.slice(2),secret='container-ecommerce-hmac-secret';
function sign(path,method='GET',raw){
 const timestamp=new Date().toISOString(),nonce=randomUUID();
 const signature=createHmac('sha256',secret).update(`${method}\n${path}\n${raw===undefined?'undefined':raw+'\n'}${timestamp}\n${nonce}`).digest('base64');
 return `HMAC keyId=default ts=${timestamp} nonce=${nonce} signature=${signature}`;
}
async function call(path,{method='GET',body}={}){
 const raw=body===undefined?undefined:JSON.stringify(body);
 const response=await fetch(base+path,{method,body:raw,headers:{authorization:sign(path,method,raw),...(raw===undefined?{}:{'content-type':'application/json'})}});
 assert.equal(response.status,200,await response.clone().text());return response.json();
}
assert.equal((await probeMcp(base,['createProduct','checkout','handleStripeWebhook','handlePaypalWebhook','createInventory'])).length,33);
assert.equal((await fetch(base+'/product')).status,401);
await call('/catalog-import',{method:'POST',body:{products:[{externalId:'container',handle:'container-product',title:'Container product',variants:[{externalId:'container-variant',title:'Box',priceCents:100,initialStock:10}]}]}});
const product=await call('/product/handle/container-product'),variant=(await call(`/variant/product/${product.id}`))[0];
await call('/order',{method:'POST',body:{id:'order',items:[{variantId:variant.id,quantity:2,unitPriceCents:100}],shippingAddress:{name:'Buyer',line1:'Private Street',city:'Oakland',state:'CA',postalCode:'94601',country:'US'},subtotalCents:200,discountCents:0,taxCents:0,taxBreakdown:[],shippingCents:0,giftCardCents:0,totalCents:200}});
const inventory=await mcpRpc(base,'tools/call',{name:'getInventory',arguments:{params:{variantId:variant.id},headers:{authorization:sign(`/inventory/${variant.id}`)}}});
assert.equal(inventory.isError,false,JSON.stringify(inventory));assert.equal(JSON.parse(inventory.content[0].text).stock,10);
async function stock(expected){for(let attempt=0;attempt<200;attempt++){if((await call(`/inventory/${variant.id}`)).stock===expected)return;await new Promise(resolve=>setTimeout(resolve,50));}throw Error(`Container worker stock did not reach ${expected}`);}
await call('/order/order/transition',{method:'PUT',body:{to:'paid'}});await stock(8);
await call('/order/order/transition',{method:'PUT',body:{to:'cancelled'}});await stock(10);
const badWebhook=await fetch(base+'/webhook/stripe',{method:'POST',headers:{'stripe-signature':'invalid','content-type':'application/json'},body:'{}'});assert.equal(badWebhook.status,400);
console.log('PASS Linux ecommerce HTTP/MCP authorization and discovery, provider callback guard, paid/cancelled inventory worker delivery');
