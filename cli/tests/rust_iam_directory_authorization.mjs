// Full generated service: verified JWT plus persisted session and current directory grants.
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import path from 'node:path';
import {mcpRpc,probeMcp} from './rust_mcp_probe.mjs';
export async function probeDirectoryAuthorization({start,environment,call,jwt,fixture,binary,createForkLaunchSdk}) {
  const settings={...environment('directory',false,true),IAM_DIRECTORY_AUTHORIZATION:'stored-session'};
  const action=kind=>promisify(execFile)(path.join(path.dirname(binary),'examples','seed'),['--directory-auth',kind],{env:process.env,timeout:30000,maxBuffer:1024*1024});
  let server=await start('directory',false,settings);
  const login=()=>call(server.baseUrl,'/auth/login','POST',{email:'member@example.com',password:'directory-password'});
  let token=(await login()).id, authorization=`Bearer ${token}`;
  const rolePath=`/role/${fixture.role.id}`,member=fixture.users[1],outsider=fixture.users[2];
  await call(server.baseUrl,rolePath,'GET',undefined,403,authorization);
  await action('bind');
  await call(server.baseUrl,rolePath,'GET',undefined,200,authorization);
  await call(server.baseUrl,rolePath,'GET',undefined,403,jwt('user-1',['admin','system'],['platform:read']));
  await call(server.baseUrl,rolePath,'GET',undefined,403,authorization+'x');
  const sdk=createForkLaunchSdk({baseUrl:server.baseUrl,headers:{authorization}});
  assert.equal((await sdk.iam.user.getUser({params:{id:member.id}})).code,200);
  assert.equal((await sdk.iam.user.getUser({params:{id:outsider.id}})).code,404);
  const tools=await probeMcp(server.baseUrl,['getRole','getUser']);
  assert.ok(tools.length>=40);
  const mcpRole=async status=>{
    const result=await mcpRpc(server.baseUrl,'tools/call',{name:'getRole',arguments:{params:{id:fixture.role.id}}},{authorization});
    assert.equal(result.isError,!status,JSON.stringify(result));
  };
  await mcpRole(true);
  const traced=await fetch(server.baseUrl+rolePath,{headers:{authorization,'x-correlation-id':'directory-auth-http-correlation'},signal:AbortSignal.timeout(10000)});
  assert.equal(traced.status,200,await traced.text());
  const tracedMcp=await mcpRpc(server.baseUrl,'tools/call',{name:'getRole',arguments:{params:{id:fixture.role.id}}},{authorization,'x-correlation-id':'directory-auth-mcp-correlation'});
  assert.equal(tracedMcp.isError,false);
  await action('verify-correlation');
  // Issued JWT has credential users:read/member grants, not these directory grants.
  const claims=JSON.parse(Buffer.from(token.split('.')[1],'base64url'));
  assert.deepEqual(claims.roles,['member']);assert.deepEqual(claims.permissions,['users:read']);
  await call(server.baseUrl,'/permission','PUT',{id:fixture.permission.id,slug:'removed:permission'});
  await call(server.baseUrl,rolePath,'GET',undefined,403,authorization);await mcpRole(false);
  await call(server.baseUrl,'/permission','PUT',{id:fixture.permission.id,slug:'platform:read'});
  await call(server.baseUrl,rolePath,'GET',undefined,200,authorization);await mcpRole(true);
  await call(server.baseUrl,`/organization/${fixture.organization.id}`,'GET',undefined,403,authorization);
  await call(server.baseUrl,'/role','PUT',{id:fixture.role.id,name:'admin'});
  await call(server.baseUrl,`/organization/${fixture.organization.id}`,'GET',undefined,200,authorization);
  await call(server.baseUrl,'/role','PUT',{id:fixture.role.id,name:'platform:read'});
  await action('move-organization');
  await call(server.baseUrl,`/user/${member.id}`,'GET',undefined,404,authorization);
  await call(server.baseUrl,`/user/${outsider.id}`,'GET',undefined,200,authorization);
  await action('restore-organization');
  await action('disable');await call(server.baseUrl,rolePath,'GET',undefined,403,authorization);await mcpRole(false);
  await action('enable');await call(server.baseUrl,rolePath,'GET',undefined,200,authorization);
  await action('unbind');await call(server.baseUrl,rolePath,'GET',undefined,403,authorization);
  // Independent credential session survives unbinding on legacy bearer surface.
  await call(server.baseUrl,'/users/user-1','GET',undefined,200,authorization);
  await action('bind');
  server.child.kill('SIGTERM');assert.deepEqual(await server.exited,[0,null]);
  server=await start('directory',false,settings);
  await call(server.baseUrl,rolePath,'GET',undefined,200,authorization);await mcpRole(true);
  await call(server.baseUrl,'/auth/logout','POST',undefined,204,authorization);
  await call(server.baseUrl,rolePath,'GET',undefined,403,authorization);await mcpRole(false);
  token=(await login()).id;authorization=`Bearer ${token}`;
  await call(server.baseUrl,rolePath,'GET',undefined,200,authorization);
  await action('delete');
  await call(server.baseUrl,rolePath,'GET',undefined,403,authorization);await mcpRole(false);
  await call(server.baseUrl,'/auth/login','POST',{email:'member@example.com',password:'directory-password'},401);
  server.child.kill('SIGTERM');assert.deepEqual(await server.exited,[0,null]);
  console.log('PASS generated stored-session directory authorization: explicit binding, JWT+persisted-token checks, current grants before RBAC, organization scope, HTTP/MCP/SDK, disable/unbind/revocation/deletion and restart; legacy bearer remains independent');
}
