import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const dist=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/';
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(dist+'index.mjs'));
const rows=[];
for(const mode of ['anonymous','missing','verified','uppercase','authenticated','mismatch','authenticated-verified','reject','untrusted','invalid-session','expired-session']) {
 const ref=await betterAuthReference(root);let calls=0,background=0,context;
 const auth=betterAuth({...ref.config,emailVerification:{sendVerificationEmail:async value=>{calls++;context=value;if(mode==='reject')throw Error('owned');}},advanced:{...ref.config.advanced,backgroundTasks:{handler:()=>{background++;}}},logger:{disabled:true},rateLimit:{enabled:false}});
 const call=(route,body,cookie)=>auth.handler(new Request('http://localhost:3000/api/auth'+route,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',...(cookie?{cookie}:{})},body:JSON.stringify(body)}));
 const signup=await call('/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'verify@example.test',password:'owned-password-test'});assert.equal(signup.status,200);
 const cookie=signup.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ');
 if(mode.includes('verified'))ref.database.user[0].emailVerified=true;
 if(mode==='expired-session')ref.database.session[0].expiresAt=new Date(0);
 const email=mode==='missing'?'absent@example.test':mode==='mismatch'?'other@example.test':mode==='uppercase'?'VERIFY@EXAMPLE.TEST':'verify@example.test';
 const start=Date.now();const r=await call('/send-verification-email',{email,callbackURL:mode==='untrusted'?'https://evil.example/':'/verified'},['authenticated','mismatch','authenticated-verified','expired-session'].includes(mode)?cookie:mode==='invalid-session'?'better-auth.session_token=invalid':undefined);
 const body=await r.text();assert.equal(background,0);const expected={mismatch:400,'authenticated-verified':400,reject:500,untrusted:403}[mode]??200;assert.equal(r.status,expected);if(!['authenticated','mismatch','authenticated-verified','untrusted'].includes(mode))assert.ok(Date.now()-start>=490);assert.equal(calls,['missing','verified','mismatch','authenticated-verified','untrusted'].includes(mode)?0:1);rows.push({mode,status:r.status,body,calls,elapsed:Date.now()-start});
 if(context){const payload=JSON.parse(Buffer.from(context.token.split('.')[1],'base64url'));assert.equal(payload.email,'verify@example.test');assert.equal(payload.exp-payload.iat,3600);assert.equal(context.url,`http://localhost:3000/api/auth/verify-email?token=${context.token}&callbackURL=%2Fverified`);}
}
console.log('PASS email verification actual11',JSON.stringify(rows));
