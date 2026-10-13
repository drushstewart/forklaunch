import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { createHmac, randomUUID } from 'node:crypto';
import { probeMcp, mcpRpc } from './rust_mcp_probe.mjs';
const database = new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
database.username = process.env.FORKLAUNCH_TEST_ROLE;
database.password = 'parity-test-only';
const secret = 'relay-generated-internal-secret';
const instanceSecret = 'relay-generated-instance-secret';
const child = spawn(process.argv[2], [], {env:{...process.env,
 DATABASE_URL:database.toString(),TENANT_ID:`${database.username}-a`,
 ENCRYPTION_KEY:'generated-auth-encryption-fixture-only', HMAC_SECRET_KEY:secret,
 INSTANCE_ID:'fixture-instance',INSTANCE_HMAC_KEY:instanceSecret,
 BETTER_AUTH_URL:'http://localhost:8000', BETTER_AUTH_SECRET:'relay-generated-cookie-secret-32-characters',
 HOST:'127.0.0.1',SERVICE_PORT:'0',NODE_ENV:'development'
},stdio:['ignore','pipe','pipe']});
let stdout='',stderr='';child.stdout.on('data',data=>stdout+=data);child.stderr.on('data',data=>stderr+=data);
const exited=new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);});
exited.catch(()=>{});
function auth(path,body,key='fixture-instance',signingSecret=instanceSecret) {
 const ts=new Date().toISOString(),nonce=randomUUID();
 const signature=createHmac('sha256',signingSecret).update(`POST\n${path}\n${body}\n${ts}\n${nonce}`).digest('base64');
 return `HMAC keyId=${key} ts=${ts} nonce=${nonce} signature=${signature}`;
}
try {
 let base;
 for(let i=0;i<600;i++) {base=stdout.match(/listening on (http:\/\/[^\s]+)/)?.[1];if(base)break;
  assert.equal(child.exitCode,null,stderr);await new Promise(resolve=>setTimeout(resolve,100));}
 assert.ok(base,stderr);
 const tools=await probeMcp(base);
 assert.equal(tools.length,8);
 const ingest=tools.find(tool=>tool.name.toLowerCase().includes('relaysessioningest'));
 const claim=tools.find(tool=>tool.name.toLowerCase().includes('mintclaimtoken'));
 assert.ok(ingest,JSON.stringify(tools));assert.ok(claim);
 const path='/relay/session-ingest',body=JSON.stringify({nonce:randomUUID(),tokens:{access_token:'fixture-token'}});
 async function post(route,raw,key,signingSecret) {return fetch(base+route,{method:'POST',headers:{'content-type':'application/json',authorization:auth(route,raw,key,signingSecret)},body:raw});}
 assert.equal((await post(path,body,'default',secret)).status,401);
 const accepted=await post(path,body);assert.equal(accepted.status,200,await accepted.clone().text());
 const {redirectPath}=await accepted.json();assert.match(redirectPath,/^\/relay\/handoff\?ticket=/);
 assert.equal((await post(path,body)).status,409);
 for(let i=0;i<2;i++) {const redirect=await fetch(base+redirectPath,{redirect:'manual'});assert.equal(redirect.status,302);assert.equal(redirect.headers.get('location'),'/');assert.equal(redirect.headers.get('set-cookie'),null);}
 assert.equal((await post('/claim/mint','{}','platform',instanceSecret)).status,409);
 assert.equal((await post('/claim/mint','{"purpose":"wrong"}','default',secret)).status,400);
 const args={nonce:randomUUID(),tokens:{}};
 const invoked=await mcpRpc(base,'tools/call',{name:ingest.name,arguments:{body:args,headers:{authorization:auth(path,JSON.stringify(args))}}});
 const {createForkLaunchSdk}=await import(pathToFileURL(process.argv[3]));
 const sdk=createForkLaunchSdk({baseUrl:base});
 const sdkBody={nonce:randomUUID(),tokens:{}};
 const sdkResult=await sdk.iam.relay.relaySessionIngest({body:sdkBody,headers:{authorization:auth(path,JSON.stringify(sdkBody))}});
 assert.equal(sdkResult.status,200,JSON.stringify(sdkResult));
 assert.match(sdkResult.response.redirectPath,/^\/relay\/handoff\?ticket=/);
 const declined=await sdk.iam.claim.mintClaimToken({body:{},headers:{authorization:auth('/claim/mint','{}','platform',instanceSecret)}});
 assert.equal(declined.status,409,JSON.stringify(declined));
 assert.equal(typeof declined.response,'string');
 assert.equal(invoked.isError,false,JSON.stringify(invoked));
 assert.match(invoked.structuredContent.redirectPath,/^\/relay\/handoff\?ticket=/);
 console.log('PASS generated relay: HTTP/MCP startup, route-scoped HMAC, ingest, nonce replay, handoff, default hooks, claim purpose and MCP invocation');
} finally {
 child.kill('SIGTERM');let timeout;
 try {assert.equal(await Promise.race([exited,new Promise((_,reject)=>{timeout=setTimeout(()=>{child.kill('SIGKILL');reject(Error('shutdown timed out'));},10000);})]),0,stderr);}
 finally {clearTimeout(timeout);}
}
