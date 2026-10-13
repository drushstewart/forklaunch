import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {createHmac} from 'node:crypto';
import {once} from 'node:events';
import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
const encode=value=>Buffer.from(JSON.stringify(value)).toString('base64url');
function token(claims){const payload=`${encode({alg:'HS256',typ:'JWT'})}.${encode({sub:'fixture-subject',exp:Math.floor(Date.now()/1000)+300,...claims})}`;return `${payload}.${createHmac('sha256','fixture-only-session-authorization-key').update(payload).digest('base64url')}`;}
const server=spawn(process.argv[2],[],{env:{...process.env,HOST:'127.0.0.1',SERVICE_HOST:'127.0.0.1',SERVICE_PORT:'0'},stdio:['ignore','pipe','pipe']});
const exited=once(server,'exit');exited.catch(()=>{});
let errors='';server.stderr.on('data',chunk=>errors+=chunk);
const timer=setTimeout(()=>server.kill('SIGKILL'),60000);
try {
 const base=await new Promise((resolve,reject)=>{let output='';server.stdout.on('data',chunk=>{output+=chunk;const match=output.match(/listening on (http:\/\/[^\s]+)/);if(match)resolve(match[1]);});server.once('exit',()=>reject(Error(errors)));});
 await probeMcp(base,['checkPermissions','checkScope','checkFeatures','checkUnconfigured']);
 const cases=[
  ['permissions',{permissions:['read']},200],['permissions',{roles:['admin']},403],
  ['scope',{scopes:['team']},200],['scope',{scopes:['admin']},403],['scope',{scopes:['team','unknown']},403],
  ['features',{active_subscription:true,features:['reports']},200],['features',{features:['reports']},403],['features',{active_subscription:true},403],
  ['unconfigured',{},401]
 ];
 for(const [name,claims,status] of cases) {
  const authorization=`Bearer ${token(claims)}`;
  const response=await fetch(`${base}/policy/${name}`,{headers:{authorization}});
  assert.equal(response.status,status,`${name} ${JSON.stringify(claims)}`);
  const body=await response.json();
  const result=await mcpRpc(base,'tools/call',{name:`check${name[0].toUpperCase()+name.slice(1)}`,arguments:{}},{authorization});
  assert.equal(result.isError,status!==200,JSON.stringify(result));
  if(status===200){assert.deepEqual(body,{subject:'fixture-subject'});assert.deepEqual(JSON.parse(result.content[0].text),body);}
 }
 for(const [headers,status] of [[{},401],[{authorization:'Bearer invalid'},403]]) {
  const response=await fetch(`${base}/policy/permissions`,{headers});assert.equal(response.status,status);
  const result=await mcpRpc(base,'tools/call',{name:'checkPermissions',arguments:{}},headers);assert.equal(result.isError,true);
 }
 server.kill('SIGTERM');const [code,signal]=await exited;assert.equal(code,0,`${signal}: ${errors}`);
 console.log('PASS generated verified-session authorization: HTTP/MCP permissions, exact scopes, subscriptions, features, missing policies, rejected credentials and graceful shutdown');
} finally {clearTimeout(timer);if(server.exitCode===null&&!server.signalCode){server.kill('SIGKILL');await exited.catch(()=>{});}}
