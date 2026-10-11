import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
import assert from 'node:assert/strict';
import { spawn,execFile } from 'node:child_process';
import {promisify} from 'node:util';
import { once } from 'node:events';
import {readFileSync} from 'node:fs';
import {generateKeyPairSync,sign,createHmac,randomUUID} from 'node:crypto';
import http from 'node:http';
import { createForkLaunchSdk } from './sdks/typescript/dist/index.js';

const directory=JSON.parse(readFileSync('iam-directory-fixture.json','utf8'));
const secret='iam-fixture-hmac',issuer='https://iam-fixture.example',audience='iam-fixture';
const {privateKey,publicKey}=generateKeyPairSync('rsa',{modulusLength:2048});
const jwk={...publicKey.export({format:'jwk'}),kid:'iam-fixture',alg:'RS256',use:'sig'};
const jwks=http.createServer((_request,response)=>{response.setHeader('content-type','application/json');response.end(JSON.stringify({keys:[jwk]}));});
await new Promise(resolve=>jwks.listen(0,'127.0.0.1',resolve));
function jwt(subject=directory.a.users[0].id,roles=['admin','platform:read'],permissions=['platform:read'],extra={}) {
 const header=Buffer.from(JSON.stringify({alg:'RS256',kid:jwk.kid,typ:'JWT'})).toString('base64url');
 const payload=Buffer.from(JSON.stringify({sub:subject,roles,permissions,iss:issuer,aud:audience,exp:Math.floor(Date.now()/1000)+300,...extra})).toString('base64url');
 return `Bearer ${header}.${payload}.${sign('RSA-SHA256',Buffer.from(`${header}.${payload}`),privateKey).toString('base64url')}`;
}
function hmac(path,method='GET',body) {const ts=new Date().toISOString(),nonce=randomUUID();return `HMAC keyId=default ts=${ts} nonce=${nonce} signature=${createHmac('sha256',secret).update(`${method}\n${path}\n${body===undefined?'undefined':body+'\n'}${ts}\n${nonce}`).digest('base64')}`;}
async function call(base,path,method='GET',body,status=200,auth) {
 const raw=body===undefined?undefined:JSON.stringify(body);
 const response=await fetch(base+path,{method,body:raw,headers:{authorization:auth??hmac(path.split('?')[0],method,raw),...(raw?{'content-type':'application/json'}:{})},signal:AbortSignal.timeout(10000)});
 assert.equal(response.status,status,await response.clone().text());return response.headers.get('content-type')?.includes('application/json')?response.json():response.text();
}
const processes = [];
const role = process.env.FORKLAUNCH_TEST_ROLE;
const databaseUrl = new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
databaseUrl.username = role;
databaseUrl.password = 'parity-test-only';
async function start(tenant) {
  const child = spawn(process.argv[2], [], {
    env: { ...process.env, DATABASE_URL: databaseUrl.toString(), TENANT_ID: `${role}-${tenant}`, HOST: '127.0.0.1', SERVICE_PORT: '0', ENCRYPTION_KEY:'iam-fixture-encryption-only',HMAC_SECRET_KEY:secret,JWKS_PUBLIC_KEY_URL:`http://127.0.0.1:${jwks.address().port}/jwks`,JWT_ISSUER:issuer,JWT_AUDIENCE:audience },
    stdio: ['ignore', 'pipe', 'pipe']
  });
  const exited = once(child, 'exit');
  processes.push({ child, exited });
  let stderr = '';
  child.stderr.on('data', chunk => stderr += chunk);
  const baseUrl = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`IAM startup timed out: ${stderr}`)), 15000);
    let output = '';
    child.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/listening on (http:\/\/[^\s]+)/);
      if (match) { clearTimeout(timeout); resolve(match[1]); }
    });
    child.once('error', error => { clearTimeout(timeout); reject(error); });
    child.once('exit', code => { clearTimeout(timeout); reject(new Error(`IAM exited ${code}: ${stderr}`)); });
  });
  return { baseUrl, child, exited };
}

try {
  const first = await start('a');
  const second = await start('b');
  const tools=await probeMcp(first.baseUrl,['listUsers','getSessionUser','createUser','getUser','getBatchPermissions','surfaceUserRoles']);
  assert.equal(tools.length,40);
  await probeMcp(second.baseUrl,['listUsers']);
  const publicSdk = createForkLaunchSdk({ baseUrl: first.baseUrl });
  const discovery=await publicSdk.iam.discovery.getauthmethods();assert.equal(discovery.code,204);assert.equal(discovery.response,undefined);
  const mcpDiscovery=await mcpRpc(first.baseUrl,'tools/call',{name:'getauthmethods',arguments:{}});assert.equal(mcpDiscovery.isError,false,JSON.stringify(mcpDiscovery));
  for(const [path,method,operation]of [['export','GET','exportuserdata'],['erase','DELETE','eraseuserdata']]) {
   await call(first.baseUrl,`/compliance/${path}/fixture-user`,method,undefined,403,jwt());
   await call(first.baseUrl,`/compliance/${path}/fixture-user`,method,undefined,404,jwt(directory.a.users[0].id,['system']));
   const result=await mcpRpc(first.baseUrl,'tools/call',{name:operation,arguments:{params:{userId:'fixture-user'}}},{authorization:jwt(directory.a.users[0].id,['system'])});assert.equal(result.isError,true);
  }
  console.log('PASS configured IAM discovery and compliance over HTTP/MCP: public 204, system-role authorization and no classified export/erase data');

  const login = await publicSdk.iam.auth.authenticate({ body: { email: 'member@example.com', password: 'first-password' } });
  assert.equal(login.code, 200);
  const token = login.response.id;
  const sdk = createForkLaunchSdk({ baseUrl: first.baseUrl, headers: { authorization: `Bearer ${token}` } });
  const page = await sdk.iam.users.listUsers({ query: { limit: 1 } });
  assert.equal(page.code, 200);
  assert.equal(page.response.items[0].display_name, 'First');
  assert.equal(page.response.next_cursor, 'user-1');
  const mcpPage=await mcpRpc(first.baseUrl,'tools/call',{name:'listUsers',arguments:{query:{limit:1}}},{authorization:`Bearer ${token}`});assert.equal(mcpPage.isError,false,JSON.stringify(mcpPage));assert.deepEqual(JSON.parse(mcpPage.content[0].text),page.response);
  const deniedMcp=await mcpRpc(first.baseUrl,'tools/call',{name:'listUsers',arguments:{query:{}}});assert.equal(deniedMcp.isError,true);
  const wrongTenantMcp=await mcpRpc(second.baseUrl,'tools/call',{name:'listUsers',arguments:{query:{}}},{authorization:`Bearer ${token}`});assert.equal(wrongTenantMcp.isError,true);
  const next = await sdk.iam.users.listUsers({ query: { limit: 1, cursor: page.response.next_cursor } });
  assert.equal(next.response.items[0].id, 'user-2');
  assert.equal(next.response.next_cursor, null);
  const wrongTenant = await createForkLaunchSdk({ baseUrl: second.baseUrl, headers: { authorization: `Bearer ${token}` } }).iam.users.listUsers({query:{}});
  assert.equal(wrongTenant.code, 401);
  assert.equal((await publicSdk.iam.users.listUsers({query:{}})).code, 401);
  assert.equal((await fetch(`${first.baseUrl}/health`)).status, 200);
  const fixture=directory.a,auth=jwt(),user=fixture.users[0],member=fixture.users[1],outsider=fixture.users[2];
  assert.equal((await call(first.baseUrl,`/role/${fixture.role.id}`,'GET',undefined,200,auth)).permissions[0].id,fixture.permission.id);
  await call(first.baseUrl,`/role/${fixture.role.id}`,'GET',undefined,403,jwt(user.id,[],[]));
  await call(first.baseUrl,`/role/${fixture.role.id}`,'GET',undefined,401,jwt(user.id,[],['platform:read'],{exp:1}));
  const directoryClient=createForkLaunchSdk({baseUrl:first.baseUrl,headers:{authorization:auth}});
  const typedUser=await directoryClient.iam.user.getUser({params:{id:member.id}});
  assert.equal(typedUser.code,200);assert.equal(typedUser.response.roles[0].permissions[0].id,fixture.permission.id);
  await call(first.baseUrl,`/user/${outsider.id}`,'GET',undefined,404,auth);
  await call(first.baseUrl,`/user/${member.id}`,'GET',undefined,401,jwt(randomUUID()));
  await call(second.baseUrl,`/user/${member.id}`,'GET',undefined,401,auth);
  const batchQuery=ids=>'?ids='+encodeURIComponent(JSON.stringify(ids));
  assert.equal((await call(first.baseUrl,'/user/batch'+batchQuery([member.id,outsider.id]),'GET',undefined,200,auth)).length,1);
  await call(first.baseUrl,'/user','PUT',{id:outsider.id,firstName:'Denied'},403,auth);
  await call(first.baseUrl,'/user/batch','PUT',[{id:member.id,firstName:'Must roll back'},{id:outsider.id,firstName:'Denied'}],403,auth);
  assert.equal((await call(first.baseUrl,`/user/${member.id}`,'GET',undefined,200,auth)).firstName,'Directory');
  await promisify(execFile)('cargo',['test','-p','iam-sdk','--test','runtime'],{env:{...process.env,IAM_BASE_URL:first.baseUrl,IAM_JWT_HEADER:auth,IAM_USER_ID:member.id,IAM_OUTSIDER_ID:outsider.id,IAM_ROLE_ID:fixture.role.id},timeout:300000,maxBuffer:4*1024*1024});
  console.log('PASS generated Rust SDK: typed nested IAM models, scoped batch reads, updates and typed authentication/authorization failures');
  await call(first.baseUrl,'/user','PUT',{id:member.id,firstName:'Updated'},200,auth);
  await call(first.baseUrl,'/user/batch','PUT',[{id:member.id,lastName:'Changed'}],200,auth);
  await call(first.baseUrl,'/user','POST',{email:'created@fixture.test',password:'unused',firstName:'Created',lastName:'Member',roles:[fixture.role.id]},201);
  await call(first.baseUrl,'/user/batch','POST',[{email:'batch-created@fixture.test',password:'unused',firstName:'Batch',lastName:'Member',roles:[]}],201);
  await call(first.baseUrl,`/user/${member.id}/surface-roles`);
  await call(first.baseUrl,`/user/${member.id}/surface-permissions`);
  for(const family of ['permission','role']) {
    const row=fixture[family],field=family==='permission'?'slug':'name';
    await call(first.baseUrl,`/${family}`,'POST',{[field]:'Created'},201);
    await call(first.baseUrl,`/${family}/batch`,'POST',[{[field]:'Batch created'}],201);
    await call(first.baseUrl,`/${family}/${row.id}`,'GET',undefined,200,auth);
    await call(first.baseUrl,`/${family}/batch`+batchQuery([row.id]),'GET',undefined,200,auth);
    await call(first.baseUrl,`/${family}`,'PUT',{id:row.id,[field]:'Updated'});
    await call(first.baseUrl,`/${family}/batch`,'PUT',[{id:row.id,[field]:'Batch updated'}]);
    await call(first.baseUrl,`/${family}/batch`+batchQuery([randomUUID()]),'DELETE');
  }
  const createdOrg=await call(first.baseUrl,'/organization','POST',{name:'Created organization',domain:'created.fixture.test',subscription:'created-org-sub'},201);
  await call(first.baseUrl,`/organization/${createdOrg.id}`,'GET',undefined,200,auth);
  await call(first.baseUrl,'/organization','PUT',{id:createdOrg.id,name:'Updated organization'});
  await call(first.baseUrl,`/organization/${createdOrg.id}`,'DELETE');
  await call(first.baseUrl,'/permission','POST',{slug:null},400);
  await call(first.baseUrl,'/user','POST',{email:'invalid',password:'unused',firstName:'Invalid',lastName:'Member',roles:[]},400);
  const mcpUser=await mcpRpc(first.baseUrl,'tools/call',{name:'getUser',arguments:{params:{id:member.id}}},{authorization:auth});
  assert.equal(mcpUser.isError,false,JSON.stringify(mcpUser));assert.equal(JSON.parse(mcpUser.content[0].text).firstName,'Updated');
  const mcpPermission=await mcpRpc(first.baseUrl,'tools/call',{name:'updatePermission',arguments:{body:{id:fixture.permission.id,slug:'mcp.updated'},headers:{authorization:hmac('/permission','PUT',JSON.stringify({id:fixture.permission.id,slug:'mcp.updated'}))}}});
  assert.equal(mcpPermission.isError,false,JSON.stringify(mcpPermission));
  const crossOrgMcp=await mcpRpc(first.baseUrl,'tools/call',{name:'updateUser',arguments:{body:{id:outsider.id,firstName:'Denied'}}},{authorization:auth});assert.equal(crossOrgMcp.isError,true);
  await call(first.baseUrl,`/user/${outsider.id}`,'DELETE',undefined,403,auth);
  await call(first.baseUrl,'/user/batch'+batchQuery([member.id,outsider.id]),'DELETE',undefined,403,auth);
  await call(first.baseUrl,'/user/batch'+batchQuery([member.id]),'DELETE',undefined,200,auth);
  await call(first.baseUrl,`/user/${user.id}`,'DELETE',undefined,200,auth);
  await call(first.baseUrl,`/permission/${fixture.permission.id}`,'DELETE');
  await call(first.baseUrl,`/role/${fixture.role.id}`,'DELETE');
  console.log('PASS 30 IAM directory HTTP routes, JWT permissions/roles, storage-resolved organization scope, atomic mixed-organization rejection, request HMAC and authenticated MCP CRUD');
  // A process restart preserves sessions and records in PostgreSQL.
  first.child.kill('SIGTERM');
  assert.equal((await first.exited)[0], 0);
  const restarted = await start('a');
  const restartedSdk = createForkLaunchSdk({ baseUrl: restarted.baseUrl, headers: { authorization: `Bearer ${token}` } });
  assert.equal((await restartedSdk.iam.users.getSessionUser({ params: { id: 'user-1' } })).response.email, 'member@example.com');
  assert.equal((await call(restarted.baseUrl,`/organization/${fixture.organization.id}`,'GET',undefined,200,auth)).id,fixture.organization.id);
  assert.equal((await restartedSdk.iam.auth.logout()).code, 204);
  assert.equal((await restartedSdk.iam.users.listUsers({query:{}})).code, 401);
  const revokedMcp=await mcpRpc(restarted.baseUrl,'tools/call',{name:'listUsers',arguments:{query:{}}},{authorization:`Bearer ${token}`});assert.equal(revokedMcp.isError,true);
  console.log('PASS generated PostgreSQL IAM, typed TypeScript SDK, tenant isolation, restart persistence, revocation, and SIGTERM');
} finally {
  for (const { child } of processes) {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGTERM');
      setTimeout(() => child.kill('SIGKILL'), 10000).unref();
    }
  }
  await Promise.all(processes.map(({ exited }) => exited));
  await new Promise(resolve=>jwks.close(resolve));
}
