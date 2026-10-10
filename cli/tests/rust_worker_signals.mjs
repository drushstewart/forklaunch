import assert from 'node:assert/strict';
import {spawn, execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {once} from 'node:events';
import {createServer, connect} from 'node:net';
const exec = promisify(execFile);
const binary = process.argv[2];
const redis = process.env.FORKLAUNCH_WORKER_TEST_BACKEND === 'redis';
const role = process.env.FORKLAUNCH_TEST_ROLE;
let storage;
if (redis) storage = {REDIS_URL:process.env.FORKLAUNCH_TEST_REDIS_URL, WORKER_NAMESPACE:process.env.FORKLAUNCH_TEST_WORKER_NAMESPACE};
else {
  const url = new URL(process.env.FORKLAUNCH_TEST_POSTGRES_URL);
  url.username = role; url.password = 'parity-test-only';
  storage = {DATABASE_URL:url.toString()};
}
const scope = {WORKER_SIGNAL_REDIS_URL:process.env.FORKLAUNCH_TEST_REDIS_URL,
  WORKER_SIGNAL_NAMESPACE:`${role}-signals`, TENANT_ID:`${role}-a`, WORKER_QUEUE:'signals'};
const environment = {...process.env, ...storage, ...scope,
  WORKER_POLL_MS:'20', WORKER_LEASE_MS:'1000', WORKER_RENEW_MS:'250', WORKER_DRAIN_MS:'3000'};
const token = 'test-only-distributed-control-token-32-characters';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const records = [];
async function command(args, overrides={}) {
  return JSON.parse((await exec(binary,args,{env:{...environment,...overrides},timeout:10000})).stdout);
}
async function signal(action, overrides={}) {
  // Administrative publication must work with unavailable queue storage.
  return command(['--signal',action],{DATABASE_URL:'postgres://127.0.0.1:1/unavailable', REDIS_URL:'redis://127.0.0.1:1/',...overrides});
}
async function start(overrides={}) {
  const reserve=createServer();
  await new Promise(resolve=>reserve.listen(0,'127.0.0.1',resolve));
  const address=`127.0.0.1:${reserve.address().port}`;
  await new Promise(resolve=>reserve.close(resolve));
  const child=spawn(binary,[],{env:{...environment,WORKER_HTTP_ADDR:address,WORKER_CONTROL_TOKEN:token,...overrides},stdio:['ignore','pipe','pipe']});
  const record={child,exit:once(child,'exit'),error:'',base:`http://${address}`};
  records.push(record);
  child.stdout.resume(); child.stderr.on('data',chunk=>record.error+=chunk);
  await waitState(record,'running');
  return record;
}
async function call(record,path,method='GET',auth=token) {
  return fetch(record.base+path,{method,headers:{Authorization:`Bearer ${auth}`},signal:AbortSignal.timeout(1500)});
}
async function waitState(record,state) {
  const deadline=Date.now()+10000;
  while(Date.now()<deadline) {
    try {
      const response=await call(record,'/health');
      if((await response.json()).state===state) return;
    } catch {}
    if(record.child.exitCode!==null) throw new Error(`worker exited: ${record.error}`);
    await sleep(25);
  }
  throw new Error(`worker did not reach ${state}: ${record.error}`);
}
async function exited(record, expected=0) {
  let timer;
  const result=await Promise.race([record.exit,new Promise((_,reject)=>{
    timer=setTimeout(()=>reject(new Error('worker did not drain')),10000);
  })]).finally(()=>clearTimeout(timer));
  assert.equal(result[0],expected,record.error);
}
try {
  await assert.rejects(signal('invalid'),/Expected --signal/);
  await assert.rejects(signal('pause',{WORKER_SIGNAL_REDIS_URL:''}));
  assert.equal((await signal('pause')).subscribers,0);
  const first=await start(), second=await start();
  const isolated=await start({TENANT_ID:`${role}-b`});
  assert.equal((await signal('pause')).subscribers,2);
  await Promise.all([waitState(first,'paused'),waitState(second,'paused')]);
  assert.equal((await call(first,'/ready')).status,503);
  await waitState(isolated,'running');
  assert.equal((await signal('drain',{WORKER_QUEUE:'other-queue'})).subscribers,0);
  assert.equal((await signal('drain',{WORKER_SIGNAL_NAMESPACE:'other-namespace'})).subscribers,0);
  assert.equal((await call(first,'/control/resume','POST','wrong')).status,401);
  await waitState(second,'paused');
  const resumed=await call(first,'/control/resume','POST');
  assert.equal(resumed.status,202);
  assert.equal((await resumed.json()).subscribers,2);
  await Promise.all([waitState(first,'running'),waitState(second,'running')]);
  assert.equal((await signal('pause')).subscribers,2);
  await Promise.all([waitState(first,'paused'),waitState(second,'paused')]);
  await exec(binary,['--enqueue','held','{}'],{env:environment,timeout:10000});
  await sleep(200);
  assert.equal((await command(['--inspect','held'])).status,'Queued');
  assert.equal((await signal('resume')).subscribers,2);
  const deadline=Date.now()+10000;
  while((await command(['--inspect','held'])).status!=='Completed') {
    assert.ok(Date.now()<deadline,'resumed workers did not process job'); await sleep(25);
  }
  const drained=await call(second,'/control/drain','POST');
  assert.equal(drained.status,202);
  assert.equal((await drained.json()).subscribers,2);
  await Promise.all([exited(first),exited(second)]);
  await waitState(isolated,'running');
  // Late subscribers do not inherit an earlier pause/drain command.
  const restarted=await start();
  assert.equal((await signal('drain')).subscribers,1);
  await exited(restarted);
  assert.equal((await signal('drain',{TENANT_ID:`${role}-b`})).subscribers,1);
  await exited(isolated);
  // Disconnect only this owned proxy, leaving the shared Redis fixture untouched.
  const target=new URL(scope.WORKER_SIGNAL_REDIS_URL);
  const sockets=new Set();
  const proxy=createServer(incoming=>{
    const outgoing=connect(Number(target.port || 6379),target.hostname);
    sockets.add(incoming); sockets.add(outgoing);
    incoming.on('error',()=>outgoing.destroy()); outgoing.on('error',()=>incoming.destroy());
    incoming.on('close',()=>{sockets.delete(incoming);outgoing.destroy();});
    outgoing.on('close',()=>{sockets.delete(outgoing);incoming.destroy();});
    incoming.pipe(outgoing).pipe(incoming);
  });
  await new Promise(resolve=>proxy.listen(0,'127.0.0.1',resolve));
  try {
    const disconnected=await start({WORKER_SIGNAL_REDIS_URL:`redis://127.0.0.1:${proxy.address().port}/`});
    await exec(binary,['--enqueue','disconnect-active','{"sleep_ms":1000}'],{env:environment,timeout:10000});
    const activeDeadline=Date.now()+10000;
    while((await command(['--inspect','disconnect-active'])).status!=='Processing') {
      assert.ok(Date.now()<activeDeadline,'disconnect test did not claim active job'); await sleep(20);
    }
    const closed=new Promise(resolve=>proxy.close(resolve));
    for(const socket of sockets) socket.destroy();
    await closed;
    await exited(disconnected,1);
    assert.match(disconnected.error,/worker_signal_disconnected/);
    assert.equal((await command(['--inspect','disconnect-active'])).status,'Completed');
  } finally {
    for(const socket of sockets) socket.destroy();
    if(proxy.listening) await new Promise(resolve=>proxy.close(resolve));
  }
  console.log(`PASS generated ${redis?'Redis':'PostgreSQL'} distributed controls: process fanout, HTTP publication, queue storage independence, scope isolation, paused jobs, drain, restart and disconnect semantics`);
} finally {
  await Promise.all(records.map(async record=>{
    if(record.child.exitCode===null && record.child.signalCode===null) {record.child.kill('SIGKILL');await record.exit;}
  }));
}
