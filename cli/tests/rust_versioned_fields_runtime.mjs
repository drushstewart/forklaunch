import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
const server=spawn(process.argv[2],[],{env:{...process.env,HOST:'127.0.0.1',PORT:'0',RUST_LOG:'info',MCP_ENABLED:'true',PROTOCOL:'http',VERSION:'v1',DOCS_PATH:'/docs'},stdio:['ignore','pipe','pipe']});
const exited=once(server,'exit');exited.catch(()=>{});
let errors='';server.stderr.on('data',chunk=>errors+=chunk);
const timer=setTimeout(()=>server.kill('SIGKILL'),45000);
try{
  const base=await new Promise((resolve,reject)=>{
    let output='';server.stdout.on('data',chunk=>{output+=chunk;const match=output.match(/listening on (http:\/\/[^\s]+)/);if(match)resolve(match[1]);});
    server.once('exit',()=>reject(Error(`Field service startup failed: ${errors}`)));
  });
  await probeMcp(base,['readFields [vfirst]','readFields [vlast]']);
  let checks=0;
  for(const [count,version,value,present] of [[undefined,'last',110,true],[0,'first',null,true],
    [9,'first',9,true],[13,'first',13,true],[42,'last',null,false],[15,'last',115,true]]){
    for(const header of [undefined,0,5]){
      const query=count===undefined?'':`count=${count}`;
      const expected={version,id:'A',count:value,countPresent:present,
        header:header===undefined?null:header===0?'0':10,headerPresent:header!==undefined,
        rawQuery:query,rawHeader:header===undefined?null:String(header)};
      const response=await fetch(`${base}/fields/%20a%20${query?'?'+query:''}`,{
        headers:header===undefined?{}:{'x-code':String(header)},
      });
      assert.equal(response.status,200);assert.deepEqual(await response.json(),expected);
      const result=await mcpRpc(base,'tools/call',{name:`readFields [v${version}]`,arguments:{
        params:{id:' a '},query:count===undefined?{}:{count},headers:header===undefined?{}:{'x-code':header},
      }});
      assert.equal(result.isError,false,JSON.stringify(result));
      assert.deepEqual(JSON.parse(result.content[0].text),expected);
      checks+=2;
    }
  }
  for(const [wire,id] of [['a%2Fb','A/B'],['%252F','%2F'],['a+b','A+B'],['%E2%98%83','☃']]){
    const response=await fetch(`${base}/fields/${wire}`);
    assert.equal(response.status,200);assert.equal((await response.json()).id,id);
  }
  for(const wire of ['%','%2','%GG','%FF','%C0%AF']){
    const response=await fetch(`${base}/fields/${wire}`);
    assert.equal(response.status,400,wire);
  }
  for(const [query,headers] of [['count=oops',{}],['count=-1',{}],['count=10',{'x-code':'nope'}]]){
    const response=await fetch(`${base}/fields/a?${query}`,{headers});
    assert.equal(response.status,400);assert.doesNotMatch(await response.text(),/private/);
  }
  console.log(`PASS ${checks} HTTP/MCP versioned field results: typed defaults, fallback, absent outputs, header truthiness and unchanged wire values`);
}finally{
  clearTimeout(timer);server.kill('SIGTERM');
  const shutdown=setTimeout(()=>server.kill('SIGKILL'),10000);
  try{const [code,signal]=await exited;assert.equal(code,0,`${signal} ${errors}`);}finally{clearTimeout(shutdown);}
}
