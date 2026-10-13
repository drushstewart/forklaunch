import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {pathToFileURL} from 'node:url';
import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
const server=spawn(process.argv[2],[],{env:{...process.env,HOST:'127.0.0.1',SERVICE_HOST:'127.0.0.1',SERVICE_PORT:'0'},stdio:['ignore','pipe','pipe']});
const exited=once(server,'exit');exited.catch(()=>{});
let errors='';server.stderr.on('data',chunk=>errors+=chunk);
const timer=setTimeout(()=>server.kill('SIGKILL'),45000);
try {
 const base=await new Promise((resolve,reject)=>{
  let output='';server.stdout.on('data',chunk=>{output+=chunk;const match=output.match(/listening on (http:\/\/[^\s]+)/);if(match)resolve(match[1]);});
  server.once('exit',()=>reject(Error(`Versioned startup failed: ${errors}`)));
 });
 const tools=await probeMcp(base,['write [vv1]','write [vv2]']);
 assert.equal(tools.find(tool=>tool.name==='write [vv1]').inputSchema.properties.body.properties.value.type,'string');
 assert.equal(tools.find(tool=>tool.name==='write [vv2]').inputSchema.properties.body.properties.value.type,'integer');
 for(const [version,value] of [['v1','水 🦀'],['v2',42]]) {
  const result=await mcpRpc(base,'tools/call',{name:`write [v${version}]`,arguments:{body:{kind:version,value}}});
  assert.equal(result.isError,false,JSON.stringify(result));assert.deepEqual(JSON.parse(result.content[0].text),{value});
 }
 for(const [value,version,status] of [['','v1',201],['a','v1',201],['水a','v2',202]]) {
  const response=await fetch(`${base}/versioned-text`,{method:'POST',headers:{'content-type':'text/plain'},body:value});
  assert.equal(response.status,status);assert.deepEqual(await response.json(),{version,value});
  const result=await mcpRpc(base,'tools/call',{name:tools.find(tool=>tool.name.endsWith(` [v${version}]`) && tool.inputSchema.properties.body?.type==='string' && tool.inputSchema.properties.body?.format!=='binary').name,arguments:{body:value}});
  assert.equal(result.isError,false,JSON.stringify(result));assert.deepEqual(JSON.parse(result.content[0].text),{version,value});
 }
 for(const bytes of [[],[0,255,128],Array.from(Buffer.from('水'))]) {
  const response=await fetch(`${base}/versioned-file`,{method:'POST',headers:{'content-type':'application/octet-stream'},body:Buffer.from(bytes)});
  assert.equal(response.status,202);assert.deepEqual(await response.json(),{version:'v2',bytes});
 }
 for(const value of ['', '水']) {
  const result=await mcpRpc(base,'tools/call',{name:'writeFile [vv2]',arguments:{body:value}});
  assert.equal(result.isError,false,JSON.stringify(result));assert.deepEqual(JSON.parse(result.content[0].text),{version:'v2',bytes:Array.from(Buffer.from(value))});
 }
 for(const [value,version,status] of [['a','v1',201],['水a','v2',202]]) {
  const raw=new URLSearchParams({value}).toString();
  const response=await fetch(`${base}/versioned-form`,{method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},body:raw});
  assert.equal(response.status,status);assert.deepEqual(await response.json(),{version,raw});
  const result=await mcpRpc(base,'tools/call',{name:`writeForm [v${version}]`,arguments:{body:{value}}});
  assert.equal(result.isError,false,JSON.stringify(result));assert.deepEqual(JSON.parse(result.content[0].text),{version,raw});
 }
 for(const body of ['value=','value=a&value=b','value[nested]=a']) {
  const response=await fetch(`${base}/versioned-form`,{method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},body});
  assert.equal(response.status,400,body);
 }
 const invalid=await fetch(`${base}/versions`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({kind:'v2',value:'bad'})});assert.equal(invalid.status,400);
 const openapi=await(await fetch(`${base}/openapi.json`)).json();
 assert.deepEqual(openapi.paths['/versions'].post['x-forklaunch-versions'],['v1','v2']);
 assert.equal(openapi.paths['/versions'].post['x-forklaunch-version-contracts'].length,2);
 const {verify}=await import(pathToFileURL(process.argv[4]));await verify(base);
 const rust=spawn(process.argv[3],[base],{stdio:['ignore','pipe','pipe']});let output='';rust.stdout.on('data',chunk=>output+=chunk);rust.stderr.on('data',chunk=>output+=chunk);
 const rustTimer=setTimeout(()=>rust.kill('SIGKILL'),20000);
 try {const [code,signal]=await once(rust,'exit');assert.equal(code,0,`${signal} ${output}`);assert.match(output,/PASS typed Rust versioned SDK/);}finally{clearTimeout(rustTimer);}
 server.kill('SIGTERM');const [code,signal]=await exited;assert.equal(code,0,`${signal} ${errors}`);
 console.log('PASS versioned HTTP/MCP startup, discovery, invocation, rejection, both typed SDKs and graceful shutdown');
}finally{clearTimeout(timer);if(server.exitCode===null&&server.signalCode===null){server.kill('SIGKILL');await exited.catch(()=>{});}}
