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
 multi(){const commands=[];const multi={get(key){commands.push(['GET',key]);return multi;},ttl(key){commands.push(['TTL',key]);return multi;},exec:()=>transport.transaction(commands)};return multi;},
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
 console.log('PASS actual TypeScript/native encrypted Redis JSON: v2/v3 both directions, rotation, plaintext migration, falsy values, missing/expired keys and wrong-tenant/corruption rejection');
}finally{for(const key of keys)await transport.command('DEL',key);}
