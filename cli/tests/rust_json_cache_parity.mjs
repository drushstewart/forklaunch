import assert from 'node:assert/strict';
import {createRequire,Module} from 'node:module';
import {readFile,realpath} from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {randomUUID} from 'node:crypto';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
import {redisTransport} from './fixtures/redis_resp.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..'),require=createRequire(import.meta.url);
const tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
const ts=require(require.resolve('typescript',{paths:[path.dirname(await realpath(tsc))]}));
const compile=source=>ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText;
const encryptionSource=path.join(root,'framework/core/src/persistence/fieldEncryptor.ts');
const loaded=new Module(encryptionSource);loaded.filename=encryptionSource;loaded._compile(compile(await readFile(encryptionSource,'utf8')),encryptionSource);
const {FieldEncryptor}=loaded.exports;
const url=process.env.FORKLAUNCH_TEST_REDIS_URL??'redis://127.0.0.1:32770/12';
const transport=redisTransport(url),keys=[];
const client={
 set:(key,value,options)=>transport.command('SET',key,value,'PX',options.PX),
 lPush:(key,value)=>transport.command('LPUSH',key,value),
 rPop:key=>transport.command('RPOP',key),
 lRange:(key,start,end)=>transport.command('LRANGE',key,start,end),
 multi(){const commands=[];const multi={set(key,value,options){commands.push(['SET',key,value,'PX',options.PX]);return multi;},lPush(key,value){commands.push(['LPUSH',key,value]);return multi;},rPop(key){commands.push(['RPOP',key]);return multi;},get(key){commands.push(['GET',key]);return multi;},ttl(key){commands.push(['TTL',key]);return multi;},exec:()=>transport.transaction(commands)};return multi;},
};
const exports={};
vm.runInNewContext(compile(await readFile(path.join(root,'framework/infrastructure/redis/index.ts'),'utf8')),{exports,Buffer,require(name){
 if(name==='@forklaunch/common')return {safeParse:JSON.parse,safeStringify:JSON.stringify};
 if(name==='@forklaunch/core/http')return {evaluateTelemetryOptions:()=>({enabled:{logging:false}})};
 if(name==='redis')return {createClient:()=>client};
 throw new Error(`Unexpected runtime import ${name}`);
}});
const master='native-json-current-fixture-key',previous=['native-json-previous-fixture-key'],tenant='tenant-a';
const key=()=>{const value=`fl-json-parity:${randomUUID()}`;keys.push(value);return value;};
async function native(operations,options={}){
 const child=spawn('cargo',['run','--quiet','--manifest-path',path.join(root,'framework-rs/Cargo.toml'),'-p','forklaunch-infrastructure-redis','--features','redis-client,encryption','--example','json_cache_parity'],{cwd:root,env:{...process.env,FORKLAUNCH_TEST_REDIS_URL:url},stdio:['pipe','pipe','inherit']});let output='';child.stdout.on('data',chunk=>output+=chunk);child.stdin.end(JSON.stringify({master,previous,tenant,operations,...options}));
 assert.equal(await new Promise(resolve=>child.once('close',resolve)),0);return JSON.parse(output);
}
try{
 for(const format of ['v2','v3']){
  const actual=new exports.RedisTtlCache(60000,{}, {url},{},{encryptor:new FieldEncryptor(master,{previousKeys:previous,format})});
  const compliance={tenantId:tenant};
  for(const value of [{email:'private@example.test',unicode:'秘密🙂',nested:[null,false,0]},false,0,null,[], 'v2:ordinary-json-string']){
   const tsKey=key(),rustKey=key();
   await actual.putRecord({key:tsKey,value},compliance);
   assert.deepEqual(await native([{op:'get',key:tsKey}],{format}),[{found:true,value}]);
   assert.deepEqual(await native([{op:'put',key:rustKey,value}],{format}),[null]);
   const record=await actual.readRecord(rustKey,compliance);
   assert.deepEqual(record.value,value);assert.ok(record.ttlMilliseconds>0);
   const wire=await transport.command('GET',rustKey);assert.ok(wire.startsWith(`${format}:`));
   assert.deepEqual(JSON.parse(new FieldEncryptor(master,{previousKeys:previous}).decrypt(wire,tenant)),value);
   assert.ok((await native([{op:'get',key:rustKey}],{tenant:'wrong-tenant'}))[0].error);
   assert.equal((await native([{op:'get',key:rustKey}],{tenant:null}))[0].error,'record_tenant_required');
   await assert.rejects(actual.readRecord(rustKey,{tenantId:'wrong-tenant'}));
  }
  const queue=key();
  await actual.enqueueBatchRecords(queue,[{order:1},{order:2}],compliance);
  assert.deepEqual(await native([{op:'queuePeekBatch',count:10},{op:'queueGet'},{op:'queuePut',value:{order:3}}],{queue,format}),[[{order:1},{order:2}],{found:true,value:{order:1}},null]);
  assert.deepEqual(JSON.parse(JSON.stringify(await actual.peekQueueRecords(queue,10,compliance))),[{order:2},{order:3}]);
  assert.deepEqual(await actual.dequeueRecord(queue,compliance),{order:2});
  assert.ok((await native([{op:'queuePeek'}],{queue,tenant:'wrong'}))[0].error);
  assert.deepEqual(await native([{op:'queueGetBatch',count:10}],{queue,format}),[[{order:3}]]);
  await native([{op:'queuePutBatch',values:[{order:4},{order:5}]}],{queue,format});
  assert.deepEqual(JSON.parse(JSON.stringify(await actual.dequeueBatchRecords(queue,2,compliance))),[{order:4},{order:5}]);
  await native([{op:'queuePutBatch',values:[false,0,null]}],{queue,format});
  assert.deepEqual(await native([{op:'queuePeekBatch',count:10},{op:'queueGetBatch',count:10},{op:'queueGet'}],{queue,format}),[[false,0,null],[false,0,null],{found:false,value:null}]);
  const timed=key(),persistent=key();
  const [written,timedRecord]=await native([{op:'put',key:timed,value:false,ttl:1250},{op:'getWithTtl',key:timed}],{format});
  assert.equal(written,null);assert.equal(timedRecord.value,false);assert.ok(timedRecord.ttl>1000&&timedRecord.ttl<=1250);
  await transport.command('SET',persistent,JSON.stringify({persistent:true}));
  assert.deepEqual(await native([{op:'getWithTtl',key:persistent}],{format}),[{key:persistent,value:{persistent:true},ttl:null}]);

  const first=key(),second=key(),absent=key();
  await actual.putBatchRecords([{key:first,value:{batch:1},ttlMilliseconds:6000},{key:second,value:{batch:2},ttlMilliseconds:12000}],compliance);
  const [tsRecords]=await native([{op:'readRecords',keys:[second,absent,first]}],{format});
  assert.deepEqual(tsRecords.map(record=>record?.value??null),[{batch:2},null,{batch:1}]);
  assert.ok(tsRecords[0].ttl>tsRecords[2].ttl);assert.ok(tsRecords[0].ttl<=12000&&tsRecords[2].ttl<=6000);
  await native([{op:'putRecords',records:[{key:first,value:{native:1},ttl:6000},{key:second,value:{native:2},ttl:12000}]}],{format});
  const tsRead=await actual.readBatchRecords([second,first],compliance);
  assert.deepEqual(JSON.parse(JSON.stringify(tsRead.map(record=>record.value))),[{native:2},{native:1}]);
  assert.ok(tsRead[0].ttlMilliseconds>tsRead[1].ttlMilliseconds);
  const rejected=key();
  assert.equal((await native([{op:'putRecords',records:[{key:rejected,value:1,ttl:6000},{key:key(),value:2,ttl:0}]}]))[0].error,'redis_cache_invalid_ttl');
  assert.equal(await transport.command('GET',rejected),null);
  assert.equal((await native([{op:'putRecords',records:[{key:rejected,value:1},{key:' ',value:2}]}]))[0].error,'redis_cache_invalid_key');
  assert.equal(await transport.command('GET',rejected),null);
  assert.deepEqual(await native([{op:'putRecords',records:[]},{op:'readRecords',keys:[]}]),[null,[]]);
  const literal=key()+'[?]*';keys.push(literal);
  await actual.putRecord({key:literal,value:{literal:true}},compliance);
  const [matched]=await native([{op:'prefix',key:literal}],{format});assert.equal(matched.length,1);assert.equal(matched[0].key,literal);assert.deepEqual(matched[0].value,{literal:true});
  const rotated=key(),plain=key(),missing=key(),corrupt=key();
  const old=new exports.RedisTtlCache(60000,{}, {url},{},{encryptor:new FieldEncryptor(previous[0],{format})});
  await old.putRecord({key:rotated,value:{rotation:'old-key'}},compliance);
  await actual.putRecord({key:plain,value:{legacy:'plaintext'}});
  assert.deepEqual(await native([{op:'batch',keys:[rotated,missing,plain]}],{format}),[[{rotation:'old-key'},null,{legacy:'plaintext'}]]);
  await transport.command('SET',corrupt,'v2:bad:bad:bad','PX',60000);
  assert.ok((await native([{op:'get',key:corrupt}]))[0].error);
  const expires=key();await native([{op:'put',key:expires,value:{expires:true},ttl:1}]);
  await new Promise(resolve=>setTimeout(resolve,10));
  assert.deepEqual(await native([{op:'get',key:expires}]),[{found:false,value:null}]);
 }
 console.log('PASS actual TypeScript/native encrypted Redis cache/queue JSON: per-record batch/millisecond/persistent TTL metadata, literal prefix scans, prevalidated writes, FIFO/batches/peeks, v2/v3 both directions, rotation, plaintext migration, falsy values, missing/expired keys and wrong-tenant/corruption rejection');
}finally{for(const key of keys)await transport.command('DEL',key);}
