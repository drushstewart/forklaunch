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
for(const [name,policy,count,expected,code]of [
['denied-first',{allowUserToCreateOrganization:false,organizationLimit:0},1,403,'YOU_ARE_NOT_ALLOWED_TO_CREATE_A_NEW_ORGANIZATION'],
['zero-limit',{organizationLimit:0},0,403,'YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS'],
['one-empty',{organizationLimit:1},0,200],
['one-viewer-membership',{organizationLimit:1},1,403,'YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS'],
['two-viewer-membership',{organizationLimit:2},1,200],
]){const auth=setup(policy,count);const out=await call(auth,'/organization/create',{name:'Next',slug:'next'},cookie);assert.equal(out.status,expected,name);if(code)assert.equal(out.body.code,code,name);rows.push({name,status:out.status,code:out.body?.code});}
for(const [name,policy,count,userId,orgId,expected,code]of [
['member-one-full',{membershipLimit:1},1,'target-user','org-policy-existing',403,'ORGANIZATION_MEMBERSHIP_LIMIT_REACHED'],
['member-zero-fallback',{membershipLimit:0},1,'target-user','org-policy-existing',200],
['missing-user-first',{membershipLimit:1},1,'missing-user','org-policy-existing',400,'USER_NOT_FOUND'],
['duplicate-first',{membershipLimit:1},1,uid,'org-policy-existing',400,'USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION'],
['missing-org-first',{membershipLimit:1},0,'target-user','missing-org',400,'ORGANIZATION_NOT_FOUND'],
]){const auth=setup(policy,count);const response=await auth.api.addMember({body:{userId,role:'member',organizationId:orgId},asResponse:true});const body=await response.json();assert.equal(response.status,expected,name);if(code)assert.equal(body.code,code,name);rows.push({name,status:response.status,code:body?.code});}
for(const [name,limit,query,expected]of [['full-low',1,'',500],['full-low-explicit',1,'&membersLimit=1',200],['full-zero',0,'',200]]){const auth=setup({membershipLimit:limit},2);ref.database.user.push({...structuredClone(ref.database.user[0]),id:'other-1',email:'other-policy@example.test'});const out=await call(auth,'/organization/get-full-organization?organizationId=org-policy-existing'+query,null,cookie);assert.equal(out.status,expected,name);rows.push({name,status:out.status,members:out.body?.members?.length});}
for(const count of [99,100,101,102]){const auth=setup({organizationLimit:101},count);for(const member of ref.database.member)member.userId=uid;const out=await call(auth,'/organization/create',{name:'Window',slug:'window'},cookie);assert.equal(out.status,200);rows.push({name:'organization-window-'+count,status:out.status,code:out.body?.code});}
{const auth=setup({organizationLimit:1},1);ref.database.organization=[];const out=await call(auth,'/organization/create',{name:'Orphan',slug:'orphan'},cookie);assert.equal(out.status,403);assert.equal(out.body.code,'YOU_HAVE_REACHED_THE_MAXIMUM_NUMBER_OF_ORGANIZATIONS');rows.push({name:'organization-orphan',status:out.status,code:out.body?.code});}
for(const [name,limit,orgPresent,expected,code] of [
 ['invite-one-full',1,true,403,'ORGANIZATION_MEMBERSHIP_LIMIT_REACHED'],
 ['invite-zero-fallback',0,true,200],
 ['invite-missing-org',1,false,400,'ORGANIZATION_NOT_FOUND'],
]){const auth=setup({membershipLimit:limit},1);ref.database.member[0].userId='target-user';if(!orgPresent)ref.database.organization=[];
ref.database.invitation.push({id:'policy-invitation',email:'org-policy@example.test',organizationId:'org-policy-existing',role:'member',status:'pending',inviterId:'target-user',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
const out=await call(auth,'/organization/accept-invitation',{invitationId:'policy-invitation'},cookie);assert.equal(out.status,expected,name);if(code)assert.equal(out.body.code,code,name);assert.equal(ref.database.invitation[0].status,expected===200?'accepted':'pending');rows.push({name,status:out.status,code:out.body?.code});}
for(const [count,expected]of [[99,200],[100,403]]){const auth=setup({},count);const response=await auth.api.addMember({body:{userId:'target-user',role:'member',organizationId:'org-policy-existing'},asResponse:true});const body=await response.json();assert.equal(response.status,expected);if(expected===403)assert.equal(body.code,'ORGANIZATION_MEMBERSHIP_LIMIT_REACHED');rows.push({name:'default-members-'+count,status:response.status});}
{const auth=setup({allowUserToCreateOrganization:false},0);const out=await call(auth,'/organization/create',{name:'Denied',slug:'denied',userId:'target-user',allowUserToCreateOrganization:true,organizationLimit:999},cookie);assert.equal(out.status,403);rows.push({name:'request-cannot-override-policy',status:out.status});}
console.log('PASS organization scalar actual'+rows.length,JSON.stringify(rows));
