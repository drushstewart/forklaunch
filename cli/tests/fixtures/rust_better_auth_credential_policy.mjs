import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const {betterAuth}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/index.mjs'));
const RealDate=Date,now=1800000000000;
globalThis.Date=class extends RealDate{constructor(...a){super(...(a.length?a:[now]))}static now(){return now}};
const rows=[];
for(const [label,options,password,status,code]of [
 ['disabled',{disableSignUp:true},'owned-password',400,'EMAIL_PASSWORD_SIGN_UP_DISABLED'],
 ['disabled-empty',{disableSignUp:true},'',400,'VALIDATION_ERROR'],
 ['short',{minPasswordLength:10},'123456789',400,'PASSWORD_TOO_SHORT'],
 ['long',{maxPasswordLength:9},'1234567890',400,'PASSWORD_TOO_LONG'],
 ['utf16-short',{minPasswordLength:5},'😀😀',400,'PASSWORD_TOO_SHORT'],
 ['utf16-equal',{minPasswordLength:4,maxPasswordLength:4},'😀😀',200,null],
 ['zero-min',{minPasswordLength:0},'1234567',400,'PASSWORD_TOO_SHORT'],
 ['zero-max',{maxPasswordLength:0},'x'.repeat(129),400,'PASSWORD_TOO_LONG'],
 ['inverted-min',{minPasswordLength:10,maxPasswordLength:5},'1234567',400,'PASSWORD_TOO_SHORT'],
 ['inverted-max',{minPasswordLength:10,maxPasswordLength:5},'1234567890',400,'PASSWORD_TOO_LONG']]){
 const ref=await betterAuthReference(root);const auth=betterAuth({...ref.config,emailAndPassword:{...ref.config.emailAndPassword,...options},rateLimit:{enabled:false},logger:{disabled:true}});
 const response=await auth.handler(new Request('http://localhost:3000/api/auth/sign-up/email',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify({name:'Owned',firstName:'Owned',lastName:'User',email:'policy@example.test',password})}));const body=await response.json();assert.equal(response.status,status,label);if(code)assert.equal(body.code,code,label);assert.equal(ref.database.user.length,status===200?1:0);rows.push({label,status,code:body.code??null});
}
for(const ttl of [0,1,120]){
 const ref=await betterAuthReference(root);let verification;
 const auth=betterAuth({...ref.config,emailAndPassword:{...ref.config.emailAndPassword,resetPasswordTokenExpiresIn:ttl,sendResetPassword:async()=>{}},emailVerification:{expiresIn:ttl,sendVerificationEmail:async(x)=>{verification=x.token}},rateLimit:{enabled:false},logger:{disabled:true}});
 const call=async(route,body,cookie='')=>auth.handler(new Request('http://localhost:3000/api/auth'+route,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},body:JSON.stringify(body)}));
 const signup=await call('/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'ttl@example.test',password:'owned-password'});assert.equal(signup.status,200);const cookie=signup.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ');
 const reset=await call('/request-password-reset',{email:'ttl@example.test'});assert.equal(reset.status,200);const record=ref.database.verification.find(x=>x.identifier.startsWith('reset-password:'));assert.equal(record.expiresAt.getTime()-now,(ttl||3600)*1000);
 const email=await call('/send-verification-email',{email:'ttl@example.test'},cookie);assert.equal(email.status,200);const claims=JSON.parse(Buffer.from(verification.split('.')[1],'base64url'));assert.equal(claims.exp-claims.iat,ttl);rows.push({label:'ttl-'+ttl,reset:ttl||3600,verification:ttl});
}
{
 const ref=await betterAuthReference(root);
 const base={...ref.config,rateLimit:{enabled:false},logger:{disabled:true}};
 const first=betterAuth({...base,emailAndPassword:{...ref.config.emailAndPassword,minPasswordLength:4,maxPasswordLength:4}});
 const call=async(auth,path,body,cookie='')=>auth.handler(new Request('http://localhost:3000/api/auth'+path,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},body:JSON.stringify(body)}));
 const created=await call(first,'/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'existing@example.test',password:'abcd'});assert.equal(created.status,200);
 const cookie=created.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ');
 const changed=betterAuth({...base,emailAndPassword:{...ref.config.emailAndPassword,minPasswordLength:10,maxPasswordLength:4}});
 for(const [route,body,status,code]of [
  ['/sign-in/email',{email:'existing@example.test',password:'abcd'},200,null],
  ['/sign-in/email',{email:'existing@example.test',password:'abcde'},400,'PASSWORD_TOO_LONG'],
  ['/verify-password',{password:'abcd'},200,null],
  ['/change-password',{currentPassword:'abcde',newPassword:'abcd'},400,'PASSWORD_TOO_SHORT']
 ]) {const response=await call(changed,route,body,cookie);const result=await response.json();assert.equal(response.status,status,route);if(code)assert.equal(result.code,code,route);rows.push({label:'existing-'+route,status,code:result.code??null});}
}
globalThis.Date=RealDate;console.log('PASS credential policy audit'+rows.length,JSON.stringify(rows));
