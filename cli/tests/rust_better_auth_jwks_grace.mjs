// Actual configured plugin publication semantics; negative/nonfinite options are
// characterized here but intentionally outside the native typed Duration API.
import assert from 'node:assert/strict';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const now=1800000000000;
const original=Date.now;
for(const seconds of [undefined,0,0.001,1,-1,Infinity,NaN]) {
 const {auth,database}=await betterAuthReference(root,{jwt:{jwks:seconds===undefined?{}:{gracePeriod:seconds}}});
 const request=()=>auth.handler(new Request('http://localhost:3000/api/auth/jwks'));
 assert.equal((await request()).status,200);
 const source=database.jwks[0];
 database.jwks.splice(0,database.jwks.length,...[null,-2592000001,-2592000000,-1001,-1000,-1,0,1,1000,1001].map((offset,index)=>({...source,id:`key-${index}`,expiresAt:offset===null?null:new Date(now+offset)})));
 try {
  Date.now=()=>now;
  const response=await request();assert.equal(response.status,200);
  const ids=(await response.json()).keys.map(k=>k.kid);
  const grace=(seconds??2592000)*1000;
  const expected=database.jwks.filter(k=>!k.expiresAt||k.expiresAt.getTime()+grace>now).map(k=>k.id);
  assert.deepEqual(ids,expected);
  console.log(JSON.stringify({seconds:String(seconds),ids}));
 } finally {Date.now=original;}
}

console.log('Better Auth JWKS grace: 70 key visibility comparisons PASS.');
