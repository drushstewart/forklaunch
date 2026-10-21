#!/usr/bin/env node
// Verify a generated producer's static contract and execute its encryption path.
import {writeFile, readFile, rm} from 'node:fs/promises';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {resolve, join} from 'node:path';
const run = promisify(execFile);
const [worker, compiler] = process.argv.slice(2);
if (!worker || !compiler) throw Error('usage: rust_worker_events.mjs worker-dir tsc');
const dir = resolve(worker, 'events/typescript');
const source = join(dir, 'consumer-check.ts');
await writeFile(source, `import {WorkProducer, WorkConsumer, createEncryptedWorkPublisher, type WorkInput} from './events.js';
const good: WorkInput = {resourceId:'33333333-3333-4333-8333-333333333333',revision:1,message:'private marker'};
// @ts-expect-error revision is numeric
const bad: WorkInput = {...good,revision:'wrong'};
// @ts-expect-error message is required
const missing: WorkInput = {resourceId:good.resourceId,revision:1};
let captured: unknown;
const publisher = createEncryptedWorkPublisher({namespace:'tasks',tenantId:'11111111-1111-4111-8111-111111111111',keyId:'primary',key:new Uint8Array(32).fill(7),async publish(event) {
 if (JSON.stringify(event).includes('private marker') || !event.ciphertext.startsWith('v3.primary.')) throw Error('unencrypted event');
 captured = event; console.log(JSON.stringify(event));
}});
await new WorkProducer(publisher, 'tasks').publish('22222222-2222-4222-8222-222222222222', good);
const expected={tenantId:'11111111-1111-4111-8111-111111111111',resourceId:good.resourceId,operationId:'22222222-2222-4222-8222-222222222222',revision:1};
let audits=0;
const options={namespace:'tasks',tenantId:expected.tenantId,activeKey:{id:'active',key:new Uint8Array(32).fill(8)},previousKeys:[{id:'primary',key:new Uint8Array(32).fill(7)}],actor:'fixture',audit:async(record:unknown):Promise<true>=>{if(JSON.stringify(record).includes(good.message))throw Error('audit payload leak');audits++;return true;}};
const consumer = new WorkConsumer(options,'tasks');
const opened=await consumer.consume(expected,captured);
if(opened.input.message !== 'private marker' || audits !== 1) throw Error('typed audited consume failed');
const reject=async(work:()=>Promise<unknown>)=>{let failed=false;try{await work();}catch{failed=true;}if(!failed)throw Error('expected rejection');};
await reject(()=>consumer.consume({...expected,revision:2},captured));
await reject(()=>new WorkConsumer(options,'other').consume(expected,captured));
await reject(()=>new WorkConsumer({...options,audit:async()=>{throw Error('unavailable');}},'tasks').consume(expected,captured));
await reject(()=>new WorkConsumer({...options,auditTimeoutMs:5,audit:async()=>new Promise<true>(()=>{})},'tasks').consume(expected,captured));
const malicious={...good};Object.defineProperty(malicious,'toJSON',{value:()=>({...good,message:42})});
await reject(()=>new WorkProducer(publisher,'tasks').publish(expected.operationId,malicious));
const moved={...good};Object.defineProperty(moved,'toJSON',{value:()=>({...good,revision:2})});
await reject(()=>new WorkProducer(publisher,'tasks').publish(expected.operationId,moved));
// Bypass the typed producer to exercise authenticated but wrong-shape input rejection before audit.
await publisher.sealAndPublish({channel:'work-v1',queue:'tasks',operationId:expected.operationId,input:{resourceId:good.resourceId,revision:1}});
const before=audits;
await reject(()=>consumer.consume(expected,captured));
if(audits !== before)throw Error('wrong-schema event reached audit');
consumer.close();
publisher.close();
`);
try {
 await run('npm', ['install','--prefix',dir,'--ignore-scripts','--no-audit','--no-fund']);
 await run(resolve(compiler), ['--strict','--skipLibCheck','--target','es2022','--module','es2022','--moduleResolution','bundler', source]);
 const {stdout} = await run('node', [join(dir,'consumer-check.js')]);
 const sealed = JSON.parse(stdout.trim().split('\n')[0]);
 if (!sealed.ciphertext || sealed.channel !== 'work-v1') throw Error('missing encrypted output');
 const artifact = JSON.parse(await readFile(resolve(worker,'events/event-contract.json'),'utf8'));
 if (!artifact.input.properties.message || artifact.format !== 'forklaunch.worker-events.v1' || !artifact.consumerAudit) throw Error('schema source missing');
 console.log('PASS generated worker schema, TypeScript typing, exact serialized producer, rotation, trusted context and audited consumer');
} finally {
 await Promise.all(['consumer-check.ts','consumer-check.js','events.js'].map(file => rm(join(dir,file),{force:true})));
}
