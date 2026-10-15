import assert from 'node:assert/strict';import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,''),deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/index.mjs'));
const {hashPassword,verifyPassword}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/crypto/password.mjs'));
const RealDate=Date,start=1800000000000;let now=start;
globalThis.Date=class extends RealDate{constructor(...a){super(...(a.length?a:[now]))}static now(){return now}};
const rows=[];
for(const mode of ['hash','verify','signup-delivery']){
 now=start;const ref=await betterAuthReference(root);
 const auth=betterAuth({...ref.config,session:{expiresIn:120},rateLimit:{enabled:false},logger:{disabled:true},emailAndPassword:{...ref.config.emailAndPassword,password:{hash:async(p)=>{const out=await hashPassword(p);if(mode==='hash')now+=7000;return out},verify:async(p)=>{const out=await verifyPassword(p);if(mode==='verify')now+=9000;return out}}},emailVerification:mode==='signup-delivery'?{sendOnSignUp:true,sendVerificationEmail:async()=>{await Promise.resolve();now+=11000}}:undefined});
 const call=async(path,body)=>auth.handler(new Request('http://localhost:3000/api/auth'+path,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify(body)}));
 const signup=await call('/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'clock@example.test',password:'owned-password'});assert.equal(signup.status,200);let body=await signup.json();
 if(mode==='verify'){const signin=await call('/sign-in/email',{email:'clock@example.test',password:'owned-password'});assert.equal(signin.status,200);body=await signin.json();}
 const session=ref.database.session.find(s=>s.token===body.token);assert.equal(session.createdAt.getTime(),now);assert.equal(session.expiresAt.getTime(),now+120000);rows.push({mode,createdOffset:now-start,ttl:120});
}
globalThis.Date=RealDate;console.log('PASS actual post-await clock3',JSON.stringify(rows));
