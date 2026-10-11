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
  if(spec==='@mikro-orm/core')return {wrap:entity=>({toPOJO:()=>({...entity})})};
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
console.log(`PASS ${cases} actual configured billing mapper cases: schema resolution, HTTP-boundary unknown fields, optional provider payloads, local UUIDs, response mapping, ownership and missing-row failures`);
console.log('AUDIT Stripe checkout/payment-link/portal configured TS mappers discard provider IDs; native cache services retain provider IDs so later provider calls remain valid. Native provider payloads require objects and native scalar coercions remain stricter.');
