import assert from 'node:assert/strict';import{pathToFileURL}from'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));const {betterAuth}=await import(pathToFileURL(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/index.mjs'));
const rows=[];
for(const [autoSignIn,requireEmailVerification,sendOnSignUp] of [[false,false,undefined],[false,false,true],[false,true,undefined],[true,true,false],[true,false,undefined]]) {
 const ref=await betterAuthReference(root);let sends=0;
 const auth=betterAuth({...ref.config,emailAndPassword:{...ref.config.emailAndPassword,autoSignIn,requireEmailVerification},emailVerification:{sendVerificationEmail:async()=>{sends++},...(sendOnSignUp===undefined?{}:{sendOnSignUp})},logger:{disabled:true},rateLimit:{enabled:false}});
 const call=(route,body)=>auth.handler(new Request('http://localhost:3000/api/auth'+route,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify(body)}));
 const input={name:'Original',firstName:'First',lastName:'Last',email:'auto@example.test',password:'owned-signup-password'};
 const first=await call('/sign-up/email',input);const created=await first.json();const count=ref.database.session.length;const before=sends;
 const second=await call('/sign-up/email',{...input,name:'Submitted',firstName:'Other'});const duplicate=await second.json();
 const login=await call('/sign-in/email',input);const skipped=!autoSignIn||requireEmailVerification;
 assert.equal(created.token===null,skipped);assert.equal(count,skipped?0:1);assert.equal(second.status,skipped?200:422);
 if(skipped){assert.equal(duplicate.token,null);assert.notEqual(duplicate.user.id,created.user.id);assert.equal(duplicate.user.name,'Submitted');assert.equal(duplicate.user.firstName,'Other');assert.equal(duplicate.user.image,null);assert.equal(duplicate.user.phoneNumber,null);assert.equal(second.headers.has('set-cookie'),false);}
 assert.equal(sends,before);assert.equal(login.status,requireEmailVerification?403:200);
 rows.push({autoSignIn,requireEmailVerification,sendOnSignUp:sendOnSignUp??null,signup:first.status,duplicate:second.status,login:login.status,sends,sessionsAfterSignup:count,cookie:first.headers.has('set-cookie')});
}
console.log('PASS autoSignIn actual5',JSON.stringify(rows));
