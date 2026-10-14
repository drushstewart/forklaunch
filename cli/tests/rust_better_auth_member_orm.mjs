// Actual configured Better Auth + encrypted MikroORM/PostgreSQL member queries.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {readFileSync,writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {isDeepStrictEqual} from 'node:util';
import {betterAuthOrm} from './fixtures/rust_better_auth_orm.mjs';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const dependencies=process.env.FORKLAUNCH_TEST_ORM_DEPENDENCIES;
assert.match(readFileSync(path.join(root,'cli/src/templates/rust/iam-better-auth/main.rs'),'utf8'), /with_member_query_profile\(MemberQueryProfile::Postgres\)/);
let binary=process.env.FORKLAUNCH_TEST_IAM_MEMBER_BINARY;
if(!binary) {
 execFileSync('cargo',['build','-p','forklaunch-iam-better-auth','--features','http,postgres','--example','member_orm_parity'],{cwd:path.join(root,'blueprint-rs'),env:{...process.env,CARGO_INCREMENTAL:'0',CARGO_BUILD_JOBS:'2'},stdio:['ignore','inherit','inherit']});
 binary=path.join(root,'blueprint-rs/target/debug/examples/member_orm_parity');
}

const source=await betterAuthOrm(root,dependencies);
const {persistence,entities,core}=source;
const {createAuthEncryptionOrm}=source.load('blueprint/iam-better-auth/domain/utils/authEncryptionPolicy.util.ts');
persistence.registerEncryptor(new persistence.FieldEncryptor('actual-member-orm-fixture-encryption-key'));
const schema=`fl_member_orm_${crypto.randomBytes(8).toString('hex')}`;
const orm=await source.MikroORM.init({entities:Object.values(entities),clientUrl:process.env.FORKLAUNCH_TEST_POSTGRES_URL,schema,pool:{min:1,max:1}});
const context=callback=>core.RequestContext.create(orm.em,()=>persistence.withEncryptionContext('member-orm-fixture',callback));
try {
 await orm.schema.create();
 if(process.env.FORKLAUNCH_TEST_MEMBER_TIMEZONE) {
  const timezone=process.env.FORKLAUNCH_TEST_MEMBER_TIMEZONE;
  await orm.em.getConnection().execute("select set_config('TimeZone', ?, false)",[timezone]);
  const actual=await orm.em.getConnection().execute('show TimeZone');
  assert.equal(actual[0].TimeZone,timezone);
  console.error(`Actual ORM session TimeZone: ${actual[0].TimeZone}`);
 }

 const reference=await betterAuthReference(root,{dependencies,orm:createAuthEncryptionOrm(orm),entities,persistence});
 const client=reference.client(),auth=await reference.auth.$context;
 const call=route=>context(()=>client.call(route));
 const signed=await context(()=>client.call('/sign-up/email',{name:'Owner',firstName:'Owner',lastName:'Fixture',email:'owner@member-orm.test',password:'owned-member-password'}));
 assert.equal(signed.status,200);
 const organization=await context(()=>client.call('/organization/create',{name:'Member ORM',slug:'member-orm',domain:'member-orm.test'}));
 assert.equal(organization.status,200);
 const roles=['a','A','á','😀','','z','a%','a_','a\\'];
 await context(async()=>{
  for(const [index,role] of roles.entries()) {
   const now=new Date(`2026-01-${String(index+1).padStart(2,'0')}T01:02:03.000Z`);
   const user=await auth.adapter.create({model:'user',data:{name:`User ${index}`,firstName:'User',lastName:String(index),email:`${index}@member-orm.test`,emailVerified:false,createdAt:now,updatedAt:now}});
   await auth.adapter.create({model:'member',data:{userId:user.id,organizationId:organization.body.id,role,createdAt:now}});
  }
 });
 const seed={op:'replace-seed',now:Date.now()};
 for(const [model,field] of [['user','users'],['account','accounts'],['session','sessions'],['organization','organizations'],['member','members']])seed[field]=await context(()=>auth.adapter.findMany({model,limit:1000}));
 const cases=[];
 const add=(name,query)=>cases.push({name,query});
 const values={id:[seed.members[1].id,seed.members[1].id.toUpperCase(),crypto.randomUUID(),'bad-id',''],organizationId:[organization.body.id,organization.body.id.toUpperCase(),'missing',''],userId:[signed.body.user.id,seed.users[1].id,seed.users[1].id.toUpperCase(),'missing',''],role:['a','A','á','%','_','a%','a_','a\\',''],createdAt:['2026-01-01T01:02:03.000Z','2026-01-01T02:02:03+01:00','2026-01-03','bad-date','']};
 for(const [field,items] of Object.entries(values))for(const operator of ['eq','ne','contains','starts_with','ends_with','lt','lte','gt','gte','in','not_in'])for(const value of items)add(`${field}/${operator}/${value}`,{filterField:field,filterOperator:operator,filterValue:value});
 for(const [field,items] of Object.entries(values))for(const operator of ['eq','ne','in','not_in','contains','lt'])add(`${field}/${operator}/array`,{filterField:field,filterOperator:operator,filterValue:items.slice(0,2)});
 for(const [field,items] of Object.entries(values))for(const operator of ['ne','lt','lte','gt','gte','contains','starts_with','ends_with','in','not_in'])add(`${field}/${operator}/missing`,{filterField:field,filterOperator:operator});
 for(const value of ['-1','-0.1','-0.49','-0.5','-0.51','0','0.5','1.49','1.5','2.5','NaN','Infinity','bad','4097'])for(const field of ['limit','offset'])add(`paging/${field}/${value}`,{[field]:value,sortBy:'createdAt'});
 for(const field of Object.keys(values)) {
  add(`${field}/missing-eq`,{filterField:field});
  for(const sortDirection of ['asc','desc'])add(`sort/${field}/${sortDirection}`,{sortBy:field,sortDirection});
 }
 for(const sortBy of ['unknown','role'])add(`empty-sort/${sortBy}`,{filterField:'role',filterValue:'absent',sortBy});
 add('single unknown sort',{filterField:'role',filterValue:'a',sortBy:'unknown'});
 const results=[];
 for(const item of cases) {
  const query=new URLSearchParams();for(const [key,value] of Object.entries(item.query))for(const item of Array.isArray(value)?value:[value])query.append(key,item);
  item.path=`/organization/list-members?${query}`;
  const response=await call(item.path);
  results.push({status:response.status,body:response.body});
 }
 const cookie=[...client.cookies].map(([key,value])=>`${key}=${value}`).join('; ');
 const inputs=[seed,...cases.map(item=>({op:'request',now:Date.now(),path:item.path,method:'GET',headers:{cookie,origin:'http://localhost:3000'},body:''}))];

 let differences=[];
 if(binary) {
  const output=execFileSync(binary,[],{cwd:root,input:inputs.map(item=>JSON.stringify(item)).join('\n')+'\n',encoding:'utf8',maxBuffer:64*1024*1024});
  const native=output.trim().split('\n').map(line=>JSON.parse(line)).slice(1);
  assert.equal(native.length,results.length);
  for(const [index,result] of results.entries())if(!isDeepStrictEqual(result,{status:native[index].status,body:native[index].body}))differences.push({name:cases[index].name,actual:result,native:{status:native[index].status,body:native[index].body}});
 }
 if(process.env.FORKLAUNCH_TEST_IAM_MEMBER_CAPTURE)writeFileSync(process.env.FORKLAUNCH_TEST_IAM_MEMBER_CAPTURE,JSON.stringify({cases,results,inputs,differences},null,2));
 if(process.env.FORKLAUNCH_TEST_IAM_MEMBER_CHARACTERIZE !== '1')assert.deepEqual(differences,[], 'actual PostgreSQL member query parity');
 console.log(`Actual configured member ORM matrix: ${cases.length} cases, ${differences.length} native differences`);
} finally {await orm.schema.drop({dropDb:false,dropMigrationsTable:true});await orm.close(true);}
