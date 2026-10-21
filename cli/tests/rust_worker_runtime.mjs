import assert from 'node:assert/strict';
import {spawn, execFile} from './fixtures/secure_worker_runtime.mjs';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {createServer} from 'node:net';
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
  WORKER_CONCURRENCY:'1', WORKER_BATCH_SIZE:'1', WORKER_POLL_MS:'20', WORKER_LEASE_MS:'1000', WORKER_RENEW_MS:'250', WORKER_DRAIN_MS:'5000'});
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
    assert.equal(job.attempts,2);
    assert.equal(Object.hasOwn(job,'last_error'),false);
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
    assert.equal(failed.attempts,2); assert.equal(Object.hasOwn(failed,'last_error'),false);
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
    assert.deepEqual(Object.keys(letters[0]).sort(),['attempts','id','queue','status']);
    assert.deepEqual(JSON.parse(await command(['--retry','failure'])),{requeued:true});
    const reset=await inspect('failure');
    assert.equal(reset.status,'Queued'); assert.equal(reset.attempts,0); assert.equal(Object.hasOwn(reset,'last_error'),false);
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

    const batchEnv={WORKER_QUEUE:'batch-runtime',WORKER_BATCH_SIZE:'3'};
    const batchCommand=args=>command(args,'a',batchEnv);
    const batchInspect=async id=>JSON.parse(await batchCommand(['--inspect',id]));
    const batchWait=async(id,status)=>{
      const deadline=Date.now()+15000;
      while(Date.now()<deadline) {
        const job=await batchInspect(id);
        if(job?.status===status) return job;
        await sleep(20);
      }
      throw new Error(`batch job ${id} did not reach ${status}`);
    };
    for(const id of ['ok-a','fail','ok-b']) {
      await batchCommand(['--enqueue',id,JSON.stringify({sleep_ms:700,fail:id==='fail'})]);
    }
    const batch=await start(batchEnv);
    await Promise.all(['ok-a','fail','ok-b'].map(id=>batchWait(id,'Processing')));
    await sleep(1300);
    for(const id of ['ok-a','fail','ok-b']) {
      const job=await batchInspect(id);
      assert.equal(job.status,'Processing'); assert.equal(job.attempts,1);
    }
    assert.equal((await batchWait('ok-a','Completed')).attempts,1);
    assert.equal((await batchWait('ok-b','Completed')).attempts,1);
    const batchFailed=await batchWait('fail','Failed');
    assert.equal(batchFailed.attempts,2);
    assert.equal(Object.hasOwn(batchFailed,'last_error'),false);
    const batchOutput=await stop(batch);
    assert.match(batchOutput,/batch received: jobs=3/);
    assert.match(batchOutput,/completed=2 retried=1 dead_lettered=1/);
    for(const id of ['drain-a','drain-b','drain-c']) {
      await batchCommand(['--enqueue',id,'{"sleep_ms":700}']);
    }
    const batchDrain=await start(batchEnv);
    await Promise.all(['drain-a','drain-b','drain-c'].map(id=>batchWait(id,'Processing')));
    await batchCommand(['--enqueue','untouched','{}']);
    assert.match(await stop(batchDrain),/completed=3/);
    assert.equal((await batchInspect('untouched')).status,'Queued');
    // Process the untouched job separately before testing crash recovery.
    const batchFlush=await start(batchEnv);
    await batchWait('untouched','Completed');
    await stop(batchFlush);
    for(const id of ['cancel-a','cancel-b']) {
      await batchCommand(['--enqueue',id,'{"sleep_ms":10000,"slow_first":true}']);
    }
    const batchCancel=await start({...batchEnv,WORKER_DRAIN_MS:'100'});
    await Promise.all(['cancel-a','cancel-b'].map(id=>batchWait(id,'Processing')));
    assert.match(await stop(batchCancel),/abandoned=2/);
    await sleep(1100);
    const batchRecovery=await start(batchEnv);
    for(const id of ['cancel-a','cancel-b']) {
      assert.equal((await batchWait(id,'Completed')).attempts,2);
    }
    await stop(batchRecovery);
    await assert.rejects(command([],'a',{WORKER_BATCH_SIZE:'0'}),/worker_invalid_config/);
    await assert.rejects(command([],'a',{WORKER_BATCH_SIZE:'1001'}),/worker_invalid_config/);
    await assert.rejects(command([],'a',{WORKER_BATCH_SIZE:'1000',WORKER_CONCURRENCY:'2'}),/worker_invalid_config/);
    console.log('PASS generated worker batches: grouped handlers, per-job retry, lease renewal, drain, and recovery');

    // Internal HTTP controls govern this process only and retain health during drain.
    const reservation=createServer();
    await new Promise(resolve=>reservation.listen(0,'127.0.0.1',resolve));
    const address=`127.0.0.1:${reservation.address().port}`;
    const httpEnv={WORKER_HTTP_ADDR:address,WORKER_QUEUE:'http-control'};
    const httpCommand=(args)=>command(args,'a',httpEnv);
    const httpInspect=async id=>JSON.parse(await httpCommand(['--inspect',id]));
    const httpWait=async(id,status)=>{
      const deadline=Date.now()+15000;
      while(Date.now()<deadline) {
        const job=await httpInspect(id);
        if(job?.status===status) return job;
        await sleep(20);
      }
      throw new Error(`HTTP-control job ${id} did not reach ${status}`);
    };
    try {
      await httpCommand(['--enqueue','first','{"sleep_ms":2400}']);
      await assert.rejects(httpCommand([]),/worker_http_error/);
      assert.equal((await httpInspect('first')).status,'Queued');
    } finally {
      await new Promise(resolve=>reservation.close(resolve));
    }
    const base=`http://${address}`;
    const call=(path,token,method='GET')=>fetch(`${base}${path}`,{
      method,headers:token?{Authorization:`Bearer ${token}`}:{},signal:AbortSignal.timeout(3000),
    });
    const waitHttp=async()=>{
      const deadline=Date.now()+10000;
      while(Date.now()<deadline) {
        try { if((await call('/health')).status===200) return; } catch {}
        await sleep(20);
      }
      throw new Error('worker HTTP listener did not start');
    };
    // An isolated empty queue keeps the first work untouched for the authenticated case.
    const healthOnly=await start({...httpEnv,WORKER_QUEUE:'http-health-only'});
    await waitHttp();
    assert.equal((await call('/ready')).status,200);
    assert.equal((await call('/control/drain','ignored','POST')).status,404);
    await stop(healthOnly);
    const token='test-only-http-control-token-at-least-32-characters';
    await assert.rejects(command([],'a',{WORKER_CONTROL_TOKEN:token}),/requires WORKER_HTTP_ADDR/);
    await assert.rejects(command([],'a',{...httpEnv,WORKER_CONTROL_TOKEN:'short'}),/worker_invalid_http_config/);
    const controlled=await start({...httpEnv,WORKER_CONTROL_TOKEN:token,
      WORKER_COMPLETED_RETENTION_SECONDS:'1',WORKER_RETENTION_INTERVAL_MS:'100'});
    await waitHttp();
    await httpWait('first','Processing');
    assert.equal((await call('/control/pause',undefined,'POST')).status,401);
    assert.equal((await call('/control/pause','wrong','POST')).status,401);
    assert.equal((await call('/ready')).status,200);
    assert.equal((await call('/control/pause',token,'POST')).status,202);
    assert.equal((await call('/ready')).status,503);
    await httpCommand(['--enqueue','second','{"sleep_ms":2400}']);
    await httpWait('first','Completed');
    assert.equal((await httpInspect('second')).status,'Queued');
    assert.equal((await call('/control/resume',token,'POST')).status,202);
    await httpWait('second','Processing');
    await httpCommand(['--enqueue','third','{}']);
    assert.equal((await call('/control/drain',token,'POST')).status,202);
    const health=await call('/health');
    assert.equal(health.status,200);
    assert.deepEqual(await health.json(),{state:'draining'});
    assert.equal((await call('/ready')).status,503);
    assert.equal((await call('/control/resume',token,'POST')).status,409);
    let controlTimer;
    const [controlledCode]=await Promise.race([controlled.exit,new Promise((_,reject)=>{
      controlTimer=setTimeout(()=>reject(new Error('HTTP-controlled worker did not drain')),10000);
    })]).finally(()=>clearTimeout(controlTimer));
    assert.equal(controlledCode,0,controlled.error);
    assert.match(controlled.output,/completed=2/);
    assert.equal((await httpInspect('second')).status,'Completed');
    assert.equal((await httpInspect('third')).status,'Queued');
    // Cleanup stops at HTTP drain, even though the earlier completion ages out.
    assert.equal((await httpInspect('first')).status,'Completed');
    assert.ok(!controlled.output.includes(token));
    assert.ok(!controlled.error.includes(token));
    console.log('PASS generated worker HTTP authentication, pause/resume, readiness, retention stop, and drain');

    console.log('PASS generated worker dead-letter inspection, explicit retry, bounded retention, and privacy');
    console.log(`PASS generated ${redis ? 'Redis' : 'PostgreSQL'} worker: lease renewal, drain, retry, cancellation, crash recovery, and tenant isolation`);
  }
} finally {
  await Promise.all(children.map(async record=>{
    if(record.child.exitCode===null && record.child.signalCode===null) {record.child.kill('SIGKILL');await record.exit;}
  }));
}
