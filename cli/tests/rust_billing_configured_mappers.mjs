// Runs the configured application mappers and their real Zod schema factories.
// ORM entity allocation/storage is owned by this fixture; no ORM decorator emulation.
import assert from 'node:assert/strict';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID} from 'node:crypto';
import {actualValidator} from './rust_actual_validator_fixture.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const families=['plan','subscription','checkoutSession','paymentLink','billingPortal'];
let cases=0;
for(const variant of ['base','stripe']){
 let actual;
 const entities=new Map();
 actual=actualValidator(root,(spec,file,load)=>{
  if(spec==='@forklaunch/validator/zod')return actual.statics;
  if(spec==='@forklaunch/blueprint-core')return {schemaValidator:actual.validator};
  if(spec==='@forklaunch/core/http')return {evaluateTelemetryOptions:()=>({enabled:{}})};
  if(spec==='@forklaunch/core/cache')return {createCacheKey:prefix=>id=>prefix+':'+id};
  if(spec==='@forklaunch/core/mappers')return {...load(path.join(root,'framework/core/src/mappers/mapper.ts')),...load(path.join(root,'framework/core/src/mappers/mapServiceSchemas.ts'))};
  if(spec==='@forklaunch/internal')return load(path.join(root,'framework/internal/src/serviceSchemaResolver.ts'));
  if(spec===`@forklaunch/implementation-billing-${variant}/schemas`)return load(path.join(root,`blueprint/implementations/billing/${variant}/domain/schemas/index.ts`));
  // These imports are present in the resolver barrel but this fixture selects Zod.
  if(spec.startsWith('./typebox/'))return {};
  if(spec.includes('/persistence/entities/')){
   const name=path.basename(spec,'.entity'),exportName=name[0].toUpperCase()+name.slice(1);
   if(!entities.has(name))entities.set(name,{[exportName]:{name}});
   return entities.get(name);
  }
  if(spec==='@mikro-orm/core')return {EntityManager:class {},wrap:entity=>({toPOJO:()=>({...entity})})};
 });
 const samples={
  plan:{name:'Configured plan',price:1200,cadence:variant==='base'?'MONTHLY':'month',currency:'USD',externalId:'prod_fixture',billingProvider:'stripe',active:true},
  subscription:{partyId:'customer',partyType:'user',productId:'product',active:true,externalId:'sub_fixture',startDate:new Date('2026-01-01T00:00:00Z'),status:'active',billingProvider:'stripe'},
  checkoutSession:{customerId:'customer',currency:'USD',paymentMethods:[variant==='base'?'credit_card':'card'],uri:'https://fixture.test/checkout',expiresAt:new Date('2026-12-01T00:00:00Z'),status:'PENDING'},
  paymentLink:{amount:1200,currency:'USD',paymentMethods:[variant==='base'?'credit_card':'card'],status:'PENDING'},
  billingPortal:{customerId:'customer',expiresAt:new Date('2026-12-01T00:00:00Z'),uri:'https://fixture.test/portal'}
 };
 for(const family of families){
  const name=family[0].toUpperCase()+family.slice(1),mappers=actual.load(path.join(root,`blueprint/billing-${variant}/domain/mappers/${family}.mappers.ts`));
  const create=mappers[`Create${name}Mapper`],update=mappers[`Update${name}Mapper`],read=mappers[`${name}Mapper`];
  const records=new Map(),em={
   create(_entity,fields){const entity={id:randomUUID(),...fields};records.set(entity.id,entity);return entity;},
   async findOneOrFail(_entity,{id}){assert.ok(records.has(id),'configured update requires persisted entity');return records.get(id);},
   assign(entity,fields){Object.assign(entity,fields);}
  };
  function parse(mapper,dto){const parsed=actual.validator.parse(actual.validator.schemify(mapper.schema),dto);assert.equal(parsed.ok,true,JSON.stringify(parsed.errors));return parsed.value;}
  const input=parse(create,{...samples[family],undeclaredField:'strip-at-http-boundary'});
  assert.equal(input.undeclaredField,undefined);cases++;
  // Provider fields are optional in the actual runtime validator, even for Stripe.
  const entity=await create.toEntity(input,em,variant==='stripe'?{id:'provider_fixture'}:undefined);
  assert.match(entity.id,/^[a-f0-9-]{36}$/);assert.equal(entity.undeclaredField,undefined);
  assert.equal(entity.customerId??entity.partyId??entity.name,samples[family].customerId??samples[family].partyId??samples[family].name);cases++;
  const dto=await read.toDto(entity);
  if(variant==='stripe')assert.equal(dto.stripeFields.id,'provider_fixture');
  if(family==='plan')assert.equal(dto.price,1200);
  if(family==='paymentLink')assert.equal(dto.amount,1200);cases++;
  if(['checkoutSession','billingPortal'].includes(family)){
   const Service=actual.load(path.join(root,`blueprint/implementations/billing/base/services/${family}.service.ts`))[`Base${name}Service`];
   const suffix=family==='billingPortal'?'BillingPortalSession':name;
   for(const enableDatabaseBackup of [false,true]){
    let writes=0,persisted=0;
    const {safeStringify}=actual.load(path.join(root,'framework/common/src/safeStringify.ts'));
    const {safeParse}=actual.load(path.join(root,'framework/common/src/safeParse.ts'));
    const cacheRows=new Map(),cache={getTtlMilliseconds:()=>60000,async putRecord({key,value}){writes++;cacheRows.set(key,safeStringify(value));},async readRecord(key){return cacheRows.has(key)?{value:safeParse(cacheRows.get(key))}:null;}};
    const readOnlyEm={create:em.create,persist(){persisted++;return {async flush(){}};},async findOneOrFail(){throw Error('cache read must not query a database entity');},assign(){throw Error('cache read must not mutate an entity');}};
    const service=new Service(readOnlyEm,cache,{},actual.validator,mappers,{enableDatabaseBackup});
    const provider={id:'provider_cache_fixture',url:'https://fixture.test/provider',opaqueTimestamp:'2026-01-01T00:00:00.000Z'};
    const created=await service[`create${suffix}`](samples[family],provider);
    const cached=await service[`get${suffix}`]({id:created.id});
    assert.deepEqual(JSON.parse(JSON.stringify(cached)),JSON.parse(JSON.stringify(created)));assert.equal(writes,1);assert.equal(persisted,Number(enableDatabaseBackup));cases++;
    assert.ok(cached.expiresAt instanceof Date);assert.notEqual(cached.expiresAt,created.expiresAt);assert.equal(cached.expiresAt.getTime(),created.expiresAt.getTime());
    if(variant==='stripe')assert.deepEqual(JSON.parse(JSON.stringify(cached.stripeFields)),provider);
    const key=[...cacheRows.keys()][0],serialized=cacheRows.get(key),withTiming={...JSON.parse(serialized),createdAt:'2026-01-01T00:00:00.000Z',updatedAt:1767225600000};
    cacheRows.set(key,JSON.stringify(withTiming));
    const timed=await service[`get${suffix}`]({id:created.id});
    assert.ok(timed.createdAt instanceof Date);assert.ok(timed.updatedAt instanceof Date);assert.equal(timed.createdAt.getTime(),1767225600000);assert.equal(timed.updatedAt.getTime(),1767225600000);assert.equal(cacheRows.get(key),JSON.stringify(withTiming));cases++;
    for(const invalid of [undefined,null,false,{},'not-a-date','9999999999999999999999']){
     cacheRows.set(key,JSON.stringify({...withTiming,expiresAt:invalid}));
     await assert.rejects(service[`get${suffix}`]({id:created.id}));cases++;
    }
    for(const field of ['createdAt','updatedAt']){
     cacheRows.set(key,JSON.stringify({...withTiming,[field]:'not-a-date'}));
     await assert.rejects(service[`get${suffix}`]({id:created.id}));cases++;
    }
    cacheRows.set(key,serialized);
    await assert.rejects(service[`get${suffix}`]({id:randomUUID()}));cases++;
   }
  }
  if(['checkoutSession','paymentLink'].includes(family)){
   // Exercise actual configured schemas and mapper calls for every retained and
   // emitted status, rather than substituting a permissive fixture enum.
   for(const status of ['PENDING','SUCCESS','CREATED','UPDATED','COMPLETED','CANCELLED','FAILED','EXPIRED']){
    const created=await create.toEntity(parse(create,{...samples[family],status}),em,{id:'provider_fixture'});
    assert.equal(parse(read,await read.toDto(created)).status,status);cases++;
    const updated=await update.toEntity(parse(update,{id:entity.id,status}),em,{id:'provider_fixture'});
    assert.equal(parse(read,await read.toDto(updated)).status,status);cases++;
   }
   for(const [mapper,input] of [[create,{...samples[family],status:'UNKNOWN'}],[update,{id:entity.id,status:'UNKNOWN'}],[read,{...dto,status:'UNKNOWN'}]]){
    assert.equal(actual.validator.parse(actual.validator.schemify(mapper.schema),input).ok,false);cases++;
   }
  }
  const patch=parse(update,{id:entity.id,undeclaredField:'strip',...(family==='billingPortal'?{customerId:'must-not-reassign',uri:'https://fixture.test/updated'}:{})});
  assert.equal(patch.undeclaredField,undefined);
  if(family==='billingPortal')assert.equal(patch.customerId,undefined);
  const changed=await update.toEntity(patch,em,variant==='stripe'?{id:'provider_updated'}:undefined);
  assert.equal(changed.id,entity.id);
  if(family==='billingPortal')assert.equal(changed.customerId,'customer');cases++;
  await assert.rejects(update.toEntity({...patch,id:randomUUID()},em,{}));cases++;
  const missing={...samples[family]};delete missing[Object.keys(missing)[0]];
  await assert.rejects(create.toEntity(missing,em,{}));cases++;
  if(variant==='stripe'){
   const providerIdInput=parse(create,{...samples[family],id:'provider_supplied_id'});
   const local=await create.toEntity(providerIdInput,em,{id:'provider_supplied_id'});
   assert.notEqual(local.id,'provider_supplied_id');cases++;
  }
 }
}
console.log(`PASS ${cases} actual configured billing mapper cases: cache DTO reads without ORM access, emitted/retained status acceptance, unknown-status rejection, schema resolution, HTTP-boundary unknown fields, optional provider payloads, local UUIDs, response mapping, ownership and missing-row failures`);
console.log('AUDIT Stripe checkout/payment-link/portal configured TS mappers discard provider IDs; native cache services retain provider IDs so later provider calls remain valid. Native provider payloads require objects and native scalar coercions remain stricter.');
