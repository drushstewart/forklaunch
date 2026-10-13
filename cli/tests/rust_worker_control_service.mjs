import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {createServer} from 'node:net';

const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const env=Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.startsWith('WORKER_')&&!['TENANT_ID','DATABASE_URL','REDIS_URL','KAFKA_BROKERS','KAFKA_TRANSACTIONAL_ID'].includes(key)));
// A control service must start without opening any job backend or requiring a tenant.
Object.assign(env,{DATABASE_URL:'postgres://127.0.0.1:1/unavailable',REDIS_URL:'redis://127.0.0.1:1/',KAFKA_BROKERS:'127.0.0.1:1'});
for(const binary of process.argv.slice(2)) {
 const reservation=createServer();
 await new Promise(resolve=>reservation.listen(0,'127.0.0.1',resolve));
 const address=`127.0.0.1:${reservation.address().port}`;
 await new Promise(resolve=>reservation.close(resolve));
 async function launch(overrides,check) {
  const child=spawn(binary,['--control-service'],{env:{...env,...overrides},stdio:['ignore','pipe','pipe']});
  const exit=once(child,'exit');let error='';child.stdout.resume();child.stderr.on('data',part=>error+=part);
  const timer=setTimeout(()=>child.kill('SIGKILL'),15000);
  try {await check(child,exit,()=>error);} finally {
   clearTimeout(timer);
   if(child.exitCode===null&&!child.signalCode){child.kill('SIGKILL');await exit;}
  }
 }
 await launch({WORKER_HTTP_ADDR:address},async(child,exit,error)=>{
  const base=`http://${address}`;
  const deadline=Date.now()+10000;
  while(true) {
   try {const response=await fetch(`${base}/health`,{signal:AbortSignal.timeout(500)});assert.equal(response.status,200);await response.arrayBuffer();break;} catch {}
   assert.equal(child.exitCode,null,error());assert.ok(Date.now()<deadline,error());await sleep(25);
  }
  assert.equal((await fetch(`${base}/ready`)).status,200);
  assert.equal((await fetch(`${base}/control/drain`,{method:'POST'})).status,404);
  child.kill('SIGTERM');const [code,signal]=await exit;assert.equal(code,0,`${signal}: ${error()}`);
 });
 for(const overrides of [{},{WORKER_HTTP_ADDR:address,WORKER_CONTROL_TOKEN:'test-only-control-token-with-32-characters'}]) {
  await launch(overrides,async(_child,exit,error)=>{
   const [code,signal]=await exit;assert.equal(signal,null,error());assert.notEqual(code,0);
   assert.match(error(),/requires WORKER_HTTP_ADDR|Management controls require a distributed signal publisher/);
  });
 }
 console.log(`PASS management-only worker health, disabled controls, configuration rejection and SIGTERM without job storage: ${binary}`);
}
