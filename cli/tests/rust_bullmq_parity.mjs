import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,realpath} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import {redisTransport} from './fixtures/redis_resp.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..'),require=createRequire(import.meta.url);
const bullmq=require(process.env.BULLMQ_SDK_PATH??'/private/tmp/fl-rust-bullmq-fixture/node_modules/bullmq');
assert.equal(require(path.join(process.env.BULLMQ_SDK_PATH??'/private/tmp/fl-rust-bullmq-fixture/node_modules/bullmq','package.json')).version,'6.3.10');
const tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
const ts=require(require.resolve('typescript',{paths:[path.dirname(await realpath(tsc))]}));
const compile=source=>ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const adminUrl=process.env.FORKLAUNCH_TEST_REDIS_URL??'redis://127.0.0.1:32770/12',transport=redisTransport(adminUrl);
const prefix=`fl_bullmq_${randomUUID().replaceAll('-','')}`;
const username=`fl_bullmq_${randomUUID().replaceAll('-','')}`,password=randomUUID(),restricted=new URL(adminUrl);restricted.username=username;restricted.password=password;const url=restricted.toString();
const env={...process.env,REDIS_KEY_PREFIX:`${prefix}:`,FORKLAUNCH_REDIS_KEY_PREFIX:'ignored-alias:'};
const isolation={};
vm.runInNewContext(compile(await readFile(path.join(root,'blueprint/implementations/worker/bullmq/domain/tenantIsolation.ts'),'utf8')),{exports:isolation,process:{env},console});
async function load(relative){const exports={};vm.runInNewContext(compile(await readFile(path.join(root,relative),'utf8')),{exports,console,require(name){if(name==='bullmq')return bullmq;if(name==='../domain/tenantIsolation')return isolation;throw new Error(`Unexpected runtime import ${name}`);}});return exports;}
const {BullMqWorkerProducer}=await load('blueprint/implementations/worker/bullmq/producers/bullMqWorker.producer.ts');
const {BullMqWorkerConsumer}=await load('blueprint/implementations/worker/bullmq/consumers/bullMqWorker.consumer.ts');
async function native(input,overrides={}){
 const child=spawn('cargo',['run','--quiet','--manifest-path',path.join(root,'blueprint-rs/Cargo.toml'),'-p','forklaunch-worker-redis','--features','bullmq','--example','bullmq_parity'],{cwd:root,env:{...env,FORKLAUNCH_TEST_REDIS_URL:url,...overrides},stdio:['pipe','pipe','inherit']});let output='';child.stdout.on('data',chunk=>output+=chunk);child.stdin.end(JSON.stringify(input));
 const timer=setTimeout(()=>child.kill('SIGTERM'),300000);const status=await new Promise(resolve=>child.once('close',resolve));clearTimeout(timer);assert.equal(status,0);return JSON.parse(output);
}
const options=backoffType=>({retries:3,interval:20,backoffType,queueOptions:{connection:{url},prefix:'caller-override'}});
async function waitFor(check){const deadline=Date.now()+15000;while(Date.now()<deadline){const value=await check();if(value)return value;await new Promise(resolve=>setTimeout(resolve,20));}throw new Error('BullMQ fixture condition timed out');}
const handles=[];
try{
 await transport.command('ACL','SETUSER',username,'reset','on',`>${password}`,'resetkeys','resetchannels',`~${prefix}:*`,`&${prefix}:*`,'+@all','-flushdb','-flushall','-acl','-config','-shutdown','-debug','-module','-migrate','-replicaof','-slaveof','-swapdb','-keys','-scan');
 for(const backoffType of ['fixed','exponential']){
  const queueName=`native-to-ts-${backoffType}`;
  const events=[{id:'first',order:1},{id:'second',order:2,failAttempts:1},{id:'third',order:3,failAttempts:10}];
  const produced=await native({mode:'produce',queue:queueName,events,backoff:backoffType,retries:3,interval:20});
  assert.equal(produced.ids.length,3);
  const attempts=new Map(),failures=[];
  const consumer=new BullMqWorkerConsumer(queueName,options(backoffType),async values=>{assert.equal(values.length,1);const event=values[0],count=attempts.get(event.id)??0;attempts.set(event.id,count+1);if(count<(event.failAttempts??0))throw new Error('fixture_failure');},values=>failures.push(...values));
  handles.push(()=>consumer.close());
  assert.deepEqual(JSON.parse(JSON.stringify(await consumer.peekEvents())),produced.events);
  const queue=new bullmq.Queue(queueName,{connection:{url},prefix});handles.push(()=>queue.close());
  for(const id of produced.ids){const job=await queue.getJob(id);assert.equal(job.opts.attempts,3);assert.deepEqual(job.opts.backoff,{type:backoffType,delay:20});assert.equal(job.name,job.data.id);}
  await consumer.start();
  await waitFor(async()=>{const counts=await queue.getJobCounts('completed','failed');return counts.completed===2&&counts.failed===1;});
  assert.equal(attempts.get('first'),1);assert.equal(attempts.get('second'),2);assert.equal(attempts.get('third'),3);assert.equal(failures.length,4);
  await consumer.close();await queue.close();handles.splice(-2);

  const reverse=`ts-to-native-${backoffType}`,producer=new BullMqWorkerProducer(reverse,options(backoffType));handles.push(()=>producer.queue.close());
  await producer.enqueueJob(events[0]);await producer.enqueueBatchJobs(events.slice(1));
  const nativeResult=await native({mode:'consume',queue:reverse,expected:3,backoff:backoffType});
  assert.equal(nativeResult.completed,2);assert.equal(nativeResult.failed,1);
  assert.equal(nativeResult.failures.length,4);assert.ok(nativeResult.failures.every(failure=>failure.error==='worker_job_failed'));
  for(const [id,count] of [['first',1],['second',2],['third',3]])assert.equal(nativeResult.attempts.filter(attempt=>attempt.event.id===id).length,count);
  const jobs=await producer.queue.getJobs(['completed','failed']);for(const job of jobs)assert.equal(job.name,job.data.id);
  await producer.queue.close();handles.pop();
 }
 for(const nativeProduces of [false,true]){
  const queueName=`retention-${nativeProduces}`,events=[{id:'ok-1'},{id:'ok-2'},{id:'fail-1',failAttempts:10},{id:'fail-2',failAttempts:10}];
  const opts=options('fixed');opts.retries=1;opts.queueOptions.defaultJobOptions={removeOnComplete:1,removeOnFail:1};
  const producer=new BullMqWorkerProducer(queueName,opts);handles.push(()=>producer.queue.close());
  if(nativeProduces){
   const produced=await native({mode:'produce',queue:queueName,events,retries:1,keepCount:1});
   for(const id of produced.ids){const job=await producer.queue.getJob(id);assert.equal(job.opts.removeOnComplete,1);assert.equal(job.opts.removeOnFail,1);}
   let attempts=0;const consumer=new BullMqWorkerConsumer(queueName,opts,async values=>{attempts++;if(values[0].failAttempts)throw Error('retention_failure');},()=>{});handles.push(()=>consumer.close());await consumer.start();await waitFor(()=>attempts===4);await waitFor(async()=>await producer.queue.getCompletedCount()===1&&await producer.queue.getFailedCount()===1);await consumer.close();handles.pop();
  }else{
   await producer.enqueueBatchJobs(events);const result=await native({mode:'consume',queue:queueName,expected:4,expectedAttempts:4});assert.equal(result.failures.length,2);assert.equal(result.completed,1);assert.equal(result.failed,1);assert.equal(result.peek.length,4);
  }
  await producer.queue.close();handles.pop();
 }
 for(const shutdownAfterStart of [false,true]){
  const queueName=`removed-failure-${shutdownAfterStart}`,producer=new BullMqWorkerProducer(queueName,options('fixed'));handles.push(()=>producer.queue.close());
  await producer.enqueueJob({id:'removed',failAttempts:10,sleepMs:100});
  const result=await native({mode:'consume',queue:queueName,expected:shutdownAfterStart?1:3,removeOnFail:true,shutdownAfterStart});
  assert.equal(result.failures.length,shutdownAfterStart?1:3);assert.ok(result.failures.every(failure=>failure.event.id==='removed'));
  if(!shutdownAfterStart)assert.equal(await producer.queue.getFailedCount(),0);
  await producer.queue.close();handles.pop();
 }
 const queueName='waiting-active',producer=new BullMqWorkerProducer(queueName,options('fixed'));handles.push(()=>producer.queue.close());
 await producer.enqueueBatchJobs([{id:'active'},{id:'waiting'}]);
 let release;const blocked=new Promise(resolve=>release=resolve);
 const consumer=new BullMqWorkerConsumer(queueName,options('fixed'),async()=>blocked,()=>{});handles.push(async()=>{release();await consumer.close();});
 await consumer.start();await waitFor(async()=>await producer.queue.getActiveCount()===1);
 const peek=await native({mode:'peek',queue:queueName});assert.deepEqual(peek.events,JSON.parse(JSON.stringify(await consumer.peekEvents())));assert.equal(peek.events.length,2);
 release();await consumer.close();await producer.queue.close();handles.splice(-2);
 const duplicate='duplicate-event-ids';const duplicates=await native({mode:'produce',queue:duplicate,events:[{id:'same',order:1},{id:'same',order:2}]});assert.equal(new Set(duplicates.ids).size,2);assert.equal(duplicates.events.length,2);
 await assert.rejects(redisTransport(url).command('GET','outside-bullmq-fixture'));await assert.rejects(redisTransport(url).command('FLUSHDB'));
 assert.deepEqual(await native({mode:'consumeDenied',queue:'denied'},{REDIS_KEY_PREFIX:'foreign-bullmq-prefix:'}),{error:'worker_tenant_isolation_error'});
 console.log('PASS actual TS/native BullMQ wire interoperability: both producer/consumer directions, batches, fixed/exponential backoff, per-attempt failure callbacks, terminal failures, default/count-based retention, removed-job/drain failure callbacks, duplicate event IDs, waiting/active inspection and restricted ACL/consumer fail-stop');
}finally{
 for(const close of handles.reverse())await close();
 let cursor='0';do{const [next,keys]=await transport.command('SCAN',cursor,'MATCH',`${prefix}:*`,'COUNT',1000);if(keys.length)await transport.command('DEL',...keys);cursor=String(next);}while(cursor!=='0');
 await transport.command('ACL','DELUSER',username);
}
