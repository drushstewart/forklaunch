// Compare configured HTTP session tokens with an owned external signing callback.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
let calls=0;
const reference=await betterAuthReference(root,{jwt:{
  jwks:{remoteUrl:'https://keys.fixture.test/jwks',keyPairConfig:{alg:'EdDSA'}},
  jwt:{sign:async payload=>{calls++;return JSON.stringify(payload);}},
}});
const RealDate=Date, now=1800000000000;
globalThis.Date=class extends RealDate {
  constructor(...args){super(...(args.length?args:[now]));}
  static [Symbol.hasInstance](value){return value instanceof RealDate;}
  static now(){return now;}
};
const inputs=[],expected=[];
try{
 const owner=reference.client();
 await owner.call('/sign-up/email',{name:'External User',firstName:'External',lastName:'User',email:'external@fixture.test',password:'fixture-only-password'});
 inputs.push({op:'seed',now,users:reference.database.user,accounts:reference.database.account,sessions:reference.database.session});
 const signed=[...owner.cookies].map(([name,value])=>`${name}=${value}`).join('; ');
 calls=0;
 for(const cookie of ['', 'better-auth.session_token=forged', signed]){
  for(const route of ['/jwks','/token','/get-session']){
   const response=await reference.auth.handler(new Request(`http://localhost:3000/api/auth${route}`,{headers:{cookie}}));
   const text=await response.text();let body=text;try{body=JSON.parse(text);}catch{}
   expected.push({status:response.status,body,jwt:response.headers.get('set-auth-jwt')});
   inputs.push({op:'request',now,path:route,method:'GET',headers:{cookie}});
  }
 }
 assert.equal(calls,2,'Only authenticated token/session routes invoke the signer');
 assert.equal(reference.database.jwks.length,0,'External signing must not create local keys');
}finally{globalThis.Date=RealDate;}
const native=spawnSync('cargo',['run','--quiet','--manifest-path',path.join(root,'blueprint-rs/Cargo.toml'),'-p','forklaunch-iam-better-auth','--features','http','--example','http_parity'],{
 input:inputs.map(value=>JSON.stringify(value)).join('\n')+'\n',encoding:'utf8',maxBuffer:16*1024*1024,timeout:180000,
 env:{...process.env,CARGO_INCREMENTAL:'0',FORKLAUNCH_AUTH_EXTERNAL_JWT:'1'},
});
assert.equal(native.status,0,native.stderr);
const results=native.stdout.trim().split('\n').map(line=>JSON.parse(line));
assert.equal(results.length,inputs.length);
for(let index=0;index<expected.length;index++){
 const result=results[index+1],actual=expected[index];
 assert.equal(result.status,actual.status,JSON.stringify(inputs[index+1]));
 if(inputs[index+1].path==='/token'&&actual.status===200){
  assert.deepEqual(JSON.parse(result.body.token),JSON.parse(actual.body.token));
 }else assert.deepEqual(result.body,actual.body);
 const token=result.headers['set-auth-jwt']?.[0]??null;
 assert.deepEqual(token?JSON.parse(token):null,actual.jwt?JSON.parse(actual.jwt):null);
}
console.log(`PASS ${expected.length} actual Better Auth external JWT HTTP cases: authenticated claims/header, forged/absent sessions, remote JWKS 404 and zero reference key writes`);
