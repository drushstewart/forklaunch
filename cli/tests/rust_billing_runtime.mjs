import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
import assert from 'node:assert/strict';
import {generateKeyPairSync,sign,createHmac,randomUUID} from 'node:crypto';
import http from 'node:http';
import {spawn} from 'node:child_process';
import {pathToFileURL} from 'node:url';
const [baseBinary,stripeBinary,sdkPath]=process.argv.slice(2),role=process.env.FORKLAUNCH_TEST_ROLE;
const secret='billing-fixture-hmac',issuer='https://issuer.example',audience='billing-fixture';
const {privateKey,publicKey}=generateKeyPairSync('rsa',{modulusLength:2048}),jwk={...publicKey.export({format:'jwk'}),kid:'billing-fixture',alg:'RS256',use:'sig'};
const jwks=http.createServer((_request,response)=>{response.setHeader('content-type','application/json');response.end(JSON.stringify({keys:[jwk]}));});
const records=new Map(),counts={};let providerCalls=0;
const provider=http.createServer(async(request,response)=>{
 providerCalls++;assert.equal(request.headers.authorization,'Bearer sk_fixture');assert.equal(request.headers['stripe-version'],'2026-08-26.dahlia');
 let raw='';for await(const chunk of request)raw+=chunk;const url=new URL(request.url,'http://fixture'),fields=new URLSearchParams(request.method==='GET'?url.search:raw);
 const match=url.pathname.match(/^\/v1\/(plans|subscriptions|checkout\/sessions|payment_links|billing_portal\/sessions)(?:\/([^/]+))?(?:\/(resume|expire))?$/);assert.ok(match,url.pathname);
 const [,resource,existing,action]=match,prefix={plans:'plan_',subscriptions:'sub_','checkout/sessions':'cs_',payment_links:'plink_','billing_portal/sessions':'bps_'}[resource];
 response.setHeader('content-type','application/json');
 if(request.method==='GET'&&!existing){response.end(JSON.stringify({object:'list',data:[...records.values()].filter(value=>value.id.startsWith(prefix)),has_more:false}));return;}
 const id=existing??prefix+(counts[resource]=(counts[resource]??0)+1);let value=records.get(id);
 if(existing&&!value){response.writeHead(404);response.end('{"error":{"type":"invalid_request_error","message":"missing private-provider-value"}}');return;}
 if(!existing){value={id,product:{id:fields.get('product')??'prod_fixture',private:'private-provider-value'},interval:fields.get('interval')??'month',currency:fields.get('currency')??'USD',amount:Number(fields.get('amount')??299),url:'https://provider.example/'+id,items:{data:[{id:'si_fixture',plan:{id:fields.get('items[0][plan]')??'plan_fixture'}}]},line_items:{data:[{amount_total:299}]}};records.set(id,value);}
 if(request.method==='DELETE'){if(resource==='subscriptions')value.status='canceled';else{records.delete(id);value={id,deleted:true};}}
 if(action==='resume'&&value.status==='canceled'){response.writeHead(400);response.end('{"error":{"type":"invalid_request_error","message":"Cannot resume cancelled subscription"}}');return;}
 if(request.method==='POST'&&existing&&!action){if(fields.has('metadata[status]'))value.metadata={status:fields.get('metadata[status]')};if(fields.has('items[0][plan]'))value.items.data[0].plan.id=fields.get('items[0][plan]');}
 response.end(JSON.stringify(value));
});
await Promise.all([jwks,provider].map(server=>new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))));
const children=[];
function hmac(path,method='GET',body){const ts=new Date().toISOString(),nonce=randomUUID(),serialized=body===undefined?'undefined':`${body}\n`;return `HMAC keyId=default ts=${ts} nonce=${nonce} signature=${createHmac('sha256',secret).update(`${method}\n${path}\n${serialized}${ts}\n${nonce}`).digest('base64')}`;}
function jwt(roles=['admin']){const header=Buffer.from(JSON.stringify({alg:'RS256',kid:jwk.kid,typ:'JWT'})).toString('base64url'),payload=Buffer.from(JSON.stringify({sub:'billing-admin',roles,iss:issuer,aud:audience,exp:Math.floor(Date.now()/1000)+300})).toString('base64url');return `Bearer ${header}.${payload}.${sign('RSA-SHA256',Buffer.from(`${header}.${payload}`),privateKey).toString('base64url')}`;}
async function start(binary,tenant='a',extra={}){
 const db=new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);db.username=role;db.password='parity-test-only';
 const env={...Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.startsWith('OTEL_')&&!key.startsWith('MCP_'))),DATABASE_URL:db.toString(),TENANT_ID:`${role}-${tenant}`,ENCRYPTION_KEY:'billing-fixture-encryption-only',HOST:'127.0.0.1',SERVICE_PORT:'0',HMAC_SECRET_KEY:secret,JWKS_PUBLIC_KEY_URL:`http://127.0.0.1:${jwks.address().port}/jwks`,JWT_ISSUER:issuer,JWT_AUDIENCE:audience,STRIPE_SECRET_KEY:'sk_fixture',STRIPE_API_BASE:`http://127.0.0.1:${provider.address().port}`,...extra};
 const child=spawn(binary,[],{env,stdio:['ignore','pipe','pipe']});let output='',errors='';child.stdout.on('data',chunk=>output+=chunk);child.stderr.on('data',chunk=>errors+=chunk);const exit=new Promise(resolve=>child.once('close',resolve)),info={child,exit,errors:()=>errors};children.push(info);
 for(let i=0;i<300;i++){const base=output.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];if(base)return{...info,base};assert.equal(child.exitCode,null,errors);await new Promise(resolve=>setTimeout(resolve,25));}throw Error(errors);
}
async function stop(info){if(info.child.exitCode===null)info.child.kill('SIGTERM');const timer=setTimeout(()=>info.child.kill('SIGKILL'),10000);try{assert.equal(await info.exit,0,info.errors());}finally{clearTimeout(timer);}}
async function call(base,path,method='GET',body,status=200,auth){const raw=body===undefined?undefined:JSON.stringify(body),response=await fetch(base+path,{method,body:raw,headers:{authorization:auth??hmac(path.split('?')[0],method,raw),...(raw?{'content-type':'application/json'}:{})},signal:AbortSignal.timeout(10000)});assert.equal(response.status,status,await response.clone().text());return response.headers.get('content-type')?.includes('application/json')?response.json():response.text();}
try{
 const sdk=await import(pathToFileURL(sdkPath));
 for(const [binary,stripe,service]of [[baseBinary,false,'billing'],[stripeBinary,true,'stripe']]){
  const running=await start(binary),base=running.base,client=sdk.createForkLaunchSdk({serviceBaseUrls:{[service]:base}});
  const tools=await probeMcp(base,['createPlan','getSubscription','createCheckoutSession','createPaymentLink','createBillingPortalSession']);assert.equal(tools.length,33);
  assert.equal((await fetch(base+'/plan')).status,401);
  await call(base,'/compliance/export/user','GET',undefined,403,jwt(['admin']));
  await call(base,'/compliance/export/user','GET',undefined,404,jwt(['system']));
  await call(base,'/compliance/erase/user','DELETE',undefined,404,jwt(['system']));
  const providerFields=stripe?{stripeFields:{}}:{providerFields:{private:'private-provider-value'}};
  const planBody={name:'prod_fixture',price:1999,cadence:stripe?'month':'MONTHLY',currency:'USD',externalId:randomUUID(),billingProvider:stripe?'stripe':'none',active:true,...providerFields};
  await call(base,'/plan','POST',{...planBody,currency:'INVALID'},400);
  const created=await client[service].plan.createPlan({body:planBody,headers:{authorization:hmac('/plan','POST',JSON.stringify(planBody))}});assert.equal(created.code,200);const plan=created.response;
  await call(base,`/plan/${plan.id}`);assert.equal((await call(base,'/plan')).length>=1,true);
  await call(base,'/plan','PUT',{id:plan.id,price:0});
  const mcp=await mcpRpc(base,'tools/call',{name:'listPlans',arguments:{query:{ids:[plan.id]},headers:{authorization:hmac('/plan')}}});assert.equal(mcp.isError,false);assert.equal(JSON.parse(mcp.content[0].text)[0].id,plan.id);
  const denied=await mcpRpc(base,'tools/call',{name:'listPlans',arguments:{}});assert.equal(denied.isError,true);
  const subBody={partyId:randomUUID(),partyType:'user',productId:plan.id,active:true,externalId:randomUUID(),billingProvider:stripe?'stripe':'none',startDate:new Date().toISOString(),status:'PENDING',...providerFields};
  const sub=await call(base,'/subscription','POST',subBody);await call(base,`/subscription/${sub.id}`);await call(base,`/subscription/user/${subBody.partyId}`);
  const org=await call(base,'/subscription','POST',{...subBody,partyId:randomUUID(),partyType:'organization',externalId:randomUUID()});await call(base,`/subscription/organization/${org.partyId}`);
  await call(base,`/subscription/${sub.id}`,'PUT',{id:sub.id,productId:plan.id,status:'COMPLETED'});await call(base,'/subscription');await call(base,`/subscription/${sub.id}/cancel`);await call(base,`/subscription/${sub.id}/resume`,'GET',undefined,stripe?502:200);
  const checkoutBody={customerId:'cus_fixture',paymentMethods:[stripe?'card':'credit_card'],currency:'USD',uri:'https://provider.example/initial',expiresAt:new Date().toISOString(),status:'PENDING',...providerFields};
  for(const suffix of ['expire','success','failure']){const checkout=await call(base,'/checkout-session','POST',checkoutBody);await call(base,`/checkout-session/${checkout.id}`);await call(base,`/checkout-session/${checkout.id}/${suffix}`);await call(base,`/checkout-session/${checkout.id}`,'GET',undefined,404);}
  for(const suffix of ['expire','success','failure']){const link=await call(base,'/payment-link','POST',{amount:299,paymentMethods:checkoutBody.paymentMethods,currency:'USD',status:'PENDING',...providerFields});await call(base,`/payment-link/${link.id}`);await call(base,`/payment-link/${link.id}`,'PUT',{id:link.id,amount:0});await call(base,'/payment-link');await call(base,`/payment-link/${link.id}`+(suffix==='expire'?'':'/'+suffix),suffix==='expire'?'DELETE':'GET');await call(base,`/payment-link/${link.id}`,'GET',undefined,404);}
  await call(base,'/billing-portal','POST',{customerId:'cus_fixture',expiresAt:new Date().toISOString(),...providerFields},403,jwt(['viewer']));
  const portal=await call(base,'/billing-portal','POST',{customerId:'cus_fixture',expiresAt:new Date().toISOString(),...providerFields},200,jwt());await call(base,`/billing-portal/${portal.id}`,'GET',undefined,200,jwt());const updated=await call(base,`/billing-portal/${portal.id}`,'PUT',{id:portal.id,...providerFields},200,jwt());assert.equal(updated.id,portal.id);
  const rustBody=JSON.stringify({...planBody,externalId:randomUUID()});
  const rust=spawn('cargo',['test','-p',service+'-sdk','--test','runtime','--','--nocapture'],{env:{...process.env,BILLING_BASE_URL:base,BILLING_PLAN_ID:plan.id,BILLING_GET_HEADER:hmac('/plan/'+plan.id),BILLING_LIST_HEADER:hmac('/plan'),BILLING_CREATE_BODY:rustBody,BILLING_CREATE_HEADER:hmac('/plan','POST',rustBody),BILLING_PORTAL_ID:portal.id,BILLING_PORTAL_HEADER:jwt()},stdio:['ignore','pipe','pipe']});let rustOutput='';rust.stdout.on('data',chunk=>rustOutput+=chunk);rust.stderr.on('data',chunk=>rustOutput+=chunk);const rustTimer=setTimeout(()=>rust.kill('SIGKILL'),120000);try{assert.equal(await new Promise(resolve=>rust.once('close',resolve)),0,rustOutput);}finally{clearTimeout(rustTimer);}
  await call(base,`/billing-portal/${portal.id}`,'DELETE',undefined,200,jwt());
  const other=await start(binary,'b');await call(other.base,`/plan/${plan.id}`,'GET',undefined,404);await stop(other);
  await stop(running);const reopened=await start(binary);await call(reopened.base,`/plan/${plan.id}`);await call(reopened.base,`/subscription/${sub.id}`);await stop(reopened);
 }
 for(const config of [{MCP_ENABLED:'false'},{MCP_ENABLED:'true',MCP_PATH:'/tools'}]){const info=await start(baseBinary,'a',config);assert.equal((await fetch(info.base+'/health')).status,200);assert.equal((await fetch(info.base+'/mcp',{method:'POST'})).status,404);if(config.MCP_ENABLED==='true')await probeMcp(info.base,[],config.MCP_PATH);await stop(info);}
 assert.ok(providerCalls>20);
 console.log('PASS generated base/Stripe billing: all five service families over HTTP, authenticated MCP tools, both SDKs, JWT roles, configured party lookups, encrypted persistence, tenant isolation, restart, MCP settings and graceful shutdown');
}finally{for(const info of children)if(info.child.exitCode===null)await stop(info);await Promise.all([jwks,provider].map(server=>new Promise(resolve=>server.close(resolve))));}
