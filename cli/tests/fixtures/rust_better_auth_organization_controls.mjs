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

for(const [enabled,authenticated,expected]of [[true,true,404],[true,false,404],[false,false,401],[false,true,403]]){const auth=setup({disableOrganizationDeletion:enabled},1);const out=await call(auth,'/organization/delete',{organizationId:'org-policy-existing'},authenticated?cookie:'');assert.equal(out.status,expected,JSON.stringify(out));assert.equal(ref.database.organization.length,1);rows.push({enabled,authenticated,status:out.status,code:out.body?.code});}

for(const body of [{organizationId:42},{organizationId:null},{},{organizationId:''}]){const auth=setup({disableOrganizationDeletion:true},1);const out=await call(auth,'/organization/delete',body);const valid=body.organizationId==='';assert.equal(out.status,valid?404:400,JSON.stringify(out));if(!valid){const received=body.organizationId===null?'null':typeof body.organizationId;assert.deepEqual(out.body,{message:`[body.organizationId] Invalid input: expected string, received ${received}`,code:'VALIDATION_ERROR'});}assert.equal(ref.database.organization.length,1);rows.push({admission:body,status:out.status,body:out.body});}

for(const [limit,role,permission,expected,code]of [[0,'new',{},400,'TOO_MANY_ROLES'],[1,'new',{},400,'TOO_MANY_ROLES'],[2,'new',{},200,null],[0,'viewer',{},400,'ROLE_NAME_IS_ALREADY_TAKEN'],[0,'new',{invalid:['bad']},400,'TOO_MANY_ROLES'],[undefined,'new',{},200,null]]){const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:limit}},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({ac:['create']}),createdAt:new Date()});const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role,permission},cookie);assert.equal(out.status,expected,JSON.stringify(out));if(code)assert.equal(out.body.code,code);if(code==='TOO_MANY_ROLES')assert.equal(out.body.message,'This organization has too many roles');rows.push({limit,role,status:out.status,code:out.body?.code});}
for(const [limit,expected,code]of [[2,400,'TOO_MANY_ROLES'],[3,400,'ROLE_NAME_IS_ALREADY_TAKEN']]){const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:limit}},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({ac:['create']}),createdAt:new Date()},{id:'existing-name',organizationId:'org-policy-existing',role:'existing',permission:'{}',createdAt:new Date()});const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'existing',permission:{}},cookie);assert.equal(out.status,expected);assert.equal(out.body.code,code);rows.push({limit,name:'existing',status:out.status,code});}
console.log('PASS organization controls actual'+rows.length,JSON.stringify(rows));
