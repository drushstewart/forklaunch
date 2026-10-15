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


for(const [name,limit] of [['zero',0],['negative',-1],['fraction',1.5],['nan',NaN],['positiveInfinity',Infinity],['negativeInfinity',-Infinity],['large',2**32]]) {
 let calls=0,args;
 const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:async(...values)=>{calls++;args=values;return limit;}}},1);
 ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({ac:['create']}),createdAt:new Date()});
 const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'new',permission:{}},cookie);
 assert.equal(out.status,1>=limit?400:200,JSON.stringify(out));assert.equal(calls,1);assert.deepEqual(args,['org-policy-existing']);
 rows.push({name,status:out.status,code:out.body?.code,calls});
}
for(const mutation of ['add-role','delete-role','delete-session','delete-user','delete-member','demote-member','delete-organization','duplicate-name','sync-fail','async-fail']) {
 let calls=0;
 const callback=()=>{calls++;
  if(mutation==='sync-fail')throw Error('PRIVATE-MARKER');
  if(mutation==='async-fail')return Promise.reject(Error('PRIVATE-MARKER'));
  if(mutation==='add-role'||mutation==='duplicate-name')ref.database.organizationRole.push({id:'intervening',organizationId:'org-policy-existing',role:mutation==='duplicate-name'?'new':'other',permission:'{}',createdAt:new Date()});
  if(mutation==='delete-role')ref.database.organizationRole.length=0;
  if(mutation==='delete-session')ref.database.session.length=0;
  if(mutation==='delete-user')ref.database.user=ref.database.user.filter(x=>x.id!==uid);
  if(mutation==='delete-member')ref.database.member.length=0;
  if(mutation==='demote-member')ref.database.member[0].role='member';
  if(mutation==='delete-organization')ref.database.organization.length=0;
  return Promise.resolve(mutation==='add-role'?1:100);
 };
 const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:callback}},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({ac:['create']}),createdAt:new Date()});
 if(mutation==='delete-role')ref.database.organizationRole.push({id:'seed',organizationId:'org-policy-existing',role:'other',permission:'{}',createdAt:new Date()});
 const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'new',permission:{}},cookie);
 const expected=['sync-fail','async-fail'].includes(mutation)?500:['add-role','duplicate-name'].includes(mutation)?400:200;
 assert.equal(out.status,expected,JSON.stringify({mutation,out}));assert.equal(calls,1);
 rows.push({mutation,status:out.status,code:out.body?.code,created:ref.database.organizationRole.some(x=>x.role==='new')});
}
for(const [kind,role,permission,expected,callsExpected]of [['predefined','viewer',{},400,0],['unauthorized','new',{},403,0],['quota-before-resource','new',{unknown:['x']},400,1],['quota-before-existing','existing',{},400,1]]) {
 let calls=0;const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:async()=>{calls++;return 0;}}},1);
 ref.database.member[0].role=kind==='unauthorized'?'member':'owner';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({ac:['create']}),createdAt:new Date()});
 ref.database.organizationRole.push({id:'existing',organizationId:'org-policy-existing',role:'existing',permission:'{}',createdAt:new Date()});
 const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role,permission},cookie);
 assert.equal(out.status,expected,JSON.stringify(out));assert.equal(calls,callsExpected);rows.push({kind,status:out.status,code:out.body?.code,calls});
}

for(const change of ['remove-grant','replace-grant']) {
 const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:async()=>{
  const row=ref.database.organizationRole.find(x=>x.id==='ac-owner');
  row.permission=JSON.stringify({ac:['create'],platform:change==='remove-grant'?[]:['platform:write']});return 100;
 }}},1);ref.database.member[0].role='quota-author';
 ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'quota-author',permission:JSON.stringify({ac:['create'],platform:['platform:write']}),createdAt:new Date()});
 const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'new',permission:{platform:['platform:write']}},cookie);
 assert.equal(out.status,200,JSON.stringify(out));rows.push({change,status:out.status,code:out.body?.code});
}
{
 const OriginalDate=Date;const later=OriginalDate.now()+60_000;
 const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:async()=>{
  globalThis.Date=class extends OriginalDate {constructor(...args){super(...(args.length?args:[later]));} static now(){return later;}};return 100;
 }}},1);ref.database.member[0].role='owner';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'owner',permission:JSON.stringify({ac:['create']}),createdAt:new Date()});
 try {const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'new',permission:{}},cookie);assert.equal(out.status,200);assert.equal(new OriginalDate(out.body.roleData.createdAt).getTime(),later);rows.push({clock:'after-callback',status:200});}finally{globalThis.Date=OriginalDate;}
}

for(const mode of ['same-org','other-org','independent-auth','concurrent-request']) {
 let auth,resolveReady,release;const ready=new Promise(r=>resolveReady=r);const wait=new Promise(r=>release=r);
 const policy={dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:async()=>{
  ref.database.organizationRole.find(x=>x.id==='ac-owner').permission=JSON.stringify({ac:['create']});
  if(mode==='concurrent-request'){resolveReady();await wait;}
  else{
   const caller=mode==='independent-auth'?betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...p.options,dynamicAccessControl:{enabled:true}}):p)}):auth;
   const out=await call(caller,'/organization/has-permission',{organizationId:mode==='other-org'?'other-org':'org-policy-existing',permissions:{platform:['platform:write']}},cookie);assert.equal(out.status,200,JSON.stringify(out));
  }
  return 100;
 }}};
 auth=setup(policy,1);ref.database.member[0].role='quota-author';
 ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'quota-author',permission:JSON.stringify({ac:['create'],platform:['platform:write']}),createdAt:new Date()});
 ref.database.organization.push({id:'other-org',name:'Other',slug:'other',createdAt:new Date()});
 ref.database.member.push({id:'other-member',organizationId:'other-org',userId:uid,role:'quota-author',createdAt:new Date()});
 ref.database.organizationRole.push({id:'other-role',organizationId:'other-org',role:'quota-author',permission:'{}',createdAt:new Date()});
 const request=call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'new',permission:{platform:['platform:write']}},cookie);
 if(mode==='concurrent-request'){await ready;const out=await call(auth,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{platform:['platform:write']}},cookie);assert.equal(out.status,200);release();}
 const out=await request;assert.equal(out.status,mode==='other-org'?200:403,JSON.stringify({mode,out}));rows.push({cache:mode,status:out.status,code:out.body?.code});
}

{
 let calls=0;
 const auth=setup({dynamicAccessControl:{enabled:true,maximumRolesPerOrganization:async()=>{
  ref.database.organizationRole.length=0;
  const base=ref.config.plugins.find(p=>p.id==='organization').options;
  const role=base.ac.newRole({platform:['platform:write']});role.authorize=()=>{calls++;return {success:false,error:'owned denial'};};
  const nested=betterAuth({...ref.config,secret,logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...base,roles:{'quota-author':role},dynamicAccessControl:{enabled:true}}):p)});
  const out=await call(nested,'/organization/has-permission',{organizationId:'org-policy-existing',permissions:{platform:['platform:write']}},cookie);assert.equal(out.status,200);return 100;
 }}},1);
 ref.database.member[0].role='quota-author';ref.database.organizationRole.push({id:'ac-owner',organizationId:'org-policy-existing',role:'quota-author',permission:JSON.stringify({ac:['create'],platform:['platform:write']}),createdAt:new Date()});
 const out=await call(auth,'/organization/create-role',{organizationId:'org-policy-existing',role:'new',permission:{platform:['platform:write']}},cookie);
 assert.equal(out.status,403,JSON.stringify(out));assert.equal(calls,2);rows.push({cache:'authorizer-config-replaced',status:out.status,calls});
}
console.log('PASS role quota callback actual '+rows.length,JSON.stringify(rows));
