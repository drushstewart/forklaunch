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
function setup(policy={},members=0){for(const key of Object.keys(ref.database))ref.database[key]=structuredClone(saved[key]);ref.database.user[0].emailVerified=true;
ref.database.user.push({...structuredClone(ref.database.user[0]),id:'target-user',email:'target-policy@example.test'});
ref.database.organization.push({id:'org-policy-existing',name:'Existing',slug:'existing',createdAt:new Date(),subscription:'free',status:'active'});
for(let i=0;i<members;i++)ref.database.member.push({id:'member-'+i,organizationId:'org-policy-existing',userId:i?('other-'+i):uid,role:i?'member':'viewer',createdAt:new Date()});
const auth=betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...p.options,...policy}):p)});return auth;}


for(const [name,policy,body,expected,oldStatus]of [
['quota-after-cancel',{invitationLimit:0},{},403,'canceled'],
['team-after-cancel',{}, {teamId:'missing-team'},400,'canceled'],
['invalidrole-before-cancel',{}, {role:'missing-role'},400,'pending'],
['resend-skips-invalidteam',{}, {resend:true,teamId:'missing-team'},200,'pending'],
]){const auth=setup({cancelPendingInvitationsOnReInvite:true,...policy},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});ref.database.invitation.push({id:'old',email:'new-invite@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});const out=await call(auth,'/organization/invite-member',{email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing',...body},cookie);assert.equal(out.status,expected,name);assert.equal(ref.database.invitation[0].status,oldStatus,name);assert.equal(ref.database.invitation.length,1);rows.push({name,status:out.status,code:out.body?.code,oldStatus});}

for(const [name,enabled,resend,status,count,oldStatus]of [['default',false,false,400,1,'pending'],['replace',true,false,200,2,'canceled'],['resend',true,true,200,1,'pending']]){
const auth=setup({cancelPendingInvitationsOnReInvite:enabled},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});ref.database.invitation.push({id:'old',email:'new-invite@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});const out=await call(auth,'/organization/invite-member',{email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing',resend},cookie);assert.equal(out.status,status);assert.equal(ref.database.invitation.length,count);assert.equal(ref.database.invitation[0].status,oldStatus);rows.push({name,status,oldStatus,count});}
{
const auth=setup({cancelPendingInvitationsOnReInvite:true},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});
for(const id of ['old1','old2'])ref.database.invitation.push({id,email:'new-invite@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
const out=await call(auth,'/organization/invite-member',{email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing'},cookie);assert.equal(out.status,200);assert.equal(ref.database.invitation.length,3);assert.equal(ref.database.invitation[0].status,'canceled');assert.equal(ref.database.invitation[1].status,'pending');rows.push({name:'existing-duplicate-pending',status:out.status,count:3});
}

for(const count of [100,101]){
const auth=setup({cancelPendingInvitationsOnReInvite:true,invitationLimit:count},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});for(let i=0;i<count;i++)ref.database.invitation.push({id:'old'+i,email:'new-invite@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});const out=await call(auth,'/organization/invite-member',{email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing'},cookie);assert.equal(out.status,200);assert.equal(ref.database.invitation.length,count+1);assert.equal(ref.database.invitation.filter(r=>r.status==='canceled').length,1);rows.push({name:'saturated-window-'+count,status:out.status,count:count+1});}
console.log('PASS cancellation actual'+rows.length,JSON.stringify(rows));
