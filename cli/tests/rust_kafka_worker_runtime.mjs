import {secureJobId} from './fixtures/secure_worker_runtime.mjs';
import assert from 'node:assert/strict';
import {spawn, execFile} from './fixtures/secure_worker_runtime.mjs';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {createServer} from 'node:net';
const exec=promisify(execFile), binary=process.argv[2], mode=process.argv[3];
const baseEnv={...process.env,KAFKA_BROKERS:process.env.FORKLAUNCH_TEST_KAFKA_BROKERS,
  WORKER_NAMESPACE:process.env.FORKLAUNCH_TEST_WORKER_NAMESPACE,TENANT_ID:'tenant-a',WORKER_QUEUE:mode,
  KAFKA_PUBLISHER_ID:`publisher-${mode}`,KAFKA_PARTITIONS:'3',KAFKA_REPLICATION_FACTOR:'1',
  KAFKA_OPERATION_TIMEOUT_MS:'15000',WORKER_MAX_ATTEMPTS:'2',WORKER_BACKOFF_SECONDS:'0',
  WORKER_HANDLER_TIMEOUT_MS:'15000',WORKER_DRAIN_MS:'5000'};
const records=[]; let nextReplica=0;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const command=async(args,overrides={})=>(await exec(binary,args,{env:{...baseEnv,...overrides},timeout:45000})).stdout;
const peek=async(overrides={})=>JSON.parse(await command(['--peek','100'],overrides));
const dead=async(overrides={})=>JSON.parse(await command(['--dead-letters','100'],overrides));
async function waitFor(check,message) {
  const deadline=Date.now()+45000;
  while(Date.now()<deadline) {if(await check()) return;await sleep(50);}
  throw new Error(message);
}
async function start(overrides={}) {
  const child=spawn(binary,[],{env:{...baseEnv,KAFKA_TRANSACTIONAL_ID:`replica-${++nextReplica}`,...overrides},stdio:['ignore','pipe','pipe']});
  const record={child,exit:once(child,'exit'),output:'',error:''};records.push(record);
  child.stdout.on('data',chunk=>record.output+=chunk);child.stderr.on('data',chunk=>record.error+=chunk);
  await waitFor(()=>{
    if(child.exitCode!==null) throw new Error(`worker exited: ${record.error}`);
    return record.output.includes('worker ready');
  },'worker did not start');
  return record;
}
async function exited(record,code=0) {
  let timer;
  const [actual]=await Promise.race([record.exit,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('worker did not exit')),20000);})]).finally(()=>clearTimeout(timer));
  assert.equal(actual,code,record.error);
  return record.output;
}
async function stop(record,signal='SIGTERM') {record.child.kill(signal);return exited(record,signal==='SIGKILL'?null:0);}
async function address() {
  const server=createServer();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const value=`127.0.0.1:${server.address().port}`;await new Promise(resolve=>server.close(resolve));return value;
}
try {
  await command(['--migrate']);
  if(mode==='stub') {
    await command(['--enqueue','unimplemented','{}']);
    const worker=await start();
    await waitFor(async()=> (await dead()).some(job=>job.id==='unimplemented'),'unimplemented handler never failed');
    assert.equal(Object.hasOwn((await dead())[0],'last_error'),false);
    await stop(worker);
    console.log('PASS generated Kafka worker never acknowledges an unimplemented handler');
  } else {
    await command(['--enqueue','success','{}']);await command(['--enqueue','failure','{"fail":true}']);
    assert.equal((await peek()).length,2);assert.equal((await peek()).length,2);
    const worker=await start();
    await waitFor(async()=> (await dead()).some(job=>job.id==='failure'),'Kafka failure was not retained');
    await waitFor(async()=> (await peek()).length===0,'Kafka records were not committed');
    const output=await stop(worker);assert.match(output,/completed=1 retried=1 dead_lettered=1/);
    assert.deepEqual((await dead()).map(job=>[job.id,job.attempts]),[['failure',2]]);
    assert.ok(!output.includes('sensitive detail'));
    const restarted=await start();await sleep(1000);assert.match(await stop(restarted),/completed=0/);
    // Kill a separate process while its application handler is running, then recover the uncommitted offset.
    const recoveryScope={WORKER_QUEUE:'crash'};await command(['--migrate'],recoveryScope);
    await command(['--enqueue','crash','{"sleep_ms":4000}'],recoveryScope);
    const crashed=await start({...recoveryScope,KAFKA_TRANSACTIONAL_ID:'crash-replica'});
    await waitFor(async()=> (await peek(recoveryScope)).length===1,'crash job was not pending');
    await waitFor(()=>crashed.output.includes(`test-handler-started ${secureJobId('crash')}`),'worker never entered the crash handler');
    await stop(crashed,'SIGKILL');
    assert.equal((await peek(recoveryScope)).length,1);
    const recovered=await start({...recoveryScope,KAFKA_TRANSACTIONAL_ID:'crash-replica'});
    await waitFor(async()=> (await peek(recoveryScope)).length===0,'crashed Kafka record was not recovered');
    assert.match(await stop(recovered),/completed=1/);
    // Distributed controls share the same state as management HTTP and consumption.
    const token='test-only-kafka-http-control-token-32-characters';
    const scope={WORKER_QUEUE:'controls',WORKER_SIGNAL_REDIS_URL:process.env.FORKLAUNCH_TEST_REDIS_URL,
      WORKER_SIGNAL_NAMESPACE:`${baseEnv.WORKER_NAMESPACE}-signals`,WORKER_CONTROL_TOKEN:token};
    await command(['--migrate'],scope);
    const firstAddress=await address(),secondAddress=await address();
    const first=await start({...scope,WORKER_HTTP_ADDR:firstAddress}),second=await start({...scope,WORKER_HTTP_ADDR:secondAddress});
    const call=(target,path,method='GET',auth=token)=>fetch(`http://${target}${path}`,{method,headers:{Authorization:`Bearer ${auth}`},signal:AbortSignal.timeout(3000)});
    await waitFor(async()=>{try{return(await call(firstAddress,'/health')).status===200&&(await call(secondAddress,'/health')).status===200;}catch{return false;}},'Kafka HTTP listeners did not start');
    const signal=async action=>JSON.parse(await command(['--signal',action],{...scope,KAFKA_BROKERS:'127.0.0.1:1'}));
    assert.equal((await signal('pause')).subscribers,2);
    await waitFor(async()=> (await call(firstAddress,'/ready')).status===503&&(await call(secondAddress,'/ready')).status===503,'Kafka group did not pause');
    assert.equal((await call(firstAddress,'/control/resume','POST','wrong')).status,401);
    await command(['--enqueue','held','{}'],scope);await sleep(250);assert.equal((await peek(scope)).length,1);
    const resumed=await call(firstAddress,'/control/resume','POST');assert.equal(resumed.status,202);assert.equal((await resumed.json()).subscribers,2);
    await waitFor(async()=> (await peek(scope)).length===0,'Kafka group did not resume');
    assert.equal((await signal('drain')).subscribers,2);
    await Promise.all([exited(first),exited(second)]);
    assert.ok(!first.output.includes(token)&&!second.output.includes(token));
    console.log('PASS generated Kafka worker: broker persistence, peek, retries, dead letters, process crash recovery, authenticated HTTP controls and distributed fanout');
  }
} finally {
  await Promise.all(records.map(async record=>{if(record.child.exitCode===null&&record.child.signalCode===null){record.child.kill('SIGKILL');await record.exit;}}));
}
