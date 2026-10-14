// Actual companion creation protocol + exact HMAC wire + native restricted-PG publication.
import assert from 'node:assert/strict';
import { readFile,writeFile,mkdtemp,rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath,pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';
import { spawn } from 'node:child_process';
import http from 'node:http';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const platform=process.env.FORKLAUNCH_PLATFORM_ROOT;assert(platform,'Set companion checkout FORKLAUNCH_PLATFORM_ROOT');
assert(process.env.FORKLAUNCH_TEST_POSTGRES_URL,'Set explicitly owned PostgreSQL fixture');
const require=createRequire(import.meta.url),ts=require(process.env.TYPESCRIPT_PATH??'/private/tmp/fl-managed-payments-context-dependencies/node_modules/typescript');
const work=await mkdtemp(path.join(tmpdir(),'fl-managed-creation-wire-'));
const emit=text=>ts.transpileModule(text,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText;
for(const [name,source] of [['context','src/modules/managed-apps/domain/services/payments-context.ts'],['creation','src/modules/managed-apps/domain/services/payment-link-creation.ts']])await writeFile(path.join(work,name+'.mjs'),emit(await readFile(path.join(platform,source),'utf8')));
await writeFile(path.join(work,'safe.mjs'),emit(await readFile(path.join(root,'framework/common/src/safeStringify.ts'),'utf8')));
await writeFile(path.join(work,'hmac.mjs'),emit((await readFile(path.join(root,'framework/core/src/http/createHmacToken.ts'),'utf8')).replace("'@forklaunch/common'","'./safe.mjs'")));
await writeFile(path.join(work,'verify.mjs'),emit((await readFile(path.join(platform,'src/modules/managed-apps/domain/utils/instance-hmac.util.ts'),'utf8')).replace("'@forklaunch/core/http'","'./hmac.mjs'")));
const {PaymentsContextProtocol}=await import(pathToFileURL(path.join(work,'context.mjs')));
const {PaymentLinkCreationProtocol,PaymentCreationError}=await import(pathToFileURL(path.join(work,'creation.mjs')));
const {parseHmacAuthorization,verifyInstanceHmac}=await import(pathToFileURL(path.join(work,'verify.mjs')));
let scenario='ok',generation='00000000-0000-4000-8000-000000000001',configuration='a'.repeat(64),posts=0,creates=0,inspects=0,lastIssuedAt=0;
let resume,entered=false;const rows=new Map(),nonces=new Set();
const owner={id:'instance-owned',organizationId:'org-owned'};
const journal={
 async inspect(o,id){const row=rows.get(id);return row&&row.envelope.instanceId===o.id&&row.envelope.organizationId===o.organizationId?row.result:null;},
 async reserve(envelope){const old=rows.get(envelope.operationId);if(old){assert.deepEqual(old.envelope,envelope);return old.result;}const ticket={idempotencyKey:'fl-managed-plink-'+crypto.randomUUID()};rows.set(envelope.operationId,{envelope,ticket,result:{state:'unresolved',operationId:envelope.operationId,envelope}});return {ticket};},
 async complete(ticket,provider,fresh){fresh();const row=[...rows.values()].find(r=>r.ticket===ticket);assert(row);row.result={state:'completed',operationId:row.envelope.operationId,envelope:row.envelope,provider};fresh();}
};
const forward=async request=>{
 assert.equal(request.stripeAccount,'acct_owned');assert.equal(request.stripeVersion,'2026-08-26.dahlia');
 if(request.method==='GET')return {status:200,body:JSON.stringify(request.path.includes('/accounts/')?{id:'acct_owned',object:'account'}:{object:'balance',livemode:false})};
 assert.equal(request.method,'POST');assert.equal(request.path,'/v1/payment_links');assert(request.idempotencyKey.startsWith('fl-managed-plink-'));
 assert.equal(request.body,'currency=USD&payment_method_types%5B%5D=card&line_items%5B0%5D%5Bprice%5D=price_owned&line_items%5B0%5D%5Bquantity%5D=1');
 posts++;if(scenario==='timeout')throw Error('owned lost provider response');
 if(scenario==='pause'){entered=true;await new Promise(r=>{resume=r;});}
 return {status:200,body:JSON.stringify({id:'plink_managed_'+posts,object:'payment_link',livemode:false,url:'https://owned.test/private'})};
};
const context=new PaymentsContextProtocol('owned-stripe-key','https://owned.stripe.test',forward);
const read=async()=>({instanceId:owner.id,organizationId:owner.organizationId,stripeAccountId:'acct_owned',bindingGeneration:generation});
const protocol=()=>new PaymentLinkCreationProtocol({id:'managed-owned',configurationId:configuration,accountId:'acct_owned',livemode:false,apiVersion:'2026-08-26.dahlia',trustedPrices:['price_owned']},context,forward,journal);
const server=http.createServer(async(req,res)=>{
 try{
  let raw='';for await(const chunk of req)raw+=chunk;assert(Buffer.byteLength(raw)<8192);
  if(req.url==='/control'){
   const value=JSON.parse(raw);if(value.scenario!==undefined)scenario=value.scenario;if(value.configuration)configuration=value.configuration;if(value.generation)generation=value.generation;if(value.resume){resume?.();resume=undefined;entered=false;}
   res.setHeader('content-type','application/json');res.end(JSON.stringify({posts,creates,inspects,entered,generation,configuration,lastIssuedAt}));return;
  }
  assert.equal(req.method,'POST');const auth=parseHmacAuthorization(req.headers.authorization);assert(auth);assert.equal(auth.keyId,owner.id);assert(!nonces.has(auth.nonce));nonces.add(auth.nonce);
  assert(verifyInstanceHmac({secretKey:'owned-instance-key',method:req.method,path:req.url.replace('/instance-gateway',''),body:raw,auth}));
  const body=JSON.parse(raw);let answer;
  switch(req.url){
   case '/instance-gateway/payments/operations/payment-link-context': answer=await protocol().creationContext(owner,body,read,undefined);lastIssuedAt=answer.scope.issuedAt;break;
   case '/instance-gateway/payments/operations/payment-link-create':creates++;answer=await protocol().create(owner,body,read,undefined);if(scenario==='lost-delivery'){res.statusCode=503;answer={};}if(scenario==='rotate-config')configuration='b'.repeat(64);if(scenario==='rotate-generation')generation=crypto.randomUUID();break;
   case '/instance-gateway/payments/operations/payment-link-inspect':inspects++;answer=await protocol().inspect(owner,body.operationId,read,undefined);break;
   default:throw Error('No raw provider routes allowed');
  }
  if(scenario==='bad-envelope'&&answer?.state==='completed')answer={...answer,envelope:{...answer.envelope,publicId:crypto.randomUUID()}};
  if(scenario==='bad-provider'&&answer?.state==='completed')answer={...answer,provider:{...answer.provider,object:'price'}};
  if(scenario==='bad-current-config'&&answer?.currentConfigurationId)answer={...answer,currentConfigurationId:'f'.repeat(64)};
  res.setHeader('content-type','application/json');res.end(JSON.stringify(answer));
 }catch(error){res.statusCode=error instanceof PaymentCreationError?409:500;res.end('{}');}
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
try{
 const base=`http://127.0.0.1:${server.address().port}`;
 await new Promise((resolve,reject)=>{
  const child=spawn('cargo',['test','--manifest-path','blueprint-rs/Cargo.toml','-p','forklaunch-billing-stripe','--features','postgres-identity','--test','managed_creation','--','--ignored','--nocapture'],{cwd:root,stdio:'inherit',env:{...process.env,FORKLAUNCH_MANAGED_CREATION_BASE:base}});
  child.on('error',reject);child.on('close',code=>code?reject(Error(`native managed creation exited ${code}`)):resolve());
 });
 console.log('PASS actual companion protocol/HMAC/canonical bytes with native restricted-PostgreSQL managed creation and inspect-only recovery; no raw Stripe forwarding or live provider calls');
}finally{resume?.();await new Promise(r=>server.close(r));await rm(work,{recursive:true,force:true});}
