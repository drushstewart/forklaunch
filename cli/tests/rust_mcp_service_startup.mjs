import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {probeMcp} from './rust_mcp_probe.mjs';
const [binary]=process.argv.slice(2);assert.ok(binary);
for(const config of [{MCP_ENABLED:'true'},{MCP_ENABLED:'false'},{MCP_ENABLED:'true',MCP_PATH:'/tools'}]) {
  const env={...Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.startsWith('OTEL_')&&!key.startsWith('MCP_'))),HOST:'127.0.0.1',SERVICE_HOST:'127.0.0.1',SERVICE_PORT:'0',...config};
  const child=spawn(binary,[],{env,stdio:['ignore','pipe','pipe']});let output='',errors='',closed=false;
  child.stdout.on('data',d=>output+=d);child.stderr.on('data',d=>errors+=d);const exit=new Promise(resolve=>child.once('close',code=>{closed=true;resolve(code);}));
  try {
    let base;for(let i=0;i<200;i++){base=output.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];if(base)break;assert.ok(!closed,errors);await new Promise(resolve=>setTimeout(resolve,25));}assert.ok(base,errors);
    if(config.MCP_ENABLED==='false'){assert.equal((await fetch(`${base}/health`)).status,200);assert.equal((await fetch(`${base}/mcp`,{method:'POST'})).status,404);}
    else{await probeMcp(base,[],config.MCP_PATH??'/mcp');if(config.MCP_PATH)assert.equal((await fetch(`${base}/mcp`,{method:'POST'})).status,404);}
  } finally {
    if(!closed)child.kill('SIGTERM');const timer=setTimeout(()=>child.kill('SIGKILL'),10000);try{assert.equal(await exit,0,errors);}finally{clearTimeout(timer);}
  }
}
console.log('PASS generated service HTTP/MCP startup, enabled/disabled/custom endpoint configuration and graceful shutdown');
