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

for(const [name,policy,existing,resend]of [
 ['default',{},false,false],['zero-expiry',{invitationExpiresIn:0},false,false],['short-expiry',{invitationExpiresIn:2},false,false],
 ['zero-limit',{invitationLimit:0},false,false],['resend-zero-limit',{invitationLimit:0},true,true],
 ['cancel-zero-limit',{invitationLimit:0,cancelPendingInvitationsOnReInvite:true},true,false],
 ['cancel',{cancelPendingInvitationsOnReInvite:true},true,false],['resend-cancel',{cancelPendingInvitationsOnReInvite:true},true,true],
]){const auth=setup(policy,1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});
if(existing)ref.database.invitation.push({id:'old',email:'new-invite@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
const start=Date.now();const out=await call(auth,'/organization/invite-member',{email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing',resend},cookie);
rows.push({name,status:out.status,code:out.body?.code,id:out.body?.id==='old'?'old':'new',ttl:out.body?.expiresAt?Math.round((Date.parse(out.body.expiresAt)-start)/1000):null,stored:ref.database.invitation.map(x=>({id:x.id==='old'?'old':'new',status:x.status}))});}
for(const [name,policy]of [['default-verification',{}],['verification-false',{requireEmailVerificationOnInvitation:false}],['verification-true',{requireEmailVerificationOnInvitation:true}]]){
const auth=setup(policy,0);ref.database.user[0].emailVerified=false;ref.database.invitation.push({id:'invite',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});const out=await call(auth,'/organization/accept-invitation',{invitationId:'invite'},cookie);rows.push({name,status:out.status,code:out.body?.code});}

const expected=[
['default',200,172800],['zero-expiry',200,172800],['short-expiry',200,2],['zero-limit',403,null],['resend-zero-limit',200,172800],['cancel-zero-limit',403,null],['cancel',200,172800],['resend-cancel',200,172800],['default-verification',403],['verification-false',200],['verification-true',403]];
for(let i=0;i<expected.length;i++){assert.equal(rows[i].name,expected[i][0]);assert.equal(rows[i].status,expected[i][1]);if(expected[i].length===3)assert.equal(rows[i].ttl,expected[i][2]);}
assert.equal(rows[5].stored[0].status,'canceled');assert.equal(rows[4].id,'old');assert.equal(rows[7].id,'old');assert.equal(rows[7].stored[0].status,'pending');
// Recipient policy affects by-ID get/accept/reject, never session-email enumeration.
for(const operation of ['get-invitation','reject-invitation','list-user-invitations']){
const auth=setup({requireEmailVerificationOnInvitation:false},1);ref.database.member[0].userId='target-user';ref.database.user[0].emailVerified=false;
ref.database.invitation.push({id:'invite',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
const path='/organization/'+operation+(operation==='get-invitation'?'?id=invite':'');const out=await call(auth,path,operation==='reject-invitation'?{invitationId:'invite'}:null,cookie);assert.equal(out.status,operation==='list-user-invitations'?403:200);rows.push({name:operation+'-unverified-override',status:out.status});}
for(const [name,count,expired,limit,expected]of [['limit-window',101,0,101,200],['expiry-after-window',101,100,1,200],['at-quota',1,0,1,403]]){
const auth=setup({invitationLimit:limit},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});
for(let i=0;i<count;i++)ref.database.invitation.push({id:'pending-'+i,email:'pending-'+i+'@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:uid,expiresAt:new Date(Date.now()+(i<expired?-3600000:3600000)),createdAt:new Date()});
const out=await call(auth,'/organization/invite-member',{email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing'},cookie);assert.equal(out.status,expected,name);assert.equal(ref.database.invitation.length,count+(expected===200?1:0));rows.push({name,status:out.status});}
const RealDate=globalThis.Date;
for(const [offset,expected]of [[2000,200],[2001,400]]){
const auth=setup({requireEmailVerificationOnInvitation:false},1);ref.database.member[0].userId='target-user';ref.database.user[0].emailVerified=false;
const start=RealDate.now();ref.database.invitation.push({id:'clock-invite',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new RealDate(start+2000),createdAt:new RealDate(start)});
globalThis.Date=class extends RealDate{constructor(...args){super(...(args.length?args:[start+offset]));}static now(){return start+offset;}};
try{const out=await call(auth,'/organization/get-invitation?id=clock-invite',null,cookie);assert.equal(out.status,expected);rows.push({name:'expiry-offset-'+offset,status:out.status});}finally{globalThis.Date=RealDate;}}
console.log('PASS invitation policy actual'+rows.length,JSON.stringify(rows,null,2));
