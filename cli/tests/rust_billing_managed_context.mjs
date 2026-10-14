// Owned platform-protocol/native/mock conformance, without external Stripe calls.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {createRequire} from 'node:module';
import {spawn,execFileSync} from 'node:child_process';
import http from 'node:http';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const platform=process.env.FORKLAUNCH_PLATFORM_ROOT;
assert(platform,'Set FORKLAUNCH_PLATFORM_ROOT to companion source checkout');
const require=createRequire(import.meta.url),ts=require(process.env.TYPESCRIPT_PATH ?? '/private/tmp/fl-managed-payments-context-dependencies/node_modules/typescript');
const work=await mkdtemp(path.join(tmpdir(),'fl-managed-context-'));
const source=await readFile(path.join(platform,'src/modules/managed-apps/domain/services/payments-context.ts'),'utf8');
const emit=text=>ts.transpileModule(text,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText;
const compiled=emit(source);
assert.equal(await readFile(path.join(root,'framework/core/bin/gateway-mock/routes/payments-context.mjs'),'utf8'),'// Mirrors companion managed-apps/domain/services/payments-context.ts; tested by source conformance.\n'+compiled);
await writeFile(path.join(work,'protocol.mjs'),compiled);
await writeFile(path.join(work,'safe.mjs'),emit(await readFile(path.join(root,'framework/common/src/safeStringify.ts'),'utf8')));
await writeFile(path.join(work,'hmac.mjs'),emit((await readFile(path.join(root,'framework/core/src/http/createHmacToken.ts'),'utf8')).replace("'@forklaunch/common'","'./safe.mjs'")));
await writeFile(path.join(work,'verify.mjs'),emit((await readFile(path.join(platform,'src/modules/managed-apps/domain/utils/instance-hmac.util.ts'),'utf8')).replace("'@forklaunch/core/http'","'./hmac.mjs'")));
const {PaymentsContextProtocol}=await import(pathToFileURL(path.join(work,'protocol.mjs')));
const {parseHmacAuthorization,verifyInstanceHmac}=await import(pathToFileURL(path.join(work,'verify.mjs')));
const {createHmacToken}=await import(pathToFileURL(path.join(work,'hmac.mjs')));
if(process.env.FORKLAUNCH_SKIP_CARGO_BUILD !== '1') execFileSync('cargo',['build','--quiet','--manifest-path','blueprint-rs/Cargo.toml','-p','forklaunch-billing-stripe','--example','client_parity'],{cwd:root,stdio:'inherit'});
const binary=path.join(root,'blueprint-rs/target/debug/examples/client_parity');
async function native(base,actions){return await new Promise((resolve,reject)=>{const child=spawn(binary,[],{cwd:root,stdio:['pipe','pipe','inherit']});let out='';child.stdout.on('data',v=>out+=v);child.on('error',reject);child.on('close',code=>code?reject(new Error(`native exit ${code}`)):resolve(JSON.parse(out)));child.stdin.end(JSON.stringify({managed:true,base,actions}));});}
let generation='00000000-0000-4000-8000-000000000001',scenario='ok';const calls=[],nonces=new Set();
const owner={id:'instance_fixture',organizationId:'org_fixture'};
const protocol=new PaymentsContextProtocol('opaque-owned-key','https://owned.stripe.test',async request=>{
  calls.push(request);assert.equal(request.method,'GET');assert.equal(request.stripeAccount,'acct_fixture');
  if(scenario==='rebind'&&(request.path.includes('/checkout/')||request.path.includes('/payment_links/')))generation=crypto.randomUUID();
  return {status:200,body:JSON.stringify(request.path.includes('/accounts/')?{object:'account',id:'acct_fixture'}:request.path==='/v1/balance'?{object:'balance',livemode:false}:request.path.includes('/payment_links/')?{object:'payment_link',id:'plink_fixture',livemode:false}:{object:'checkout.session',id:'cs_fixture',livemode:false,customer:'cus_fixture',mode:'subscription'})};
});
const read=async()=>({instanceId:owner.id,organizationId:owner.organizationId,stripeAccountId:'acct_fixture',bindingGeneration:generation});
const server=http.createServer(async(req,res)=>{try{
  let raw='';for await(const chunk of req)raw+=chunk;
  const auth=parseHmacAuthorization(req.headers.authorization);assert(auth);assert.equal(auth.keyId,owner.id);assert(!nonces.has(auth.nonce));nonces.add(auth.nonce);
  assert(verifyInstanceHmac({secretKey:'instance-secret-fixture',method:req.method,path:req.url.replace('/instance-gateway',''),body:raw,auth}));
  const request=JSON.parse(raw);let answer;
  if(req.url==='/instance-gateway/payments/context')answer=await protocol.issue(owner,request,read);
  else {assert(['/instance-gateway/payments/context/checkout-read','/instance-gateway/payments/context/payment-link-read'].includes(req.url));answer=await protocol[req.url.endsWith('/payment-link-read')?'paymentLink':'checkout'](owner,request,read);if(scenario==='bad-envelope')answer.scope={...answer.scope,organizationId:'other'};}
  res.setHeader('content-type','application/json');res.end(JSON.stringify(answer));
}catch{res.statusCode=409;res.end('{}');}});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let mock;
try {
  const base=`http://127.0.0.1:${server.address().port}`;
  const action={resource:'context',method:'verify',account:'acct_fixture',livemode:false,checkoutId:'cs_fixture'};
  assert.equal((await native(base,[action]))[0].ok.id,'cs_fixture');
  for(const kind of ['rebind','bad-envelope']){scenario=kind;assert.equal((await native(base,[action]))[0].error,'billing_provider_context_mismatch');}
  const linkAction={resource:'context',method:'verify',account:'acct_fixture',livemode:false,paymentLinkId:'plink_fixture'};
  scenario='ok';assert.equal((await native(base,[linkAction]))[0].ok.id,'plink_fixture');
  for(const kind of ['rebind','bad-envelope']){scenario=kind;assert.equal((await native(base,[linkAction]))[0].error,'billing_provider_context_mismatch');}
  scenario='ok';assert.equal((await native(base,[{...linkAction,paymentLinkId:'cs_fixture'}]))[0].error,'billing_provider_context_mismatch');
  assert(calls.every(c=>c.method==='GET'));
  const {startGatewayMock}=await import(pathToFileURL(path.join(root,'framework/core/bin/gateway-mock/server.mjs')));
  mock=await startGatewayMock({PORT:'0',MOCK_INSTANCE_HMAC_KEY:'instance-secret-fixture'});
  const mockBase=`http://127.0.0.1:${mock.address().port}`;
  async function signed(route,body){const raw=JSON.stringify(body),ts=new Date(),nonce=crypto.randomUUID();return fetch(mockBase+'/instance-gateway'+route,{method:'POST',headers:{'content-type':'application/json',authorization:`HMAC keyId=instance_fixture ts=${ts.toISOString()} nonce=${nonce} signature=${createHmacToken({method:'POST',path:route,body:raw,timestamp:ts,nonce,secretKey:'instance-secret-fixture'})}`},body:raw});}
  assert.equal((await signed('/payments/onboarding-link',{returnUrl:'https://owned.test/return',refreshUrl:'https://owned.test/refresh'})).status,200);
  const created=(await native(mockBase,[{resource:'checkout',method:'create',fields:{customer:'cus_fixture',mode:'subscription'}}]))[0].ok;
  assert.equal((await native(mockBase,[{...action,account:'acct_mock_0001',checkoutId:created.id}]))[0].ok.id,created.id);
  const createdLink=(await native(mockBase,[{resource:'links',method:'create',fields:{line_items:[{price:'price_fixture',quantity:1}]}}]))[0].ok;
  assert.equal((await native(mockBase,[{...linkAction,account:'acct_mock_0001',paymentLinkId:createdLink.id}]))[0].ok.id,createdLink.id);
  console.log('PASS platform protocol/native signed HTTP: checkout/payment-link valid, rebind, changed envelope, cross-kind denial; gateway mock onboarding/create/context/read; exact source conformance');
} finally {await new Promise(resolve=>server.close(resolve));if(mock)await new Promise(resolve=>mock.close(resolve));await rm(work,{recursive:true,force:true});}
