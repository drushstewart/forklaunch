import assert from 'node:assert/strict';import {pathToFileURL,fileURLToPath} from 'node:url';import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..'),dist=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/';
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));const {betterAuth}=await import(pathToFileURL(dist+'index.mjs'));
const originalError=console.error;console.error=()=>{};const observations=[];
for(const mode of ['await','capture','handler-throw','prepare-throw','delivery-reject','missing','social','untrusted','uppercase']) {
 const ref=await betterAuthReference(root);let release,entered;const ready=new Promise(r=>entered=r),pending=new Promise(r=>release=r);let callbackCalls=0,handlerCalls=0,context;const tasks=[];
 const sendResetPassword=input=>{context=input;callbackCalls++;entered();if(mode==='prepare-throw')throw Error('owned-preparation');if(mode==='delivery-reject')return Promise.reject(Error('owned-delivery'));return pending;};
 const handler=promise=>{handlerCalls++;tasks.push(promise);if(mode==='handler-throw')throw Error('owned-handler');};
 const auth=betterAuth({...ref.config,emailAndPassword:{...ref.config.emailAndPassword,sendResetPassword},advanced:{...ref.config.advanced,...(mode==='await'?{}:{backgroundTasks:{handler}})},logger:{disabled:true},rateLimit:{enabled:false}});
 const call=(route,body)=>auth.handler(new Request('http://localhost:3000/api/auth'+route,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify(body)}));
 assert.equal((await call('/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'reset@example.test',password:'owned-reset-password'})).status,200);
 if(mode==='social')ref.database.account=[];
 let done=false;const response=call('/request-password-reset',{email:mode==='missing'?'missing@example.test':mode==='uppercase'?'RESET@EXAMPLE.TEST':'reset@example.test',redirectTo:mode==='untrusted'?'https://evil.example/':'/owned-reset'}).then(r=>{done=true;return r});
 if(!['missing','untrusted'].includes(mode)){await ready;await new Promise(r=>setTimeout(r,20));assert.equal(done,mode!=='await');assert.ok(ref.database.verification.some(r=>r.identifier==='reset-password:'+context.token));assert.match(context.token,/^[a-zA-Z0-9]{24}$/);assert.equal(context.url,`http://localhost:3000/api/auth/reset-password/${context.token}?callbackURL=%2Fowned-reset`);assert.ok(!('password' in context.user));release();}
 const result=await response;const expected=mode==='prepare-throw'?500:mode==='untrusted'?403:200;assert.equal(result.status,expected);await Promise.all(tasks);if(['missing','untrusted'].includes(mode)){assert.equal(callbackCalls,0);assert.equal(ref.database.verification.length,0);}observations.push({mode,status:result.status,callbackCalls,handlerCalls,stored:ref.database.verification.length});
}
console.error=originalError;console.log('PASS reset delivery actual9',JSON.stringify(observations));
