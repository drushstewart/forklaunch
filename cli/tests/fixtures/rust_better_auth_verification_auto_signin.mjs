import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const dist=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/';
const {betterAuthReference}=await import(pathToFileURL(`${root}/cli/tests/fixtures/rust_better_auth_reference.mjs`));
const {betterAuth}=await import(pathToFileURL(`${dist}index.mjs`));
const {createEmailVerificationToken}=await import(pathToFileURL(`${dist}api/routes/email-verification.mjs`));
const rows=[];
for(const mode of ['disabled','anonymous','current','foreign','invalid','expired','already','redirect','after-fail','create-fail','create-throw','expired-create-throw','expired-create-fail','transient','transient-current']){
 const ref=await betterAuthReference(root);
 const auth=betterAuth({...ref.config,emailVerification:{autoSignInAfterVerification:mode!=='disabled',afterEmailVerification:async()=>{if(mode==='after-fail')throw Error('owned-after')}},logger:{disabled:true},rateLimit:{enabled:false}});
 const signup=async(email,rememberMe=true)=>auth.handler(new Request('http://localhost:3000/api/auth/sign-up/email',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json'},body:JSON.stringify({name:'Owned',firstName:'Owned',lastName:'User',email,password:'owned-password',rememberMe})}));
 const first=await signup('auto-verify@example.test',!mode.startsWith('transient'));const initial=await first.json();
 let cookie=first.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ');
 if(mode==='foreign'){const other=await signup('foreign@example.test');cookie=other.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ');}
 if(mode.startsWith('expired'))ref.database.session.find(s=>s.userId===initial.user.id).expiresAt=new Date(0);
 if(mode==='already')ref.database.user.find(u=>u.id===initial.user.id).emailVerified=true;
 if(mode==='transient')cookie=first.headers.getSetCookie().map(x=>x.split(';')[0]).filter(x=>x.includes('dont_remember')).join('; ');
 if(mode==='create-throw'||mode==='expired-create-throw')(await auth.$context).internalAdapter.createSession=async()=>{throw Error('owned-create');};
 if(mode==='create-fail'||mode==='expired-create-fail')(await auth.$context).internalAdapter.createSession=async()=>null;
 const token=await createEmailVerificationToken((await auth.$context).secret,'auto-verify@example.test');
 const reqcookie=['current','foreign','expired','expired-create-throw','expired-create-fail','transient','transient-current'].includes(mode)?cookie:mode==='invalid'?'better-auth.session_token=invalid':undefined;
 const result=await auth.handler(new Request(`http://localhost:3000/api/auth/verify-email?token=${token}${mode==='redirect'?'&callbackURL=%2Fdone':''}`,{headers:reqcookie?{cookie:reqcookie}:{}}));
 const set=result.headers.getSetCookie();const signed=set.filter(x=>x.startsWith('better-auth.session_token=')).at(-1);
 const body=await result.text();
 const actualToken=signed?decodeURIComponent(signed.split(';')[0].split('=')[1]).split('.')[0]:undefined;
 const failed=['after-fail','create-fail','create-throw','expired-create-throw','expired-create-fail'].includes(mode);
 assert.equal(result.status,failed?500:mode==='redirect'?302:200);
 assert.equal(!!signed,mode==='expired-create-fail'||(!failed&&!['disabled','already'].includes(mode)));
 assert.equal(ref.database.user.find(u=>u.id===initial.user.id).emailVerified,true);
 assert.equal(ref.database.session.filter(s=>s.userId===initial.user.id).length,mode.startsWith('expired-create')?0:['disabled','current','expired','already','after-fail','create-fail','create-throw','transient-current'].includes(mode)?1:2);
 assert.equal(actualToken===initial.token,['current','transient-current'].includes(mode));
 if(signed)assert.equal(signed.includes('Max-Age='),!mode.startsWith('transient'));
 if(mode==='expired-create-throw'){assert.equal(body,'');assert.equal(set.length,0);}
 rows.push({mode,status:result.status,body,sessions:ref.database.session.filter(s=>s.userId===initial.user.id).length,foreign:ref.database.session.filter(s=>s.userId!==initial.user.id).length,verified:ref.database.user.find(u=>u.id===initial.user.id).emailVerified,cookie:!!signed,reused:actualToken===initial.token,maxAge:signed?.match(/Max-Age=([^;]+)/)?.[1],cookieNames:set.map(x=>x.split('=')[0]),expires:actualToken?ref.database.session.find(s=>s.token===actualToken)?.expiresAt:undefined});
}
console.log('PASS verification auto-signin actual15',JSON.stringify(rows));
