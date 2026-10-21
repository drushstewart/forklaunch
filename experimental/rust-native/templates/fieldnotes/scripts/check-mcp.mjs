import assert from 'node:assert/strict';
const api=process.env.API_CHECK_URL??'http://notes:8000',auth=process.env.AUTH_CHECK_URL??'http://auth:3001';
const origin=process.env.PUBLIC_ORIGIN;
const sign=await fetch(auth+'/api/auth/sign-in/email',{method:'POST',headers:{origin,'content-type':'application/json'},body:JSON.stringify({email:'alice@example.test',password:process.env.DEMO_PASSWORD})});
assert.equal(sign.status,200);
const cookie=sign.headers.getSetCookie().map(v=>v.split(';')[0]).join('; ');
let session,protocol;
async function rpc(method,params,id){
 const result=await fetch(api+'/mcp',{method:'POST',headers:{origin,cookie,'content-type':'application/json',accept:'application/json, text/event-stream',...(session?{'mcp-session-id':session}:{}),...(protocol?{'mcp-protocol-version':protocol}:{})},body:JSON.stringify({jsonrpc:'2.0',...(id===undefined?{}:{id}),method,...(params?{params}:{})})});
 assert.ok(result.ok,`${method} HTTP ${result.status}`);
 session=result.headers.get('mcp-session-id')??session;
 const raw=await result.text();if(!raw)return;
 const data=result.headers.get('content-type')?.includes('text/event-stream')?JSON.parse(raw.split('\n').find(line=>line.startsWith('data:')).slice(5)):JSON.parse(raw);
 assert.ok(!data.error,`${method} RPC error`);return data.result;
}
const initialized=await rpc('initialize',{protocolVersion:'2025-03-26',capabilities:{},clientInfo:{name:'fieldnotes-review',version:'1'}},1);
protocol=initialized.protocolVersion;
await rpc('notifications/initialized');
const list=await rpc('tools/list',{},2);
assert.ok(list.tools.length>0);
assert.ok(list.tools.every(tool=>!tool.name.toLowerCase().includes('applyinsight')),'private callback excluded');
const tool=list.tools.find(tool=>tool.name.toLowerCase().replaceAll('_','').includes('listnotes'));
assert.ok(tool,'note list tool registered');
const result=await rpc('tools/call',{name:tool.name,arguments:{params:{workspace_id:'11111111-1111-4111-8111-111111111111'}}},3);
assert.ok(!result.isError,'real authenticated tool dispatch succeeds');
assert.ok(result.content.length>0);
console.log('PASS MCP initialize, tool discovery, private callback exclusion, authenticated native route invocation');
