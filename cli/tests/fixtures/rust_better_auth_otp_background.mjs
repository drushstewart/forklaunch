import assert from 'node:assert/strict';import {pathToFileURL,fileURLToPath} from 'node:url';import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..'),dist=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/';
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(dist+'index.mjs'));const {twoFactor}=await import(pathToFileURL(dist+'plugins/two-factor/index.mjs'));
const observations=[];const originalError=console.error;console.error=()=>{};
for(const mode of ['await','capture','handler-throw','prepare-throw','delivery-reject']) {
 const ref=await betterAuthReference(root);ref.database.twoFactor=[];let release,entered;const ready=new Promise(r=>entered=r),pending=new Promise(r=>release=r);let callbackCalls=0,handlerCalls=0;const tasks=[];
 const sendOTP=()=>{callbackCalls++;entered();if(mode==='prepare-throw')throw Error('owned-preparation');if(mode==='delivery-reject')return Promise.reject(Error('owned-delivery'));return pending;};
 const handler=promise=>{handlerCalls++;tasks.push(promise);if(mode==='handler-throw')throw Error('owned-handler');};
 const auth=betterAuth({...ref.config,advanced:{...ref.config.advanced,...(mode==='await'?{}:{backgroundTasks:{handler}})},plugins:[...ref.config.plugins,twoFactor({otpOptions:{sendOTP}})],logger:{disabled:true},rateLimit:{enabled:false}});
 const signup=await auth.handler(new Request('http://localhost:3000/api/auth/sign-up/email',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify({name:'Owned',firstName:'Owned',lastName:'User',email:'background@example.test',password:'owned-background-password'})}));assert.equal(signup.status,200);
 const cookie=signup.headers.getSetCookie().map(s=>s.split(';')[0]).join('; ');
 let done=false;const response=auth.handler(new Request('http://localhost:3000/api/auth/two-factor/send-otp',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},body:'{}'})).then(r=>{done=true;return r});
 await ready;await new Promise(r=>setTimeout(r,30));assert.equal(done,mode!=='await');assert.ok(ref.database.verification.length>0);
 release();const result=await response;assert.equal(result.status,mode==='prepare-throw'?500:200);await Promise.all(tasks);
 assert.equal(handlerCalls,['await','prepare-throw'].includes(mode)?0:1);assert.equal(callbackCalls,1);
 observations.push({mode,returnedBeforeDelivery:mode!=='await',status:result.status,handlerCalls,storedBeforeDelivery:true});
}
console.error=originalError;console.log('PASS backgroundOTP',JSON.stringify(observations));
