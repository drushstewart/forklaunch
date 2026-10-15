import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import {pathToFileURL} from 'node:url';
const root=new URL('../../..',import.meta.url).pathname.replace(/\/$/,'');
const deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(deps,'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES to the configured ORM fixture dependencies');
const {betterAuthOrm}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_orm.mjs'));
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/index.mjs'));
const {organization}=await import(pathToFileURL(deps+'/node_modules/better-auth/dist/plugins/organization/index.mjs'));
const source=await betterAuthOrm(root,deps);
const {persistence,entities,core}=source;
const {createAuthEncryptionOrm}=source.load('blueprint/iam-better-auth/domain/utils/authEncryptionPolicy.util.ts');
persistence.registerEncryptor(new persistence.FieldEncryptor('owned-team-orm-fixture-encryption-key'));
const schema='fl_team_quota_'+crypto.randomBytes(8).toString('hex');
const orm=await source.MikroORM.init({entities:Object.values(entities),clientUrl:process.env.FORKLAUNCH_TEST_POSTGRES_URL??'postgres://postgres:parity-test-only@127.0.0.1:32768/parity',schema,pool:{min:1,max:1}});
const context=callback=>core.RequestContext.create(orm.em,()=>persistence.withEncryptionContext('team-quota-fixture',callback));
const error=console.error;console.error=()=>{};
try {
 await orm.schema.create();
 const ref=await betterAuthReference(root,{dependencies:deps,orm:createAuthEncryptionOrm(orm),entities,persistence});
 const client=ref.client();
 const signed=await context(()=>client.call('/sign-up/email',{name:'Owner',firstName:'Owned',lastName:'User',email:'owner@quota-orm.test',password:'owned-team-password'}));assert.equal(signed.status,200);
 const created=await context(()=>client.call('/organization/create',{name:'Quota ORM',slug:'quota-orm',domain:'quota-orm.test'}));assert.equal(created.status,200);
 const cookie=[...client.cookies].map(([k,v])=>`${k}=${v}`).join('; ');
 let maximum=1,calls=0,currentKind='';
 const auth=betterAuth({...ref.config,secret:'better-auth-protocol-fixture-secret-32-characters',logger:{disabled:true},rateLimit:{enabled:false},plugins:ref.config.plugins.map(p=>p.id==='organization'?organization({...p.options,teams:{...p.options.teams,maximumMembersPerTeam:async({teamId})=>{calls++;if(currentKind==='deleted')await adapter.delete({model:'team',where:[{field:'id',value:teamId}]});return maximum;}}}):p)});
 const adapter=(await auth.$context).adapter;
 await context(()=>adapter.create({model:'organizationRole',data:{organizationId:created.body.id,role:'owner',permission:JSON.stringify({team:['create','delete','update'],member:['create','update'],invitation:['create']}),createdAt:new Date()}}));
 const results=[];
 for(const kind of ['team','invite','accept','member','duplicate','deleted']) for(const [label,limit]of [['zero',0],['negative',-1],['half',0.5],['one',1],['nan',NaN],['infinity',Infinity],['negative-infinity',-Infinity],['i32-max',2147483647],['u32',4294967296]]) {
  maximum=limit;calls=0;currentKind=kind;
  const seeded=await context(async()=>{
   const user=await adapter.create({model:'user',data:{name:'Target',firstName:'Target',lastName:'Owned',email:crypto.randomUUID()+'@quota-orm.test',emailVerified:true,createdAt:new Date(),updatedAt:new Date()}});
   if(['team','duplicate','deleted'].includes(kind)) await adapter.create({model:'member',data:{organizationId:created.body.id,userId:user.id,role:'member',createdAt:new Date()}});
   const team=await adapter.create({model:'team',data:{organizationId:created.body.id,name:'Owned',memberCount:0,createdAt:new Date()}});
   if(kind==='duplicate') await adapter.create({model:'teamMember',data:{teamId:team.id,userId:user.id,createdAt:new Date()}});
   if(kind==='accept') {
    const invitation=await adapter.create({model:'invitation',data:{organizationId:created.body.id,email:user.email,role:'member',inviterId:signed.body.user.id,teamId:team.id,status:'pending',expiresAt:new Date(Date.now()+3600000),createdAt:new Date()}});
    const session=await adapter.create({model:'session',data:{userId:user.id,token:crypto.randomUUID(),expiresAt:new Date(Date.now()+3600000),createdAt:new Date(),updatedAt:new Date()}});
    return {user,team,invitation,session};
   }
   return {user,team};
  });
  let response;
  if(kind==='member') { try {response=await context(()=>auth.api.addMember({body:{organizationId:created.body.id,teamId:seeded.team.id,userId:seeded.user.id,role:'member'},headers:new Headers({cookie}),asResponse:true}));}catch {response=new Response(null,{status:500});} }
  else {
   let path='/organization/add-team-member',requestCookie=cookie,body={organizationId:created.body.id,teamId:seeded.team.id,userId:seeded.user.id};
   if(kind==='invite'){path='/organization/invite-member';body={organizationId:created.body.id,teamId:seeded.team.id,email:seeded.user.email,role:'member'};}
   if(kind==='accept'){
    path='/organization/accept-invitation';body={invitationId:seeded.invitation.id};
    const signature=crypto.createHmac('sha256','better-auth-protocol-fixture-secret-32-characters').update(seeded.session.token).digest('base64');
    requestCookie='better-auth.session_token='+encodeURIComponent(seeded.session.token+'.'+signature);
   }
   response=await context(()=>auth.handler(new Request('http://localhost:3000/api/auth'+path,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie:requestCookie},body:JSON.stringify(body)})));
  }
  const text=await response.text();let body;try{body=JSON.parse(text);}catch{};
  assert.equal(calls,1,JSON.stringify({kind,label,status:response.status,code:body?.code,message:body?.message}));
  const expected=kind==='duplicate'?200:kind==='invite'?(limit<=0?403:200):!Number.isFinite(limit)?500:kind==='deleted'||limit<=0?403:200;
  assert.equal(response.status,expected,JSON.stringify({kind,label,status:response.status}));
  if(expected===403)assert.equal(body.code,'TEAM_MEMBER_LIMIT_REACHED');
  if(expected===500)assert.equal(text,'');
  const state=await context(async()=>({
   pairs:(await adapter.findMany({model:'teamMember',where:[{field:'teamId',value:seeded.team.id}]})).length,
   member:!!await adapter.findOne({model:'member',where:[{field:'organizationId',value:created.body.id},{field:'userId',value:seeded.user.id}]}),
   invitation:kind==='accept'?(await adapter.findOne({model:'invitation',where:[{field:'id',value:seeded.invitation.id}]}))?.status:null
  }));
  assert.equal(state.pairs,kind==='duplicate'||(kind!=='invite'&&expected===200)?1:0);
  assert.equal(state.member,['team','duplicate','deleted'].includes(kind)||(kind!=='invite'&&expected===200));
  if(kind==='accept')assert.equal(state.invitation,expected===200?'accepted':'pending');
  results.push({kind,label,status:response.status,code:body?.code??null,...state});
 }
 assert.equal(results.length,54);console.log('PASS configured ORM team quota '+results.length,JSON.stringify(results));
} finally {await orm.em.getConnection().execute(`DROP SCHEMA IF EXISTS "${schema}" CASCADE`);await orm.close(true);console.error=error;console.log('CLEANED '+schema);}
