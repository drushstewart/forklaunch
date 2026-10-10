import assert from 'node:assert/strict';
import { generateKeyPairSync,sign,createHmac,randomUUID } from 'node:crypto';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
const [baseBinary,twilioBinary,sdkPath]=process.argv.slice(2);
const role=process.env.FORKLAUNCH_TEST_ROLE;
const secret='messaging-fixture-hmac-secret',token='twilio-fixture-token';
const callbackUrl='https://sms.example.com/webhook?source=fixture';
const issuer='https://issuer.example',audience='sms-fixture';
const {privateKey,publicKey}=generateKeyPairSync('rsa',{modulusLength:2048});
const jwk={...publicKey.export({format:'jwk'}),kid:'sms-fixture',alg:'RS256',use:'sig'};
let jwksFetches=0,dispatches=0;
const jwks=http.createServer((_req,res)=>{jwksFetches++;res.setHeader('content-type','application/json');res.end(JSON.stringify({keys:[jwk]}));});
const provider=http.createServer(async(req,res)=>{
  dispatches++;
  assert.equal(req.headers.authorization,`Basic ${Buffer.from(`ACfixture:${token}`).toString('base64')}`);
  assert.equal(req.url,'/Accounts/ACfixture/Messages.json');
  let body='';for await(const chunk of req)body+=chunk;
  const params=new URLSearchParams(body);assert.equal(params.get('StatusCallback'),callbackUrl);
  if(params.get('Body')==='provider-malformed'){res.end('not-json');return;}
  if(params.get('Body')==='provider-large'){res.end('x'.repeat(65537));return;}
  if(params.get('Body')==='provider-redirect'){res.writeHead(302,{location:'/must-not-follow'});res.end('redirect');return;}
  if(params.get('Body')==='provider-error'){res.writeHead(400);res.end('fixture rejection');return;}
  res.setHeader('content-type','application/json');res.end(JSON.stringify({sid:`SM-fixture-${dispatches}`,status:'queued',error_message:null}));
});
await Promise.all([jwks,provider].map(server=>new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))));
const children=[];
function hmac(path,method='GET',body,ts=new Date().toISOString()){
  const nonce=randomUUID();const serialized=body===undefined?'undefined':`${body}\n`;
  const signature=createHmac('sha256',secret).update(`${method}\n${path}\n${serialized}${ts}\n${nonce}`).digest('base64');
  return `HMAC keyId=default ts=${ts} nonce=${nonce} signature=${signature}`;
}
function jwt(roles=['system'],overrides={}){
  const header=Buffer.from(JSON.stringify({alg:'RS256',kid:jwk.kid,typ:'JWT'})).toString('base64url');
  const payload=Buffer.from(JSON.stringify({sub:'sms-system-user',exp:Math.floor(Date.now()/1000)+300,roles,iss:issuer,aud:audience,...overrides})).toString('base64url');
  return `Bearer ${header}.${payload}.${sign('RSA-SHA256',Buffer.from(`${header}.${payload}`),privateKey).toString('base64url')}`;
}
function callbackSignature(params,url=callbackUrl){
  const fields=new Map();for(const [key,value]of params)fields.set(key,[...(fields.get(key)??[]),value]);
  const canonical=[...fields.keys()].sort().reduce((s,k)=>s+[...new Set(fields.get(k))].sort().map(v=>k+v).join(''),url);
  return createHmac('sha1',token).update(canonical).digest('base64');
}
async function start(binary,tenant='a'){
  const db=new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);db.username=role;db.password='parity-test-only';
  const env={...Object.fromEntries(Object.entries(process.env).filter(([k])=>!k.startsWith('OTEL_'))),DATABASE_URL:db.toString(),TENANT_ID:`${role}-${tenant}`,ENCRYPTION_KEY:'sms-fixture-encryption-key-only',HOST:'127.0.0.1',SERVICE_PORT:'0',HMAC_SECRET_KEY:secret,JWKS_PUBLIC_KEY_URL:`http://127.0.0.1:${jwks.address().port}/jwks`,JWT_ISSUER:issuer,JWT_AUDIENCE:audience,TWILIO_ACCOUNT_SID:'ACfixture',TWILIO_AUTH_TOKEN:token,TWILIO_FROM_NUMBER:'+15550000000',TWILIO_STATUS_CALLBACK_URL:callbackUrl,TWILIO_API_BASE:`http://127.0.0.1:${provider.address().port}`};
  const child=spawn(binary,[],{env,stdio:['ignore','pipe','pipe']});let stdout='',stderr='';
  child.stdout.on('data',d=>stdout+=d);child.stderr.on('data',d=>stderr+=d);
  const exited=new Promise(resolve=>child.once('close',resolve));const info={child,exited,stderr:()=>stderr};children.push(info);
  for(let i=0;i<300;i++){
    const base=stdout.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];if(base)return{...info,base};
    assert.equal(child.exitCode,null,stderr);await new Promise(resolve=>setTimeout(resolve,50));
  }throw new Error(`Messaging startup timeout: ${stderr}`);
}
async function stop(instance){instance.child.kill('SIGTERM');const timer=setTimeout(()=>instance.child.kill('SIGKILL'),15000);try{assert.equal(await instance.exited,0,instance.stderr());}finally{clearTimeout(timer);}}
async function call(base,path,{method='GET',body,authorization=hmac(path,method,body),headers={}}={}){
  return fetch(base+path,{method,...(body===undefined?{}:{body}),headers:{authorization,...(body===undefined?{}:{'content-type':'application/json'}),...headers}});
}
try{
  let base=await start(baseBinary);const twilio=await start(twilioBinary);const other=await start(twilioBinary,'b');
  const sdk=await import(pathToFileURL(sdkPath));
  const client=sdk.createForkLaunchSdk({serviceBaseUrls:{messaging:base.base,twilio:twilio.base}});
  assert.equal((await fetch(`${base.base}/health`)).status,200);
  assert.equal((await fetch(`${base.base}/sms/missing`)).status,401);
  const request={to:'+15550000001',body:'private SMS 💬',metadata:{nested:[1,true,null]}};
  const serialized=JSON.stringify(request);
  for(const authorization of [hmac('/wrong','POST',serialized),hmac('/sms','POST','tampered'),hmac('/sms','POST',serialized,'2020-01-01T00:00:00.000Z')]){
    const response=await call(base.base,'/sms',{method:'POST',body:serialized,authorization});assert.equal(response.status,401);await response.arrayBuffer();
  }
  const sent=await client.messaging.sms.sendSms({body:request,headers:{authorization:hmac('/sms','POST',serialized)}});
  assert.equal(sent.code,200);assert.equal(sent.response.status,'sent');assert.deepEqual(sent.response.metadata,request.metadata);
  const id=sent.response.id;
  assert.equal((await call(other.base,`/sms/${id}`)).status,404);
  const queued=await client.twilio.sms.sendSms({body:request,headers:{authorization:hmac('/sms','POST',serialized)}});
  assert.equal(queued.code,200);assert.equal(queued.response.status,'queued');assert.equal(dispatches,1);
  assert.ok(queued.response.providerMessageId);
  const callback=new URLSearchParams({MessageSid:queued.response.providerMessageId,MessageStatus:'failed',ErrorMessage:'delivery failed',AccountSid:'ACfixture',Extra:'💬'});
  async function sendCallback(params,signature=callbackSignature(params),extra={}){return fetch(twilio.base+'/webhook',{method:'POST',body:params,headers:{'x-twilio-signature':signature,...extra}});}
  for(const signature of ['',callbackSignature(callback,'https://attacker.example/webhook'),'invalid']){
    const response=await sendCallback(callback,signature);assert.equal(response.status,401);await response.arrayBuffer();
  }
  let response=await sendCallback(callback,callbackSignature(callback),{'x-forwarded-host':'attacker.example'});assert.equal(response.status,200);assert.equal(await response.text(),'Status callback processed');
  let fetched=await client.twilio.sms.getSmsRecord({params:{id:queued.response.id},headers:{authorization:hmac(`/sms/${queued.response.id}`)}});assert.equal(fetched.response.status,'failed');assert.equal(fetched.response.error,'delivery failed');
  callback.set('MessageStatus','read');callback.delete('ErrorMessage');response=await sendCallback(callback);assert.equal(response.status,200);await response.arrayBuffer();
  fetched=await client.twilio.sms.getSmsRecord({params:{id:queued.response.id},headers:{authorization:hmac(`/sms/${queued.response.id}`)}});assert.equal(fetched.response.status,'delivered');assert.ok(!('error'in fetched.response));
  response=await sendCallback(callback);assert.equal(response.status,200);await response.arrayBuffer();
  const duplicate=new URLSearchParams(callback);duplicate.append('MessageStatus','sent');response=await sendCallback(duplicate);assert.equal(response.status,401);await response.arrayBuffer();
  const tampered=new URLSearchParams(callback);tampered.set('Extra','tampered');response=await sendCallback(tampered,callbackSignature(callback));assert.equal(response.status,401);await response.arrayBuffer();
  const unknown=new URLSearchParams(callback);unknown.set('MessageSid','unknown');response=await sendCallback(unknown);assert.equal(response.status,404);await response.arrayBuffer();
  const sdkCallback={MessageSid:queued.response.providerMessageId,MessageStatus:'delivered',Extra:'💬 + = &',Repeated:['z','a','a']};
  const sdkForm=new URLSearchParams();for(const [key,values]of Object.entries(sdkCallback))for(const value of Array.isArray(values)?values:[values])sdkForm.append(key,value);
  const sdkHandled=await client.twilio.webhook.handleStatusCallback({body:sdkCallback,headers:{'x-twilio-signature':callbackSignature(sdkForm)}});
  assert.equal(sdkHandled.code,200);assert.equal(sdkHandled.response,'Status callback processed');
  const sdkRaw=await client.twilio.webhook.handleStatusCallback({body:sdkForm,headers:{'x-twilio-signature':callbackSignature(sdkForm)}});assert.equal(sdkRaw.code,200);
  await assert.rejects(()=>client.twilio.webhook.handleStatusCallback({body:{MessageSid:'sid',MessageStatus:'sent',Nested:{invalid:true}},headers:{}}),/Form values/);
  const rustBody=JSON.stringify({to:'+15550000008',body:'private SMS from Rust SDK'});
  const rust=spawn('cargo',['test','-p','twilio-sdk','--test','runtime','--','--nocapture'],{env:{...process.env,SMS_BASE_URL:twilio.base,SMS_RECORD_ID:queued.response.id,SMS_GET_HEADER:hmac(`/sms/${queued.response.id}`),SMS_MISSING_HEADER:hmac('/sms/missing'),SMS_CALLBACK_FORM:sdkForm.toString(),SMS_CALLBACK_SIGNATURE:callbackSignature(sdkForm),SMS_SEND_BODY:rustBody,SMS_SEND_HEADER:hmac('/sms','POST',rustBody)},stdio:['ignore','pipe','pipe']});
  let rustOutput='';rust.stdout.on('data',d=>rustOutput+=d);rust.stderr.on('data',d=>rustOutput+=d);
  const rustTimer=setTimeout(()=>rust.kill('SIGKILL'),120000);const rustCode=await new Promise(resolve=>rust.once('close',resolve));clearTimeout(rustTimer);assert.equal(rustCode,0,rustOutput);
  const failedBody=JSON.stringify({...request,body:'provider-error'});response=await call(twilio.base,'/sms',{method:'POST',body:failedBody});assert.equal(response.status,200);const failed=await response.json();assert.equal(failed.status,'failed');assert.equal(failed.error,'Twilio API error (400): fixture rejection');
  const previousDispatches=dispatches;
  for(const body of ['provider-malformed','provider-large']){const payload=JSON.stringify({...request,to:'+15550000007',body});response=await call(twilio.base,'/sms',{method:'POST',body:payload});assert.equal(response.status,500);await response.arrayBuffer();}
  const redirectPayload=JSON.stringify({...request,to:'+15550000007',body:'provider-redirect'});response=await call(twilio.base,'/sms',{method:'POST',body:redirectPayload});assert.equal(response.status,200);assert.equal((await response.json()).status,'failed');
  assert.equal(dispatches,previousDispatches+3,'provider requests must never retry or follow redirects');
  for(const roles of [['admin'],[]]){response=await call(base.base,'/compliance/export/'+encodeURIComponent(request.to),{authorization:jwt(roles)});assert.equal(response.status,403);await response.arrayBuffer();}
  for(const overrides of [{iss:'attacker'},{aud:'wrong'},{exp:1},{nbf:Math.floor(Date.now()/1000)+600}]){response=await call(base.base,'/compliance/export/'+encodeURIComponent(request.to),{authorization:jwt(['system'],overrides)});assert.equal(response.status,401);await response.arrayBuffer();}
  const exported=await client.messaging.compliance.exportUserData({params:{userId:request.to},headers:{authorization:jwt()}});assert.equal(exported.code,200);assert.equal(exported.response.entities.SmsRecord.length,3);assert.deepEqual(exported.response.failures,[]);
  assert.deepEqual(Object.keys(exported.response.entities.SmsRecord[0]).sort(),['body','id','to']);
  response=await call(other.base,'/compliance/export/'+encodeURIComponent(request.to),{authorization:jwt()});assert.equal(response.status,404);await response.arrayBuffer();
  await stop(base);base=await start(baseBinary);
  response=await call(base.base,`/sms/${id}`);assert.equal(response.status,200);assert.equal((await response.json()).body,request.body);
  const erasure=await call(base.base,'/compliance/erase/'+encodeURIComponent(request.to),{method:'DELETE',authorization:jwt()});assert.equal(erasure.status,200);assert.deepEqual(await erasure.json(),{entitiesAffected:['SmsRecord'],recordsDeleted:3,failures:[]});
  response=await call(base.base,`/sms/${id}`);assert.equal(response.status,404);await response.arrayBuffer();
  response=await call(base.base,'/compliance/erase/'+encodeURIComponent(request.to),{method:'DELETE',authorization:jwt()});assert.equal(response.status,404);await response.arrayBuffer();
  // Leave a persisted record for the owner-side ciphertext check.
  const last=JSON.stringify({...request,to:'+15550000009'});response=await call(base.base,'/sms',{method:'POST',body:last});assert.equal(response.status,200);await response.arrayBuffer();
  for(const instance of [base,twilio,other])await stop(instance);
  for(const info of children){assert.ok(!info.stderr().includes(secret));assert.ok(!info.stderr().includes(token));assert.ok(!info.stderr().includes(request.body));}
  assert.ok(jwksFetches>=2&&jwksFetches<=4);
  console.log('PASS generated base/Twilio SMS persistence, TypeScript/Rust SDKs, HMAC body integrity, signed callbacks, tenant isolation, JWKS/RBAC compliance, restart recovery, audit-safe logging and SIGTERM');
}finally{
  for(const {child} of children)if(child.exitCode===null&&child.signalCode===null)child.kill('SIGKILL');
  await Promise.all([jwks,provider].map(server=>new Promise(resolve=>server.close(resolve))));
}
