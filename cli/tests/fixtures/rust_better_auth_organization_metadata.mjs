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


function admit(){ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'owner-grant',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});}
const body={email:'new-invite@example.test',role:'member',organizationId:'org-policy-existing'};

for(const metadata of [{nested:{enabled:true},count:3},JSON.stringify({nested:{enabled:true},count:3}),null,42,[true,null,{label:"é"}]]){
 const seen=[];const auth=setup({invitationLimit:async(data)=>{seen.push({stage:'quota',metadata:data.organization.metadata});return 100;},sendInvitationEmail:async(data)=>{seen.push({stage:'delivery',metadata:data.organization.metadata});}},1);admit();
 ref.database.organization[0].metadata=metadata;
 const out=await call(auth,'/organization/invite-member',body,cookie);
 rows.push({source:'seeded',metadata,status:out.status,code:out.body?.code,seen});
}
{
 const seen=[];const auth=setup({invitationLimit:async(data)=>{seen.push({stage:'quota',metadata:data.organization.metadata});return 100;},sendInvitationEmail:async(data)=>{seen.push({stage:'delivery',metadata:data.organization.metadata});}},1);
 const created=await call(auth,'/organization/create',{name:'Metadata owned',slug:'metadata-owned',metadata:{nested:{enabled:true},count:3}},cookie);
 assert.equal(created.status,200,JSON.stringify(created.body));
 const org=ref.database.organization.find(x=>x.id===created.body.id);
 ref.database.organizationRole.push({id:'metadata-owner-grant',organizationId:org.id,role:'owner',permission:JSON.stringify({invitation:['create']}),createdAt:new Date()});
 const out=await call(auth,'/organization/invite-member',{...body,organizationId:org.id},cookie);
 rows.push({source:'created',storedMetadata:org.metadata,responseMetadata:created.body.metadata,status:out.status,code:out.body?.code,seen});
}
for(const row of rows){
 assert.equal(row.status,200,JSON.stringify(row));
 const expected=row.source==='created'?row.storedMetadata:row.metadata;
 assert.deepEqual(row.seen,[{stage:'quota',metadata:expected},{stage:'delivery',metadata:expected}]);
 if(row.source==='created'){
  assert.equal(typeof row.storedMetadata,'string');
  assert.deepEqual(JSON.parse(row.storedMetadata),row.responseMetadata);
 }
}
for(const metadata of [{nested:{enabled:true}},42,[true,null],'{"stored":true}',null]){
 const seen=[];const auth=setup({membershipLimit:async(_user,organization)=>{seen.push(organization.metadata);return 100;}},1);
 ref.database.organization[0].metadata=metadata;
 const added=await auth.api.addMember({body:{userId:'target-user',role:'member',organizationId:'org-policy-existing'}});
 assert.equal(added.userId,'target-user');assert.deepEqual(seen,[metadata]);
}
for(const metadata of [{nested:{enabled:true}},42,[true,null],'{"stored":true}',null]){
 const seen=[];const auth=setup({membershipLimit:async(_user,organization)=>{seen.push(organization.metadata);return 100;}},0);
 ref.database.organization[0].metadata=metadata;
 ref.database.invitation.push({id:'metadata-accept',organizationId:'org-policy-existing',inviterId:'target-user',email:'org-policy@example.test',role:'member',status:'pending',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()});
 const accepted=await call(auth,'/organization/accept-invitation',{invitationId:'metadata-accept'},cookie);
 assert.equal(accepted.status,200,JSON.stringify(accepted.body));assert.deepEqual(seen,[metadata]);
}
console.log('PASS organization callback metadata shapes '+rows.length+' invitation/delivery +5 add-member +5 accept-invitation');

