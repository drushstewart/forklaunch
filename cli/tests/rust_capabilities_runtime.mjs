import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';
import {randomUUID} from 'node:crypto';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {actualHttpSources} from './rust_actual_http_fixture.mjs';
import {probeMcp} from './rust_mcp_probe.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const {load}=actualHttpSources(root);
const {createHmacToken}=load(path.join(root,'framework/core/src/http/createHmacToken.ts'));
const reserve=createServer();reserve.listen(0,'127.0.0.1');await once(reserve,'listening');
const port=reserve.address().port;await new Promise(resolve=>reserve.close(resolve));
const key='native-capabilities-fixture-key',base=`http://127.0.0.1:${port}`;
const service=spawn(process.argv[2],[],{env:{...process.env,HOST:'127.0.0.1',PORT:String(port),RUST_LOG:'info',MCP_ENABLED:'true',PROTOCOL:'http',VERSION:'v1',DOCS_PATH:'/docs',INSTANCE_HMAC_KEY:key,OTEL_EXPORTER_OTLP_ENDPOINT:'',MCP_PATH:'/mcp'},stdio:['ignore','pipe','pipe']});
let output='';for(const stream of [service.stdout,service.stderr])stream.on('data',chunk=>output+=chunk);
function signed(feature,body,options={}){
 const route=`/platform-events/${feature}`,timestamp=new Date(options.timestamp??Date.now()),nonce=options.nonce??randomUUID();
 const signature=createHmacToken({secretKey:options.key??key,method:'POST',path:route,body,timestamp,nonce});
 return `HMAC keyId=platform ts=${timestamp.toISOString()} nonce=${nonce} signature=${signature}`;
}
async function post(feature,body,authorization){return fetch(`${base}/platform-events/${feature}`,{method:'POST',headers:{'content-type':'application/json',...(authorization?{authorization}:{})},body,signal:AbortSignal.timeout(5000)});}
try{
 for(let retry=0;;retry++){
  try{if((await fetch(`${base}/health`)).ok)break;}catch{}
  if(retry>=100||service.exitCode!==null)throw new Error(`Service failed to start: ${output}`);
  await new Promise(resolve=>setTimeout(resolve,100));
 }
 const tools=await probeMcp(base);
 assert.ok(!tools.some(tool=>tool.name.toLowerCase().includes('receiveplatformevent')),'Internal signed callbacks must be omitted from MCP tools');
 if(process.argv.includes('--bounded-store')){
  for(const [feature,status] of [['email',200],['sms',200],['voice',503]]){
   const body=JSON.stringify({id:`bounded_${feature}`,feature,type:`${feature}.fixture`});
   assert.equal((await post(feature,body,signed(feature,body))).status,status,'The preserved nonce factory must provide one bounded store across all features');
  }
  console.log('PASS customized nonce-store factory survives sync/rename and supplies shared bounded callback storage');
 }else{
 for(const feature of ['email','sms','whatsapp','voice','payments']){
  const body=JSON.stringify({id:`event_${feature}`,feature,type:`${feature}.delivered`,data:{note:'Exact Unicode 💬 bytes'}});
  assert.equal((await post(feature,body)).status,401);
  assert.equal((await post(feature,body,signed(feature,body,{key:'wrong'}))).status,401);
  assert.equal((await post(feature,body,signed(feature,body,{timestamp:Date.now()-600000}))).status,401);
  const authorization=signed(feature,body);
  assert.equal((await post(feature,body,authorization)).status,200);
  assert.equal((await post(feature,body,authorization)).status,401);
  assert.equal((await post(feature,body,signed(feature,body))).status,200,'Event-ID deduplication belongs to the business handler');
 }
 const oversized=JSON.stringify({id:'oversized',feature:'email',type:'email.test',data:'x'.repeat(1024*1024)});
 assert.ok([401,413].includes((await post('email',oversized,signed('email',oversized))).status),'Signed bodies remain bounded');
 const secured=await fetch(`${base}/health`);
 assert.equal(secured.headers.get('x-content-type-options'),'nosniff','Native callback integration retains shared HTTP security middleware');
 assert.equal((await post('unknown','{}')).status,404);
 const encodedBody=JSON.stringify({id:'encoded-email',feature:'email',type:'email.delivered'});
 assert.equal((await post('%65mail',encodedBody,signed('%65mail',encodedBody))).status,200,'Dispatch must use the decoded route parameter while signatures bind the actual URL');
 assert.equal((await post('%65mail',encodedBody,signed('email',encodedBody))).status,401,'Encoded URLs must not bypass exact-path signature verification');
 const malformed='{invalid';assert.equal((await post('email',malformed,signed('email',malformed))).status,401);
 console.log('PASS all five generated native capability callbacks: actual TypeScript signatures, HTTP/MCP startup, forgery/staleness/replay rejection, event redelivery and unknown feature');
 }
}finally{
 if(service.exitCode===null){const exit=once(service,'exit');service.kill('SIGTERM');await Promise.race([exit,new Promise((_,reject)=>setTimeout(()=>{service.kill('SIGKILL');reject(new Error(`Shutdown timed out: ${output}`));},10000).unref())]);}
}
