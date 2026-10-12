import assert from 'node:assert/strict';
import {spawn,execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomUUID} from 'node:crypto';
import {createRequire} from 'node:module';
import {createServer} from 'node:net';
import {redisTransport} from './fixtures/redis_resp.mjs';
const require=createRequire(import.meta.url),bullmq=require(process.env.BULLMQ_SDK_PATH??'/private/tmp/fl-rust-bullmq-fixture/node_modules/bullmq');
const [binary,mode]=process.argv.slice(2),exec=promisify(execFile);
const prefix=`fl_generated_bullmq_${randomUUID().replaceAll('-','')}`,url=process.env.FORKLAUNCH_TEST_REDIS_URL??'redis://127.0.0.1:32770/12';
const env={...process.env,REDIS_URL:url,REDIS_KEY_PREFIX:prefix,WORKER_QUEUE:'jobs',WORKER_MAX_ATTEMPTS:'2',WORKER_BACKOFF_MS:'20',WORKER_DRAIN_MS:'5000',WORKER_HANDLER_TIMEOUT_MS:'5000'};
const queue=new bullmq.Queue('jobs',{connection:{url},prefix}),children=[];
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function wait(check){for(let i=0;i<400;i++){const result=await check();if(result)return result;await sleep(25);}throw Error('Generated BullMQ runtime timed out');}
async function command(args){return (await exec(binary,args,{env,timeout:10000})).stdout;}
async function start(overrides={}){const child=spawn(binary,[],{env:{...env,...overrides},stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',d=>out+=d);child.stderr.on('data',d=>err+=d);const exit=new Promise(resolve=>child.once('close',resolve));const entry={child,exit};children.push(entry);await wait(()=>{assert.equal(child.exitCode,null,err);return out.includes('worker ready');});return entry;}
async function stop(worker){worker.child.kill('SIGTERM');assert.equal(await Promise.race([worker.exit,sleep(10000).then(()=>{throw Error('Worker shutdown timed out');})]),0);}
async function freePort(){const server=createServer();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const port=server.address().port;await new Promise(resolve=>server.close(resolve));return port;}
try{
 await command(['--migrate']);
 if(mode==='stub'){
  const {id}=JSON.parse(await command(['--enqueue','unimplemented','{}']));const worker=await start();
  await wait(async()=>await (await queue.getJob(id)).getState()==='failed');
  const failedJob=await queue.getJob(id);assert.equal(failedJob.failedReason,'worker_handler_not_implemented');assert.deepEqual(failedJob.stacktrace,['worker_handler_not_implemented','worker_handler_not_implemented']);await stop(worker);
  console.log('PASS generated BullMQ default handler fails explicitly and exhausts retries');
 }else{
  const port=await freePort(),token='fixture-bullmq-control-token-32-characters',base=`http://127.0.0.1:${port}`;
  const worker=await start({WORKER_HTTP_ADDR:`127.0.0.1:${port}`,WORKER_CONTROL_TOKEN:token});
  await wait(async()=>{try{return (await fetch(`${base}/health`)).status===200;}catch{return false;}});
  assert.equal((await fetch(`${base}/ready`)).status,200);
  assert.equal((await fetch(`${base}/control/pause`,{method:'POST'})).status,401);
  const control=action=>fetch(`${base}/control/${action}`,{method:'POST',headers:{authorization:`Bearer ${token}`}});
  assert.equal((await control('pause')).status,202);await sleep(150);
  const {id}=JSON.parse(await command(['--enqueue','paused','{}']));await sleep(150);assert.equal(await (await queue.getJob(id)).getState(),'waiting');
  assert.ok(JSON.parse(await command(['--peek','10'])).some(event=>event.id==='paused'));
  assert.equal((await control('resume')).status,202);await wait(async()=>await (await queue.getJob(id)).getState()==='completed');
  const {id:failed}=JSON.parse(await command(['--enqueue','failed','{"fail":true}']));await wait(async()=>await (await queue.getJob(failed)).getState()==='failed');assert.equal((await queue.getJob(failed)).attemptsMade,2);assert.equal((await queue.getJob(failed)).failedReason,'test_handler_failure');assert.deepEqual((await queue.getJob(failed)).stacktrace,['test_handler_failure','test_handler_failure']);
  assert.ok(JSON.parse(await command(['--dead-letters','10'])).some(job=>job.id===failed&&job.attempts===2));
  assert.deepEqual(JSON.parse(await command(['--dead-letters','0'])),[]);
  const {id:slow}=JSON.parse(await command(['--enqueue','slow','{"sleep_ms":500}']));await wait(async()=>await (await queue.getJob(slow)).getState()==='active');
  assert.equal((await control('drain')).status,202);assert.equal(await worker.exit,0);assert.equal(await (await queue.getJob(slow)).getState(),'completed');
  const restarted=await start();await stop(restarted);
  console.log('PASS generated BullMQ worker retries, HTTP health/readiness/authenticated pause/resume/drain, active-job shutdown, inspection and restart');
 }
}finally{
 for(const worker of children){if(worker.child.exitCode===null){worker.child.kill('SIGTERM');const timer=setTimeout(()=>worker.child.kill('SIGKILL'),10000);await worker.exit;clearTimeout(timer);}}
 await queue.close();const transport=redisTransport(url);let cursor='0';do{const [next,keys]=await transport.command('SCAN',cursor,'MATCH',`${prefix}:*`,'COUNT',1000);if(keys.length)await transport.command('DEL',...keys);cursor=String(next);}while(cursor!=='0');
}
