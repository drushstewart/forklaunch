// A generated Kafka worker must survive transient broker failures: a broker that
// starts after the worker (first connect), and a broker restart while it runs.
// Uses its own broker container (persistent data, fixed loopback port) so
// stopping it does not disturb the shared fixture broker.
import assert from 'node:assert/strict';
import {spawn, execFile} from './fixtures/secure_worker_runtime.mjs';
import {execFile as dockerExecFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {createServer} from 'node:net';
const exec=promisify(execFile), docker=(...args)=>promisify(dockerExecFile)('docker',args,{timeout:120000});
const binary=process.argv[2], image=process.argv[3];
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const port=await new Promise(resolve=>{const server=createServer();server.listen(0,'127.0.0.1',()=>{const {port}=server.address();server.close(()=>resolve(port));});});
const brokers=`127.0.0.1:${port}`, container=`forklaunch-rust-kafka-restart-${process.pid}`;
const baseEnv={...process.env,KAFKA_BROKERS:brokers,WORKER_NAMESPACE:`${process.env.FORKLAUNCH_TEST_WORKER_NAMESPACE}-restart`,
  TENANT_ID:'tenant-a',WORKER_QUEUE:'restart',KAFKA_PUBLISHER_ID:'publisher-restart',KAFKA_PARTITIONS:'1',
  KAFKA_REPLICATION_FACTOR:'1',KAFKA_OPERATION_TIMEOUT_MS:'5000',KAFKA_STARTUP_TIMEOUT_MS:'120000',
  WORKER_MAX_ATTEMPTS:'2',WORKER_BACKOFF_SECONDS:'0',WORKER_HANDLER_TIMEOUT_MS:'15000',WORKER_DRAIN_MS:'5000'};
const records=[];
const command=async(args)=>(await exec(binary,args,{env:baseEnv,timeout:60000})).stdout;
const peek=async()=>JSON.parse(await command(['--peek','100']));
async function waitFor(check,message,ms=90000) {
  const deadline=Date.now()+ms;
  while(Date.now()<deadline) {try{if(await check()) return;}catch{} await sleep(250);}
  throw new Error(message);
}
async function healthy() {
  await waitFor(async()=>{await docker('exec',container,'rpk','cluster','health','--exit-when-healthy');return true;},'restart broker never became healthy');
}
function start(replica) {
  const child=spawn(binary,[],{env:{...baseEnv,KAFKA_TRANSACTIONAL_ID:replica},stdio:['ignore','pipe','pipe']});
  const record={child,exit:once(child,'exit'),output:'',error:''};records.push(record);
  child.stdout.on('data',chunk=>record.output+=chunk);child.stderr.on('data',chunk=>record.error+=chunk);
  return record;
}
const ready=record=>waitFor(()=>{if(record.child.exitCode!==null) throw new Error(`worker exited: ${record.error}`);return record.output.includes('worker ready');},'worker did not become ready');
async function stop(record) {
  record.child.kill('SIGTERM');
  const [code]=await record.exit;assert.equal(code,0,record.error);
  return record.output;
}
async function processed(id) {
  await command(['--enqueue',id,'{}']);
  await waitFor(async()=> (await peek()).length===0,`${id} was not processed`);
}
try {
  await docker('run','--detach','--name',container,'--memory','1536m','--cpus','2','--publish',`${brokers}:9092`,image,
    'redpanda','start','--overprovisioned','--smp','1','--memory','768M','--reserve-memory','0M','--node-id','0',
    '--check=false','--kafka-addr','0.0.0.0:9092','--advertise-kafka-addr',brokers);
  await healthy();
  await command(['--migrate']);

  // 1. Broker restart while the worker is idle: the worker keeps polling, then resumes.
  const running=start('restart-replica-a');await ready(running);
  await processed('before-restart');
  await docker('stop','--time','2',container);
  await sleep(6000);
  assert.equal(running.child.exitCode,null,`worker exited during broker outage: ${running.error}`);
  assert.match(running.error,/retrying|Connection refused|transport|Disconnected/i,'broker outage was not logged');
  await docker('start',container);await healthy();
  await processed('after-restart');
  assert.match(await stop(running),/completed=2/);

  // 2. Late start: the broker comes up after the worker; startup retries instead of exiting.
  await docker('stop','--time','2',container);
  const late=start('restart-replica-b');
  await sleep(6000);
  assert.equal(late.child.exitCode,null,`worker exited before the broker started: ${late.error}`);
  assert.ok(!late.output.includes('worker ready'));
  assert.match(late.error,/not ready; retrying/,'startup retry was not logged');
  await docker('start',container);await healthy();
  await ready(late);
  await processed('late-start');
  assert.match(await stop(late),/completed=1/);
  console.log('PASS generated Kafka worker retries a late-starting broker and survives a broker restart');
} finally {
  await Promise.all(records.map(async record=>{if(record.child.exitCode===null&&record.child.signalCode===null){record.child.kill('SIGKILL');await record.exit;}}));
  await docker('rm','-f',container).catch(()=>{});
}
