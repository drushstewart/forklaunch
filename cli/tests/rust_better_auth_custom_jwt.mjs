// Actual Better Auth server-only helper with an explicitly supplied signing callback.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const reference=await betterAuthReference(root), RealDate=Date, now=1800000000;
globalThis.Date=class extends RealDate {
  constructor(...args){super(...(args.length?args:[now*1000]));}
  static [Symbol.hasInstance](value){return value instanceof RealDate;}
  static now(){return now*1000;}
};
const cases=[undefined,null,{}, {payload:null},{payload:[]},{payload:{},overrideOptions:null}];
for(const payload of [{},{sub:42,iat:null,nbf:null,jti:[]},{iat:0},{iat:false},{iat:'100'},
  {exp:'2h'},{exp:0},{exp:false},{exp:{}},{iss:5,aud:[1,null]},
  {iss:null,aud:null},{iat:now,nbf:'3m'},{iat:[],sub:false},JSON.parse('{"__proto__":{"x":1},"a":1}')])
for(const jwt of [{},{issuer:'custom',audience:['a','b'],expirationTime:'2h'},
  {issuer:42,audience:false,expirationTime:123},{expirationTime:'bad'}])
  cases.push({payload,overrideOptions:{jwt}});
const inputs=[],expected=[];
try{
 for(const body of cases){
  let wire=body===undefined?undefined:structuredClone(body);
  if(wire&&typeof wire==='object'&&!Array.isArray(wire)&&
      (wire.overrideOptions===undefined||(wire.overrideOptions&&typeof wire.overrideOptions==='object'&&!Array.isArray(wire.overrideOptions)))){
    wire.overrideOptions={...wire.overrideOptions,jwt:{...wire.overrideOptions?.jwt,sign:async payload=>JSON.stringify(payload)}};
  }
  let result;
  try{const response=await reference.auth.api.signJWT({body:wire});result={payload:JSON.parse(response.token)};}
  catch(error){result={error:{...(error.body?.code?{code:error.body.code}:{}),message:error.body?.message??error.message}};}
  expected.push(result);inputs.push({op:'custom-sign',now,...(body===undefined?{}:{body})});
  assert.equal(reference.database.jwks.length,0,'Custom signing must not provision local keys');
  inputs.push({op:'records',now});
 }
}finally{globalThis.Date=RealDate;}
const native=spawnSync('cargo',['run','--quiet','--manifest-path',path.join(root,'blueprint-rs/Cargo.toml'),
 '-p','forklaunch-iam-better-auth','--example','jwt_helper_parity'],{
 input:inputs.map(v=>JSON.stringify(v)).join('\n')+'\n',encoding:'utf8',maxBuffer:16*1024*1024,
 env:{...process.env,CARGO_INCREMENTAL:'0'},timeout:180000});
assert.equal(native.status,0,native.stderr);
const results=native.stdout.trim().split('\n').map(line=>JSON.parse(line));
assert.equal(results.length,inputs.length);
for(let index=0;index<cases.length;index++){
 const response=results[index*2];
 const actual=response.body?{payload:JSON.parse(response.body.token)}:response;
 assert.deepEqual(actual,expected[index],JSON.stringify(cases[index]));
 assert.deepEqual(results[index*2+1],[],'Native custom signer must not provision local keys');
}
console.log(`PASS ${cases.length} actual Better Auth custom JWT callback cases: defaults, raw claims, errors, null/absence and zero local-key side effects`);
