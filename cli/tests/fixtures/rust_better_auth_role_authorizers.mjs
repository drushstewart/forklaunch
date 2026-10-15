import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../../..');
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

for(const [label,actor,outcome,dynamic] of [
 ['grant','custom','grant',false],['deny','custom','deny',false],['throw','custom','throw',false],['missing','custom','missing',false],['repeat','custom,custom','deny',false],['shortcircuit','custom,constructor','grant',false],['earlyfault','constructor,custom','grant',false],['dynamic-replaces','custom','grant',true]
]){let calls=0;const observations=[];const role={statements:{project:['read']},authorize(request,connector){calls++;observations.push({keys:Object.keys(request),connector:connector??null,receiver:this===role});if(outcome==='throw')throw Error('owned-test-error');if(outcome==='missing')return undefined;return {success:outcome==='grant'};}};
const auth=setup({ac,roles:{custom:role}},1);ref.database.member[0].role=actor;if(dynamic)ref.database.organizationRole.push({id:'dynamic',organizationId:'org-policy-existing',role:'custom',permission:'{"project":["read"]}',createdAt:new Date()});const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{project:[label==='deny'?'read':'write']}},cookie);rows.push({label,status:out.status,body:out.body,calls,observations});}

for(const malformed of [false,true]){let calls=0;const roles={custom:{statements:{member:['update']},authorize(){calls++;return {success:false};}}};const auth=setup({ac,roles,creatorRole:'custom'},2);ref.database.user.push({...structuredClone(ref.database.user[0]),id:'other-1',email:'other1@owned.test'});ref.database.member[0].role='custom';if(malformed)ref.database.organizationRole.push({id:'bad-dynamic',organizationId:'org-policy-existing',role:'other',permission:'invalid',createdAt:new Date()});const out=await call(auth,'/organization/update-member-role',{organizationId:'org-policy-existing',memberId:'member-1',role:'member'},cookie);rows.push({label:'creator-bypass',malformed,status:out.status,body:out.body,calls});}
for(const name of ['__proto__','constructor','toString','']){let calls=0;const roles=Object.fromEntries([[name,{statements:{},authorize(){calls++;return {success:true};}}]]);const auth=setup({ac,roles},1);ref.database.member[0].role=name;const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{project:['write']}},cookie);rows.push({label:'own-name-authorizer',name,status:out.status,body:out.body,calls});}
{
 let keys=[];const auth=setup({ac,roles:{custom:{statements:{},authorize(request){keys=Object.keys(request);return {success:true};}}}},1);ref.database.member[0].role='custom';
 const response=await auth.handler(new Request('http://localhost:3000/api/auth/organization/has-permission',{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie},body:'{"organizationId":"org-policy-existing","permissions":{"tail":["write"],"0":["read"]}}'}));
 assert.equal(response.status,200);assert.deepEqual(keys,['0','tail']);assert.equal((await response.json()).success,true);rows.push({label:'request-entry-order',status:200,body:{success:true},calls:1});
}
{
 let keys;const auth=setup({ac,roles:{custom:{statements:{},authorize(request){keys=Object.keys(request);return {success:true};}}}},1);ref.database.member[0].role='custom';
 const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:JSON.parse('{"__proto__":42}')},cookie);assert.deepEqual(keys,[]);rows.push({label:'stripped-prototype-request',status:out.status,body:out.body,calls:1});
}
const expected=[
 ['grant',200,true,1],['deny',200,false,1],['throw',500,null,1],['missing',200,false,1],['repeat',200,false,2],['shortcircuit',200,true,1],['earlyfault',500,null,0],['dynamic-replaces',200,false,0],['creator-bypass',200,undefined,0],['creator-bypass',500,null,0],['own-name-authorizer',200,true,1],['own-name-authorizer',200,true,1],['own-name-authorizer',200,true,1],['own-name-authorizer',200,true,1],['request-entry-order',200,true,1],['stripped-prototype-request',200,true,1]
];
assert.equal(rows.length,expected.length);
for(const [i,[label,status,success,calls]]of expected.entries()){
 const row=rows[i];assert.equal(row.label,label);assert.equal(row.status,status,label);assert.equal(row.calls,calls,label);
 if(success===null)assert.equal(row.body,null,label);else if(success!==undefined)assert.equal(row.body.success,success,label);
 for(const observation of row.observations??[]){assert.equal(observation.receiver,true);assert.equal(observation.connector,null);assert.deepEqual(observation.keys,['project']);}
}
console.log('PASS synchronous role authorizer '+rows.length,JSON.stringify(rows));
