import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/index.mjs'));
const {organization}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/plugins/organization/index.mjs'));
const ref=await betterAuthReference(root);
const call=async(auth,path,body,cookie='')=>{const r=await auth.handler(new Request('http://localhost:3000/api/auth'+path,{method:body?'POST':'GET',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},...(body?{body:JSON.stringify(body)}:{})}));const text=await r.text();return {status:r.status,body:text?JSON.parse(text):null,cookie:r.headers.getSetCookie().map(x=>x.split(';')[0]).join('; ')};};
const signup=await call(ref.auth,'/sign-up/email',{name:'Owned',firstName:'Owned',lastName:'User',email:'org-policy@example.test',password:'owned-password'});assert.equal(signup.status,200);
const saved=structuredClone(ref.database),uid=signup.body.user.id,cookie=signup.cookie;
const secret=(await ref.auth.$context).secret;
const rows=[];
function setup(policy={},members=0,handler){for(const key of Object.keys(ref.database))ref.database[key]=structuredClone(saved[key]);ref.database.user[0].emailVerified=true;
ref.database.user.push({...structuredClone(ref.database.user[0]),id:'target-user',email:'target-policy@example.test'});
ref.database.organization.push({id:'org-policy-existing',name:'Existing',slug:'existing',createdAt:new Date(),subscription:'free',status:'active'});
for(let i=0;i<members;i++)ref.database.member.push({id:'member-'+i,organizationId:'org-policy-existing',userId:i?('other-'+i):uid,role:i?'member':'viewer',createdAt:new Date()});
const auth=betterAuth({...ref.config,secret,advanced:{...ref.config.advanced,...(handler?{backgroundTasks:{handler}}:{})},logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...p.options,...policy}):p)});return auth;}

for(const mode of ['success','prepare-error','async-error','await','capture','handler-error','resend-prepare','resend-async','cancel-quota']){
 let calls=0,handled=0,release,entered;const pending=new Promise(r=>release=r),ready=new Promise(r=>entered=r),tasks=[];let observed;
 const hook=data=>{calls++;observed=data;entered();if(mode.includes('prepare'))throw Error('owned-preparation');if(mode.includes('async'))return Promise.reject(Error('owned-delivery'));return ['await','capture','handler-error'].includes(mode)?pending:Promise.resolve();};
 const handler=task=>{handled++;tasks.push(task);if(mode==='handler-error')throw Error('owned-scheduling');};
 const policy={sendInvitationEmail:hook,...(mode==='cancel-quota'?{cancelPendingInvitationsOnReInvite:true,invitationLimit:0}:{})};
 const auth=setup(policy,1,['capture','handler-error'].includes(mode)?handler:undefined);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});
 const resend=mode.startsWith('resend');if(resend||mode==='cancel-quota')ref.database.invitation.push({id:'old',email:'delivery@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+1000),createdAt:new Date()});
 let done=false;const response=call(auth,'/organization/invite-member',{email:'DELIVERY@example.test',role:'member',organizationId:'org-policy-existing',resend},cookie).then(r=>{done=true;return r;});
 if(mode==='cancel-quota'){const out=await response;assert.equal(out.status,403);assert.equal(calls,0);assert.equal(ref.database.invitation[0].status,'canceled');rows.push({mode,status:out.status,calls});continue;}
 await ready;await new Promise(r=>setTimeout(r,20));assert.equal(done,mode!=='await');assert.equal(ref.database.invitation.length,1);assert.equal(observed.email,'delivery@example.test');assert.equal(observed.inviter.user.id,uid);assert.equal(observed.inviter.organizationId,'org-policy-existing');assert.equal(observed.organization.id,'org-policy-existing');assert.equal(observed.invitation.id,observed.id);if(resend){assert.equal(observed.id,'old');assert.ok(Date.parse(ref.database.invitation[0].expiresAt)>Date.now()+3600000);}
 release();const out=await response;await Promise.all(tasks);assert.equal(out.status,mode.includes('prepare')?500:200);assert.equal(calls,1);assert.equal(handled,['capture','handler-error'].includes(mode)?1:0);rows.push({mode,status:out.status,calls,handled,persisted:true,returnedBeforeDelivery:mode!=='await'});
}
console.log('PASS invitation delivery actual'+rows.length,JSON.stringify(rows));
