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


const {createAccessControl}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/plugins/access/access.mjs'));
const ac=createAccessControl({project:['read','write'],ac:['create','update','delete']});
for(const [label,roles,member,permissions,expected,dynamic] of [
 ['replacement',{custom:ac.newRole({project:['read']})},'owner',{platform:['platform:read']},false],
 ['custom',{custom:ac.newRole({project:['read']})},'custom',{project:['read']},true],
 ['empty',{},'owner',{platform:['platform:read']},false],
 ['or',{custom:ac.newRole({project:['read']}),other:ac.newRole({project:['write']})},'custom,other',{project:['write']},true],
 ['no-cross-role-union',{custom:ac.newRole({project:['read']}),other:ac.newRole({project:['write']})},'custom,other',{project:['read','write']},false],
 ['dynamic-merge',{custom:ac.newRole({project:['read']})},'custom',{project:['read','write']},true,{project:['write']}],
 ['custom-authorize',{custom:{statements:{},authorize(){return {success:true};}}},'custom',{project:['write']},true],
 ['dynamic-replaces-authorize',{custom:{statements:{},authorize(){return {success:true};}}},'custom',{project:['write']},false,{project:['read']}],
]){const auth=setup({ac,roles},1);ref.database.member[0].role=member;if(dynamic)ref.database.organizationRole.push({id:'dynamic-custom',organizationId:'org-policy-existing',role:'custom',permission:JSON.stringify(dynamic),createdAt:new Date()});const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions},cookie);assert.equal(out.status,200,JSON.stringify(out));assert.equal(out.body.success,expected,label);rows.push({label,success:out.body.success});}
const full={organization:['update','delete'],member:['update','delete'],invitation:['create','cancel'],team:['create','update','delete'],ac:['create','read','update','delete'],project:['read','write','outside-catalog']};
const catalog=createAccessControl({...full,project:['read','write']});
for(const [label,body,expected,code] of [
 ['reserved',{role:'boss',permission:{}},400,'ROLE_NAME_IS_ALREADY_TAKEN'],
 ['resource',{role:'fresh',permission:{unknown:['read']}},400,'INVALID_RESOURCE'],
 ['permission',{role:'fresh',permission:{project:['missing']}},403,'YOU_ARE_NOT_ALLOWED_TO_CREATE_A_ROLE'],
 ['create',{role:'fresh',permission:{project:['read']}},200,null],
 ['catalog-action-not-runtime-limit',{role:'fresh',permission:{project:['outside-catalog']}},200,null],
 ['old-default-not-reserved',{role:'owner',permission:{}},200,null],
]){const auth=setup({ac:catalog,roles:{boss:catalog.newRole(full),reader:catalog.newRole({project:['read']})}},1);ref.database.member[0].role='boss';const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',...body},cookie);assert.equal(out.status,expected,JSON.stringify(out));if(code)assert.equal(out.body.code,code,JSON.stringify(out));rows.push({label,status:out.status,code:out.body?.code});}
for(const role of ['reader','owner','admin','member','viewer']){const auth=setup({creatorRole:'boss',ac:catalog,roles:{boss:catalog.newRole(full),reader:catalog.newRole({project:['read']})}},1);ref.database.member[0].role='boss';const out=await call(auth,'/organization/invite-member',{organizationId:'org-policy-existing',email:'new-ac@example.test',role},cookie);assert.equal(out.status,role==='viewer'?400:200,JSON.stringify(out));rows.push({label:'invite-static-admission',role,status:out.status,body:out.body});}
{
 const auth=setup({ac:catalog,roles:{boss:catalog.newRole(full),reader:catalog.newRole({project:['read']})}},1);ref.database.member[0].role='boss';
 const created=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'fresh',permission:{project:['read']}},cookie);assert.equal(created.status,200);const roleId=created.body.roleData.id;
 for(const [path,body,status,code]of [
 ['/organization/get-role?organizationId=org-policy-existing&roleId='+roleId,null,200,null],
 ['/organization/list-roles?organizationId=org-policy-existing',null,200,null],
 ['/organization/update-role',{organizationId:'org-policy-existing',roleId,data:{roleName:'boss'}},400,'ROLE_NAME_IS_ALREADY_TAKEN'],
 ['/organization/update-role',{organizationId:'org-policy-existing',roleId,data:{permission:{project:['write']}}},200,null],
 ['/organization/delete-role',{organizationId:'org-policy-existing',roleName:'boss'},400,'CANNOT_DELETE_A_PRE_DEFINED_ROLE'],
 ['/organization/delete-role',{organizationId:'org-policy-existing',roleId},200,null],
 ]){const out=await call(auth,path,body,cookie);assert.equal(out.status,status,JSON.stringify(out));if(code)assert.equal(out.body.code,code);rows.push({label:'role-lifecycle',path:path.split('?')[0],status:out.status,code:out.body?.code});}
}
// Own keys, inherited faults, and the pinned Zod record boundary are explicit.
for(const name of ['','__proto__','constructor','toString']){
 for(const own of [true,false]){const roleMap=Object.fromEntries(own?[[name,ac.newRole({project:['read']})]]:[]);const auth=setup({ac,roles:roleMap},1);ref.database.member[0].role=name;const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{project:['read']}},cookie);assert.equal(out.status,own||name===''?200:500);if(out.status===200)assert.equal(out.body.success,own);rows.push({boundary:'prototype-role',name,own,status:out.status});}
 const statements=Object.fromEntries([[name,['read']]]);const resourceAc=createAccessControl(statements);const auth=setup({ac:resourceAc,roles:{viewer:resourceAc.newRole(statements)}},1);const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:statements},cookie);assert.equal(out.status,200);assert.equal(out.body.success,name!=='__proto__');rows.push({boundary:'prototype-resource',name,status:out.status,success:out.body.success});
}
for(const [actor,status]of [['boss,constructor',200],['constructor,boss',500]]){const auth=setup({ac:catalog,roles:{boss:catalog.newRole(full)}},1);ref.database.member[0].role=actor;const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{project:['read']}},cookie);assert.equal(out.status,status);rows.push({label:'role-short-circuit',actor,status});}
for(const own of [false,true]){const statements=own?{constructor:['read']}:{};const auth=setup({ac:catalog,roles:{boss:catalog.newRole(statements)}},1);ref.database.member[0].role='boss';ref.database.organizationRole.push({id:'dynamic-constructor',organizationId:'org-policy-existing',role:'boss',permission:'{"constructor":["write"]}',createdAt:new Date()});const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{constructor:['write']}},cookie);assert.equal(out.status,own?200:500);rows.push({label:'dynamic-inherited-resource',own,status:out.status});}
{
const auth=setup({ac:catalog,roles:{boss:catalog.newRole(full)}},1);ref.database.member[0].role='boss';
for(const [path,body]of [
 ['/organization/has-permission',{organizationId:'org-policy-existing',permissions:JSON.parse('{"__proto__":42}')}],
 ['/organization/create-role',{organizationId:'org-policy-existing',role:'proto-record',permission:JSON.parse('{"__proto__":42,"project":["read"]}')}],
 ['/organization/update-role',{organizationId:'org-policy-existing',roleName:'proto-record',data:{permission:JSON.parse('{"__proto__":42,"project":["write"]}')}}],
]){assert.ok(JSON.stringify(body).includes('"__proto__":42'));const out=await call(auth,path,body,cookie);assert.equal(out.status,200,JSON.stringify(out));if(path.endsWith('has-permission'))assert.equal(out.body.success,false);else assert.equal(Object.hasOwn(out.body.roleData.permission,'__proto__'),false);rows.push({label:'zod-record-proto-admission',path,status:out.status});}
}
for(const [encoded,status]of [
 ['{"toString":["read"],"0":["read"]}',200],
 ['{"toString":["read"],"absent":["read"]}',500],
 ['{"absent":["read"],"toString":["read"]}',200],
 ['{"toString":["read"],"01":["read"]}',500],
 ['{"toString":["read"],"4294967294":["read"]}',200],
 ['{"toString":["read"],"4294967295":["read"]}',500],
]){const auth=setup({ac:catalog,roles:{boss:catalog.newRole(full)}},1);ref.database.member[0].role='boss';const response=await auth.handler(new Request('http://localhost:3000/api/auth/organization/has-permission',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},body:'{"organizationId":"org-policy-existing","permissions":'+encoded+'}'}));assert.equal(response.status,status);const text=await response.text();if(status===200)assert.equal(JSON.parse(text).success,false);else assert.equal(text,'');rows.push({label:'raw-resource-order',encoded,status});}
for(const [label,actor,dynamic,request,own] of [
['repeat','__proto__',[['__proto__',{project:['read']}],['__proto__',{project:['write']}]],{project:['read','write']}],
['shadow','authorize',[['__proto__',{project:['read']}],['authorize',{project:['write']}]],{project:['write']}],
['spread-string','statements',[['__proto__',{statements:['readwrite']}],['statements',{}]],{'0':['read']}],
['spread-merge','statements',[['__proto__',{statements:['readwrite']}],['statements',{'0':['other']}]],{'0':['r','other']}],
['own-proto','authorize',[['__proto__',{project:['read']}]],{project:['read']},true],
]){const auth=setup({ac,roles:own?Object.fromEntries([['__proto__',ac.newRole({})]]):{}},1);ref.database.member[0].role=actor;for(const [i,[role,permission]]of dynamic.entries())ref.database.organizationRole.push({id:'dynamic-'+i,organizationId:'org-policy-existing',role,permission:JSON.stringify(permission),createdAt:new Date()});const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:request},cookie);assert.equal(out.status,200,label);assert.equal(out.body.success,!own,label);rows.push({label,status:out.status,body:out.body});}
for(const actor of ['__proto__','authorize','statements','constructor']){const auth=setup({ac,roles:{}},1);ref.database.member[0].role=actor;ref.database.organizationRole.push({id:'dynamic-proto',organizationId:'org-policy-existing',role:'__proto__',permission:'{"project":["read"]}',createdAt:new Date()});const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{project:['read']}},cookie);assert.equal(out.status,actor==='__proto__'?200:500);if(out.status===200)assert.equal(out.body.success,true);rows.push({label:'dynamic-role-prototype',actor,status:out.status});}
console.log('PASS access-control audit'+rows.length,JSON.stringify(rows));
