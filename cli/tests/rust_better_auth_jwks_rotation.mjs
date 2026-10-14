import assert from 'node:assert/strict';
import path from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const dependencies=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;assert.ok(dependencies);
const load=relative=>import(pathToFileURL(path.join(dependencies,'node_modules',relative)));
const {signJWT}=await load('better-auth/dist/plugins/jwt/sign.mjs');
const {betterAuth}=await load('better-auth/dist/index.mjs');
const {decodeProtectedHeader}=await load('jose/dist/webapi/index.js');
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const RealDate=Date;let clock=1800000000000;
globalThis.Date=class extends RealDate{constructor(...args){super(...(args.length?args:[clock]));}static now(){return clock;}};
try {
 for(const interval of [undefined,0,0.001,2]) {
  clock=1800000000000;const rows=[];let created=0;
  const options={jwks:{disablePrivateKeyEncryption:true,...(interval===undefined?{}:{rotationInterval:interval})},adapter:{getJwks:async()=>rows,createJwk:async record=>{const row={...record,id:`key-${++created}`};rows.push(row);return row;}}};
  const context={context:{options:{baseURL:'http://localhost:3000'},adapter:{}}};
  const first=await signJWT(context,{options,payload:{sub:'owner'}});assert.equal(decodeProtectedHeader(first).kid,'key-1');
  if(!interval){assert.equal(rows[0].expiresAt,undefined);clock+=100000;await signJWT(context,{options,payload:{sub:'owner'}});assert.equal(created,1);continue;}
  assert.equal(rows[0].expiresAt.getTime(),clock+interval*1000);
  clock=rows[0].expiresAt.getTime();
  await signJWT(context,{options,payload:{sub:'owner'},signingKeyId:'key-1'});assert.equal(created,1);
  const second=await signJWT(context,{options,payload:{sub:'owner'}});assert.equal(decodeProtectedHeader(second).kid,'key-2');assert.equal(created,2);
  clock++;await assert.rejects(signJWT(context,{options,payload:{sub:'owner'},signingKeyId:'key-1'}));assert.equal(created,2);
  const reference=await betterAuthReference(root,{jwt:{...options,jwks:{...options.jwks,gracePeriod:0.002}}});
  const auth=betterAuth({...reference.config,logger:{disabled:true}});
  const keys=async()=>{const response=await auth.handler(new Request('http://localhost:3000/api/auth/jwks'));assert.equal(response.status,200);return (await response.json()).keys.map(key=>key.kid);};
  clock=rows[0].expiresAt.getTime()+1;assert.ok((await keys()).includes('key-1'));
  clock++;assert.ok(!(await keys()).includes('key-1'));assert.equal(created,2);
 }
}finally{globalThis.Date=RealDate;}
console.log('PASS actual rotation: absent/zero disabled; 1ms/2s expiry; unpinned replacement at equality; pinned equality accepted then expired rejection; strict 2ms publication grace');
for(const override of [false,true]){
 let created;
 const reference=await betterAuthReference(root,{jwt:{jwks:{rotationInterval:2},adapter:{getJwks:async()=>[],createJwk:async row=>(created={...row,id:'configured'})}}});
 const auth=betterAuth({...reference.config,logger:{disabled:true}});
 await auth.api.signJWT({body:{payload:{sub:'owner'},...(override?{overrideOptions:{jwks:{}}}:{})}});
 assert.equal(created.expiresAt instanceof Date,!override);
}
console.log('PASS actual signJWT empty jwks override disables configured rotation through shallow merge');
