import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/index.mjs'));
const RealDate=Date,start=1800000000000;let clock=start;
globalThis.Date=class extends RealDate{constructor(...args){super(...(args.length?args:[clock]))}static now(){return clock}};
const rows=[];
for(const mode of ['normal','transient','before','equal','after','zero-update','disabled','explicit-disabled','expiry-equal','expiry-after','zero-expiry','update-after-expiry','fresh-before','fresh-equal','fresh-after','fresh-zero']){
 clock=start;const ref=await betterAuthReference(root);
 const policy={expiresIn:mode==='zero-expiry'?0:120,updateAge:mode==='zero-update'?0:mode==='update-after-expiry'?121:30,freshAge:mode==='fresh-zero'?0:60,disableSessionRefresh:mode==='disabled'};
 const auth=betterAuth({...ref.config,session:policy,rateLimit:{enabled:false},logger:{disabled:true}});
 const response=await auth.handler(new Request('http://localhost:3000/api/auth/sign-up/email',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify({name:'Owned',firstName:'Owned',lastName:'User',email:'policy@example.test',password:'owned-password',rememberMe:mode!=='transient'})}));
 assert.equal(response.status,200);const first=await response.json();const original=ref.database.session[0].expiresAt.getTime();
 const cookie=response.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ');
 clock+=mode==='before'?29999:mode==='equal'?30000:['after','disabled','explicit-disabled','transient'].includes(mode)?30001:mode==='expiry-equal'?120000:mode==='expiry-after'?120001:mode==='update-after-expiry'?120000:mode==='fresh-before'?59999:mode==='fresh-equal'?60000:mode.startsWith('fresh-')?60001:0;
 const get=await auth.handler(new Request('http://localhost:3000/api/auth/'+(mode.startsWith('fresh-')?'list-sessions':'get-session')+(mode==='explicit-disabled'?'?disableRefresh=true':''),{headers:{cookie}}));
 const body=await get.json();const session=ref.database.session.find(x=>x.token===first.token);
 assert.equal(original-start,mode==='transient'?86400000:mode==='zero-expiry'?604800000:120000);
 assert.equal(get.status,['fresh-equal','fresh-after'].includes(mode)?403:200);
 if(!mode.startsWith('fresh-'))assert.equal(body===null,mode==='expiry-after');
 if(['fresh-equal','fresh-after'].includes(mode))assert.equal(body.code,'SESSION_NOT_FRESH');
 const refreshed=['equal','after','zero-update','expiry-equal'].includes(mode)||mode.startsWith('fresh-');
 if(session&&!mode.startsWith('fresh-'))assert.equal(session.expiresAt.getTime(),refreshed?clock+120000:original);
 if(!mode.startsWith('fresh-'))assert.equal(get.headers.getSetCookie().length,mode==='expiry-after'?3:refreshed?1:0);
 const firstCookies=response.headers.getSetCookie().join(';');
 assert.equal(firstCookies.includes('Max-Age='),mode!=='transient');
 rows.push({mode,initialSeconds:(original-start)/1000,context:(await auth.$context).sessionConfig,status:get.status,bodyNull:body===null,refresh:session?.expiresAt.getTime()!==original,currentExpiry:session?.expiresAt.getTime(),cookie:response.headers.getSetCookie().map(x=>x.match(/Max-Age=([^;]+)/)?.[1]).filter(Boolean),getCookies:get.headers.getSetCookie().length});
}
const {twoFactor}=await import(pathToFileURL(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/plugins/two-factor/index.mjs'));
const {createEmailVerificationToken}=await import(pathToFileURL(process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/api/routes/email-verification.mjs'));
clock=start;
const ref=await betterAuthReference(root);
ref.database.twoFactor=[];
let deliveredOtp;
const auth=betterAuth({...ref.config,session:{expiresIn:120,updateAge:30,freshAge:60},emailVerification:{autoSignInAfterVerification:true},plugins:[...ref.config.plugins,twoFactor({skipVerificationOnEnable:true,otpOptions:{sendOTP:async({otp})=>{deliveredOtp=otp;}}})],rateLimit:{enabled:false},logger:{disabled:true}});
const call=async(route,body,cookie='')=>{const response=await auth.handler(new Request('http://localhost:3000/api/auth'+route,{method:body?'POST':'GET',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},...(body?{body:JSON.stringify(body)}:{})}));return {response,body:await response.json(),cookie:response.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ')};};
const signup=await call('/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'creation-policy@example.test',password:'owned-password'});assert.equal(signup.response.status,200);
clock+=20000;
const enabled=await call('/two-factor/enable',{password:'owned-password'},signup.cookie);assert.equal(enabled.response.status,200);
let active=ref.database.session[0];assert.equal(active.expiresAt.getTime(),clock+120000);
for(const [index,rememberMe]of [false,true].entries()){
 const signin=await call('/sign-in/email',{email:'creation-policy@example.test',password:'owned-password',rememberMe});assert.equal(signin.body.twoFactorRedirect,true);
 const verified=await call('/two-factor/verify-backup-code',{code:enabled.body.backupCodes[index]},signin.cookie);assert.equal(verified.response.status,200);
 active=ref.database.session.find(x=>x.token===verified.body.token);assert.equal(active.expiresAt.getTime(),clock+(rememberMe?120000:86400000));
 rows.push({mode:'mfa-'+rememberMe,seconds:(active.expiresAt.getTime()-clock)/1000});
}
const otpSignin=await call('/sign-in/email',{email:'creation-policy@example.test',password:'owned-password',rememberMe:false});
const sent=await call('/two-factor/send-otp',{},otpSignin.cookie);assert.equal(sent.response.status,200);
const otpVerified=await call('/two-factor/verify-otp',{code:deliveredOtp},otpSignin.cookie);assert.equal(otpVerified.response.status,200);
assert.equal(ref.database.session.find(x=>x.token===otpVerified.body.token).expiresAt.getTime(),clock+86400000);
rows.push({mode:'otp-transient',seconds:86400});
const disabled=await call('/two-factor/disable',{password:'owned-password'},enabled.cookie);assert.equal(disabled.response.status,200);
assert.equal(ref.database.session.find(x=>x.token===decodeURIComponent(disabled.cookie.split(';')[0].split('=')[1]).split('.')[0]).expiresAt.getTime(),clock+120000);
const transient=await call('/sign-in/email',{email:'creation-policy@example.test',password:'owned-password',rememberMe:false});
const changed=await call('/change-password',{currentPassword:'owned-password',newPassword:'next-password',revokeOtherSessions:true},transient.cookie);assert.equal(changed.response.status,200);
active=ref.database.session.find(x=>x.token===changed.body.token);assert.equal(active.expiresAt.getTime(),clock+120000);
const token=await createEmailVerificationToken((await auth.$context).secret,'creation-policy@example.test');
const marker=transient.cookie.split('; ').filter(x=>x.includes('dont_remember')).join('; ');
const verification=await call('/verify-email?token='+token,null,marker);assert.equal(verification.response.status,200);
const newtoken=decodeURIComponent(verification.cookie.split(';')[0].split('=')[1]).split('.')[0];active=ref.database.session.find(x=>x.token===newtoken);assert.equal(active.expiresAt.getTime(),clock+120000);
assert.equal(verification.response.headers.getSetCookie().some(x=>x.includes('Max-Age=')),false);
rows.push({mode:'mfa-enroll-disable-password-auto-verify',seconds:120});

for(const disabled of [true,false]) {
 const current=betterAuth({...ref.config,session:{expiresIn:300,updateAge:0,freshAge:0,disableSessionRefresh:disabled},rateLimit:{enabled:false},logger:{disabled:true}});
 const cookie=verification.cookie.split('; ').filter(x=>!x.includes('dont_remember')).join('; ');
 const response=await current.handler(new Request('http://localhost:3000/api/auth/get-session',{headers:{cookie}}));assert.equal(response.status,200);
 const body=await response.json();assert.equal(new Date(body.session.expiresAt).getTime(),clock+(disabled?120000:300000));
 rows.push({mode:'existing-policy-'+disabled,seconds:disabled?120:300});
}
globalThis.Date=RealDate;
console.log('PASS session policy actual'+rows.length,JSON.stringify(rows));
