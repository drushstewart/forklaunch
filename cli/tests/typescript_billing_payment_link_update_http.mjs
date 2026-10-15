// Runs the configured application mappers and their real Zod schema factories.
// ORM entity allocation/storage is owned by this fixture; no ORM decorator emulation.
import assert from 'node:assert/strict';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID} from 'node:crypto';
import {actualValidator} from './rust_actual_validator_fixture.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');

import {createRequire} from 'node:module';
import {readFileSync,realpathSync} from 'node:fs';
import vm from 'node:vm';
const variant='stripe';
 let actual;
 class FixtureEntityManager {}
 const entities=new Map();
 actual=actualValidator(root,(spec,file,load)=>{
  if(spec==='@forklaunch/validator/zod')return actual.statics;
  if(spec==='@forklaunch/blueprint-core')return {schemaValidator:actual.validator};
  if(spec==='@forklaunch/core/http')return {evaluateTelemetryOptions:()=>({enabled:{}})};
  if(spec==='@forklaunch/core/cache')return {createCacheKey:prefix=>id=>prefix+':'+id};
  if(spec==='@forklaunch/implementation-billing-base/services')return {...load(path.join(root,'blueprint/implementations/billing/base/services/billingPortal.service.ts')),...load(path.join(root,'blueprint/implementations/billing/base/services/checkoutSession.service.ts')),...load(path.join(root,'blueprint/implementations/billing/base/services/paymentLink.service.ts')),...load(path.join(root,'blueprint/implementations/billing/base/services/plan.service.ts')),...load(path.join(root,'blueprint/implementations/billing/base/services/subscription.service.ts'))};
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
  if(spec==='@mikro-orm/core')return {EntityManager:FixtureEntityManager,wrap:entity=>({toPOJO:()=>({...entity})})};
 });



const require=createRequire(import.meta.url);
const dependencies=process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES;
assert.ok(dependencies,'provide actual canonical dependencies');
const express=require(path.join(dependencies,'node_modules/express'));
const ts=require(require.resolve('typescript',{paths:[path.dirname(realpathSync(process.env.TSC_BIN))]}));
const file=path.join(root,'framework/express/src/expressApplication.ts');
const source=ts.createSourceFile(file,readFileSync(file,'utf8'),ts.ScriptTarget.Latest,true);
let initializer;
function visit(node){if(ts.isVariableDeclaration(node)&&node.name.getText(source)==='errorHandler')initializer=node.initializer;ts.forEachChild(node,visit);}
visit(source);assert.ok(initializer);
const printed=ts.createPrinter().printNode(ts.EmitHint.Expression,initializer,source);
const compiled=ts.transpileModule(`exports.handler=(function(){return ${printed};}).call({openTelemetryCollector:{error(){}}});`,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
function actualModule(relative,imports={}) {
 const exports={};const code=ts.transpileModule(readFileSync(path.join(root,relative),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
 vm.runInNewContext(code,{exports,Date,require(spec){if(Object.hasOwn(imports,spec))return imports[spec];return require(path.join(dependencies,'node_modules',spec));}});return exports;
}
const guard=actualModule('framework/core/src/http/guards/isForklaunchRequest.ts');
const contextModule=actualModule('framework/core/src/http/middleware/request/createContext.middleware.ts',{
 'uuid':{v4:randomUUID}, // Owned entropy primitive; actual middleware chooses/propagates it.
 '@forklaunch/common':{getEnvVar:()=>undefined},
 '../../telemetry/constants':{ATTR_CORRELATION_ID:'correlation.id',ATTR_SERVICE_NAME:'service.name'},
 '../../telemetry/openTelemetryCollector':{httpRequestsInFlightCounter:{add(){}}}
});
const box={exports:{},express,console:{error(){}},isForklaunchRequest:guard.isForklaunchRequest,meta:value=>value,ATTR_HTTP_RESPONSE_STATUS_CODE:'http.response.status_code'};
vm.runInNewContext(compiled,box);
const app=express();app.use(express.json());
app.use((req,res,next)=>{if(req.headers['x-owned-context']==='yes'){req.contractDetails={};return contextModule.createContext(actual.validator)(req,res,next);}next();});

const Stripe=require(process.env.STRIPE_SDK_PATH);
const mappers=actual.load(path.join(root,'blueprint/billing-stripe/domain/mappers/paymentLink.mappers.ts'));
const Service=actual.load(path.join(root,'blueprint/implementations/billing/stripe/services/paymentLink.service.ts')).StripePaymentLinkService;
const id=randomUUID();
const good={id,amount:12,currency:'USD',paymentMethods:['card'],status:'PENDING',stripeFields:{id:'plink_retained',object:'payment_link',livemode:false},createdAt:'2026-01-01T00:00:00Z'};
let scenario='missing',providerCalls=0,cacheWrites=0,reads=0,written,providerFields;
const cache={async readRecord(){reads++;if(scenario==='missing')return null;if(scenario==='store')throw Error('private cache');return {value:{...good,...(scenario==='cache-id'?{id:'other'}:{}),...(scenario==='cache-date'?{createdAt:'bad-date'}:{})}};},getTtlMilliseconds:()=>60000,async putRecord({value}){cacheWrites++;if(scenario!=='success')throw Error('unexpected write');written=value;}};
const client={paymentLinks:{async update(target,fields){providerCalls++;providerFields=structuredClone(fields);assert.equal(target,id,'retained provider reference must not change public mutation dispatch');if(scenario==='success')return {id:target,object:'payment_link',line_items:{data:[{amount_total:34}]}};if(scenario==='timeout')throw new Stripe.errors.StripeConnectionError({message:'private timeout'});throw new Stripe.errors.StripeInvalidRequestError({message:'private provider detail',statusCode:Number(scenario)});}}};
const service=new Service(client,{},cache,{},actual.validator,mappers);
app.put('/paymentLink/:id',async(req,res,next)=>{try{res.json(await service.updatePaymentLink({...req.body,id:req.params.id}));}catch(error){next(error);}});
app.use(box.exports.handler);
const server=await new Promise(resolve=>{const server=app.listen(0,'127.0.0.1',()=>resolve(server));});
try {
 let cases=0;
 const failures=[['missing',{}],['store',{}],['cache-id',{}],['cache-date',{}],['patch-status',{status:'INVALID'}],['patch-currency',{currency:'INVALID'}],['patch-method',{paymentMethods:['INVALID']}],['patch-amount',{amount:'bad'}],['patch-null',{amount:null}],['timeout',{}],['404',{}],['429',{}],['500',{}]];
 for(const [failure,patch] of failures)for(const correlation of [undefined,'update-correlation','']) {
  scenario=failure;providerCalls=0;cacheWrites=0;reads=0;
  const headers={'x-owned-context':'yes','content-type':'application/json'};if(correlation!==undefined)headers['x-correlation-id']=correlation;
  const response=await fetch(`http://127.0.0.1:${server.address().port}/paymentLink/${id}`,{method:'PUT',headers,body:JSON.stringify(patch)});
  const expected=['404','429','500'].includes(failure)?Number(failure):500;
  assert.equal(response.status,expected);assert.match(response.headers.get('content-type'),/^text\/plain/);
  const observed=response.headers.get('x-correlation-id');if(correlation)assert.equal(observed,correlation);else assert.match(observed,/^[a-f0-9-]{36}$/);
  assert.equal(await response.text(),`${expected>=500?'Internal server error':'Request could not be processed'}:\n\nCorrelation id: ${observed}`);
  assert.equal(providerCalls,['patch-null','timeout','404','429','500'].includes(failure)?1:0,failure);assert.equal(reads,1);assert.equal(cacheWrites,0);cases++;
 }
 // Execute the actual successful parse middleware and controller callback.
 // The handler registry/bootstrap itself is not instantiated by this fixture.
 const requestShape=actualModule('framework/core/src/http/guards/isRequestShape.ts');
 const parser=actualModule('framework/core/src/http/middleware/request/parse.middleware.ts',{
  '@forklaunch/common':{isRecord(){throw Error('unexpected versioned path');}},
  '@forklaunch/validator':{prettyPrintParseErrors(){throw Error('unexpected invalid parse');}},
  '../../guards/isRequestShape':requestShape,
  '../../guards/hasSend':actualModule('framework/core/src/http/guards/hasSend.ts'),
  '../internal/validationDiagnostic':{sendValidationDiagnostic(){throw Error('unexpected invalid parse');}}
 });
 const controllerPath=path.join(root,'blueprint/billing-stripe/api/controllers/paymentLink.controller.ts');
 const controllerSource=ts.createSourceFile(controllerPath,readFileSync(controllerPath,'utf8'),ts.ScriptTarget.Latest,true);let updateDeclaration;
 function findController(node){if(ts.isVariableDeclaration(node)&&node.name.getText(controllerSource)==='updatePaymentLink')updateDeclaration=node;ts.forEachChild(node,findController);}
 findController(controllerSource);assert.ok(updateDeclaration);assert.equal(updateDeclaration.initializer.arguments[2].properties.find(p=>p.name.getText(controllerSource)==='body').initializer.getText(controllerSource),'UpdatePaymentLinkMapper.schema');
 const callback=ts.createPrinter().printNode(ts.EmitHint.Expression,updateDeclaration.initializer.arguments[3],controllerSource);
 const controllerBox={exports:{},serviceFactory:()=>service,openTelemetryCollector:{debug(){}}};vm.runInNewContext(ts.transpileModule(`exports.handler=${callback};`,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,controllerBox);
 for(const amount of [null,'12',false,[]]) {
  scenario='success';providerCalls=0;cacheWrites=0;written=undefined;
  const req={params:{id},body:{id,amount},query:{},headers:{},contractDetails:{},schemaValidator:actual.validator,requestSchema:actual.validator.schemify({body:actual.validator.schemify(mappers.UpdatePaymentLinkMapper.schema),params:actual.validator.schemify({id:actual.statics.string})})};
  let parsed=false;parser.parse(req,{},()=>{parsed=true;});assert.ok(parsed);assert.equal(req.body.amount,Number(amount));assert.equal(req._rawBody.amount,amount);
  let returned;const res={status(value){assert.equal(value,200);return this;},json(value){returned=value;}};await controllerBox.exports.handler(req,res);
  assert.equal(providerCalls,1);assert.equal(providerFields.amount,undefined);assert.equal(written.amount,34);assert.equal(returned.amount,34);assert.equal(cacheWrites,1);cases++;
 }
 console.log(`PASS ${cases} actual configured PaymentLink update service/mapper + actual createContext/final Express handler: cached/missing/invalid patch preflight0provider writes; null amount is admitted before owned provider failure; SDK error-class status404/429/500/connection retained; target remains public ID. Error transport route skips request admission; additional actual parse+controller successes prove null/numeric-string/bool/array amount becomes Number(input), provider ignores requested amount, and cached/returned amount becomes provider sum34. No full bootstrap claim.`);
}finally{await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
