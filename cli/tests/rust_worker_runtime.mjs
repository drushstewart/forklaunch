import assert from 'node:assert/strict';
import {spawn, execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
const exec = promisify(execFile);
const binary = process.argv[2];
const mode = process.argv[3];
const redis = process.env.FORKLAUNCH_WORKER_TEST_BACKEND === 'redis';
const role = process.env.FORKLAUNCH_TEST_ROLE;
let storage;
if(redis) {
  storage = {REDIS_URL:process.env.FORKLAUNCH_TEST_REDIS_URL,
    WORKER_NAMESPACE:process.env.FORKLAUNCH_TEST_WORKER_NAMESPACE};
} else {
  const url = new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
  url.username = role;
  url.password = 'parity-test-only';
  storage = {DATABASE_URL:url.toString()};
}
const env = tenant => ({...process.env, ...storage, TENANT_ID:`${role}-${tenant}`,
  WORKER_QUEUE:mode === 'stub' ? 'stub' : 'runtime', WORKER_MAX_ATTEMPTS:'2', WORKER_BACKOFF_SECONDS:'0',
  WORKER_CONCURRENCY:'1', WORKER_POLL_MS:'20', WORKER_LEASE_MS:'1000', WORKER_RENEW_MS:'250', WORKER_DRAIN_MS:'5000'});
const children = [];
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function command(args, tenant = 'a', overrides={}) { return (await exec(binary, args, {env:{...env(tenant),...overrides}, timeout:10000})).stdout; }
async function inspect(id, tenant = 'a') { return JSON.parse(await command(['--inspect',id],tenant)); }
async function waitState(id,status,tenant='a') {
  const deadline=Date.now()+15000;
  while(Date.now()<deadline) {
    const job=await inspect(id,tenant);
    if(job?.status===status) return job;
    await sleep(25);
  }
  throw new Error(`job ${id} did not reach ${status}`);
}
async function start(overrides={}) {
  const child=spawn(binary,[],{env:{...env('a'),...overrides},stdio:['ignore','pipe','pipe']});
  const exit=once(child,'exit');
  const record={child,exit,output:'',error:''}; children.push(record);
  child.stdout.on('data',chunk=>record.output+=chunk);
  child.stderr.on('data',chunk=>record.error+=chunk);
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error(`worker startup timed out: ${record.error}`)),10000);
    child.stdout.on('data',()=>{if(record.output.includes('worker ready')) {clearTimeout(timer);resolve();}});
    child.once('error',error=>{clearTimeout(timer);reject(error);});
    child.once('exit',code=>{clearTimeout(timer);reject(new Error(`worker exited ${code}: ${record.error}`));});
  });
  return record;
}
async function stop(record,signal='SIGTERM') {
  record.child.kill(signal);
  let timer;
  const result=await Promise.race([record.exit,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('worker did not stop')),10000);})]).finally(()=>clearTimeout(timer));
  if(signal==='SIGTERM') assert.equal(result[0],0,record.error);
  return record.output;
}
try {
  if(mode==='stub') {
    await command(['--enqueue','unimplemented','{}']);
    const worker=await start();
    const job=await waitState('unimplemented','Failed');
    assert.equal(job.last_error,'worker_handler_not_implemented');
    await stop(worker);
    console.log('PASS fresh worker never acknowledges an unimplemented handler');
  } else {
    await command(['--enqueue','first',JSON.stringify({sleep_ms:2400})]);
    await command(['--enqueue','first','{}'],'b');
    const worker=await start();
    await waitState('first','Processing');
    await sleep(1300);
    const active=await inspect('first');
    assert.equal(active.status,'Processing'); assert.equal(active.attempts,1);
    await command(['--enqueue','pending','{}']);
    const output=await stop(worker);
    assert.match(output,/completed=1/);
    assert.equal((await inspect('first')).status,'Completed');
    assert.equal((await inspect('first','b')).status,'Queued');
    assert.equal((await inspect('pending')).status,'Queued');
    await assert.rejects(command(['--enqueue','first','{}']));

    await command(['--enqueue','failure','{"fail":true}']);
    const restarted=await start();
    await waitState('pending','Completed');
    const failed=await waitState('failure','Failed');
    assert.equal(failed.attempts,2); assert.equal(failed.last_error,'test_handler_failure');
    await stop(restarted);

    await command(['--enqueue','cancel','{"sleep_ms":10000,"slow_first":true}']);
    const draining=await start({WORKER_DRAIN_MS:'100'});
    await waitState('cancel','Processing');
    const cancelled=await stop(draining);
    assert.match(cancelled,/abandoned=1/);
    assert.equal((await inspect('cancel')).status,'Processing');
    await sleep(1100);
    const recovery=await start();
    assert.equal((await waitState('cancel','Completed')).attempts,2);
    await stop(recovery);

    await command(['--enqueue','crash','{"sleep_ms":10000,"slow_first":true}']);
    const crashed=await start();
    await waitState('crash','Processing');
    await stop(crashed,'SIGKILL');
    await sleep(1100);
    const recovered=await start();
    assert.equal((await waitState('crash','Completed')).attempts,2);
    await stop(recovered);
    for(const id of ['parallel-1','parallel-2']) {
      await command(['--enqueue',id,'{"sleep_ms":2400}']);
    }
    const parallel=await start({WORKER_CONCURRENCY:'2'});
    await Promise.all(['parallel-1','parallel-2'].map(id=>waitState(id,'Processing')));
    await command(['--enqueue','parallel-pending','{}']);
    const parallelOutput=await stop(parallel);
    assert.match(parallelOutput,/completed=2/);
    for(const id of ['parallel-1','parallel-2']) {
      const job=await inspect(id);
      assert.equal(job.status,'Completed'); assert.equal(job.attempts,1);
    }
    assert.equal((await inspect('parallel-pending')).status,'Queued');
    const letters=JSON.parse(await command(['--dead-letters','1']));
    assert.equal(letters.length,1); assert.equal(letters[0].id,'failure');
    assert.deepEqual(Object.keys(letters[0]).sort(),['attempts','id','last_error','queue','status']);
    assert.deepEqual(JSON.parse(await command(['--retry','failure'])),{requeued:true});
    const reset=await inspect('failure');
    assert.equal(reset.status,'Queued'); assert.equal(reset.attempts,0); assert.equal(reset.last_error,null);
    assert.deepEqual(JSON.parse(await command(['--retry','failure'])),{requeued:false});
    const adminWorker=await start();
    assert.equal((await waitState('failure','Failed')).attempts,2);
    await waitState('parallel-pending','Completed');
    await stop(adminWorker);
    await command(['--enqueue','retention-pending','{}']);
    assert.deepEqual(JSON.parse(await command(['--purge-completed','86400','1000'])),{purged:0});
    await sleep(1100);
    assert.deepEqual(JSON.parse(await command(['--purge-completed','1','1'])),{purged:1});
    assert.deepEqual(JSON.parse(await command(['--purge-failed','1','1'])),{purged:1});
    assert.equal(await inspect('failure'),null);
    assert.equal((await inspect('retention-pending')).status,'Queued');
    assert.equal((await inspect('first','b')).status,'Queued');
    for(const args of [['--dead-letters','0'],['--purge-failed','0','1'],['--purge-completed','1','1001']]) {
      await assert.rejects(command(args));
    }
    const retentionEnv={WORKER_QUEUE:'auto-retention'};
    const retainedInspect=async id=>JSON.parse(await command(['--inspect',id],'a',retentionEnv));
    async function waitRetained(id,status) {
      const deadline=Date.now()+10000;
      while(Date.now()<deadline) {
        const job=await retainedInspect(id);
        if(job?.status===status) return job;
        await sleep(25);
      }
      throw new Error(`retention job ${id} did not reach ${status}`);
    }
    await command(['--enqueue','old-completed','{}'],'a',retentionEnv);
    await command(['--enqueue','old-failed','{"fail":true}'],'a',retentionEnv);
    const seedRetention=await start(retentionEnv);
    await waitRetained('old-completed','Completed');
    await waitRetained('old-failed','Failed');
    await stop(seedRetention);
    await command(['--enqueue','slow','{"sleep_ms":2400}'],'a',retentionEnv);
    await command(['--enqueue','untouched','{}'],'a',retentionEnv);
    await command(['--enqueue','old-completed','{}'],'b',retentionEnv);
    const retaining=await start({...retentionEnv,
      WORKER_COMPLETED_RETENTION_SECONDS:'1', WORKER_FAILED_RETENTION_SECONDS:'1',
      WORKER_RETENTION_INTERVAL_MS:'100', WORKER_RETENTION_BATCH_LIMIT:'1'});
    await waitRetained('slow','Processing');
    await sleep(1300);
    assert.equal(await retainedInspect('old-completed'),null);
    assert.equal(await retainedInspect('old-failed'),null);
    const retentionOutput=await stop(retaining);
    assert.match(retentionOutput,/completed_purged=1 failed_purged=1/);
    assert.equal((await retainedInspect('slow')).status,'Completed');
    assert.equal((await retainedInspect('untouched')).status,'Queued');
    assert.equal(JSON.parse(await command(['--inspect','old-completed'],'b',retentionEnv)).status,'Queued');
    console.log('PASS generated worker opt-in retention, active-job protection, tenant isolation, and coordinated drain');
    console.log('PASS generated worker dead-letter inspection, explicit retry, bounded retention, and privacy');
    console.log(`PASS generated ${redis ? 'Redis' : 'PostgreSQL'} worker: lease renewal, drain, retry, cancellation, crash recovery, and tenant isolation`);
  }
} finally {
  await Promise.all(children.map(async record=>{
    if(record.child.exitCode===null && record.child.signalCode===null) {record.child.kill('SIGKILL');await record.exit;}
  }));
}
