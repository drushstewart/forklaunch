import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {actualValidator} from './rust_actual_validator_fixture.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const {validator,load}=actualValidator(root);
const require=createRequire(path.join(process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES??'/private/tmp/fl-canonical-http-dependencies','package.json'));
const {z}=require('zod/v3'),Ajv=require('ajv');
const socketRequire=createRequire(path.join(process.env.FORKLAUNCH_TEST_SOCKET_DEPENDENCIES??'/private/tmp/fl-binary-socket-dependencies','package.json'));
let AjvDraft4;try{AjvDraft4=require('ajv-draft-04')}catch(error){if(error.code!=='MODULE_NOT_FOUND')throw error;AjvDraft4=socketRequire('ajv-draft-04')}
const {generateSchema}=load(path.join(root,'framework/validator/shims/zod-v3-openapi/zod-openapi.ts'));
// Finite JSON union acceptance; this does not encode coercion or transforms.
let comparisons=0,exports=0;
const values=[null,false,true,-10,-0,0,1,5,10,20,'','a','same','😀',[],{}, {kind:'a'},[1]];
const shapes=[x=>x,x=>x.optional(),x=>z.object({value:x}),x=>z.array(x)];
function check(label,schema){
 for(let i=0;i<shapes.length;i++)for(const version of ['3.0','3.1']){
  const wrapped=shapes[i](schema),exported=generateSchema(wrapped,false,version);
  const validate=(version==='3.0'?new AjvDraft4({strict:false}):new Ajv({strict:false})).compile(exported);exports++;
  for(const value of values){const input=i===2?{value}:i===3?[value]:value;
   assert.equal(validate(input),validator.parse(wrapped,input).ok,JSON.stringify({label,version,i,input,exported}));comparisons++;
  }
 }
}
// Factory annotations must not narrow unconstrained runtime validators to objects.
for(const [name,source] of [['any',validator.any],['unknown',validator.unknown],['type',validator.type()]]){
 check('factory '+name,source);
 for(const version of ['3.0','3.1']){
  const exported=generateSchema(source,false,version);
  assert.equal(exported.type,undefined,name+' has no object-only restriction');
  const override=generateSchema(source.openapi({type:'object'}),false,version);
  assert.equal(override.type,'object','explicit application overrides remain authoritative');
 }
}
check('empty',validator.enum_({}));
for(const entries of [[],['same'],['same','same'],['a','same','a'],[0,-0,1,1],[true,false,true],['a',1,true],['a',1,'same'],[false,0,false]]){
 check('enum '+JSON.stringify(entries),validator.enum_(Object.fromEntries(entries.map((v,i)=>[i,v]))));
}
const branches=[z.string(),z.string().min(1),z.number(),z.number().min(0),z.number().max(10),z.boolean(),z.unknown(),z.object({kind:z.literal('a')})];
for(const left of branches)for(const right of branches)check('ordinary pair',z.union([left,right]));
for(const version of ['3.0','3.1']){
 const branches=[z.number().min(0),z.number().max(10)];
 const generated=branches.map(x=>generateSchema(x,false,version));
 assert.equal(JSON.stringify(generateSchema(z.union(branches).openapi({oneOf:[{type:'boolean'}]}),false,version)),JSON.stringify({oneOf:[...generated,{type:'boolean'}]}));
 assert.equal(JSON.stringify(generateSchema(z.union(branches).openapi({anyOf:[{type:'boolean'}]}),false,version)),JSON.stringify({oneOf:generated,anyOf:[{type:'boolean'}]}));
 const discriminated=generateSchema(z.discriminatedUnion('kind',[z.object({kind:z.literal('a')}),z.object({kind:z.literal('b')})]),false,version);
 assert.ok(discriminated.oneOf);assert.equal(discriminated.anyOf,undefined);assert.equal(discriminated.discriminator.propertyName,'kind');
 const nonfinite=generateSchema(validator.enum_({a:Infinity,b:-Infinity}),false,version);
 assert.deepEqual(Array.from(nonfinite.enum),[Infinity,-Infinity]); // Explicit unchanged non-JSON boundary.
}
// Nullable union is representable in 3.1; existing 3.0 nullable-combinator
// limitation remains separate, rather than claiming a dialect migration here.
const nullable=z.union([z.string(),z.null()]);
const nullableSchema=generateSchema(nullable,false,'3.1');const nullableCheck=new Ajv({strict:false}).compile(nullableSchema);
for(const value of values){assert.equal(nullableCheck(value),validator.parse(nullable,value).ok);comparisons++;}
assert.throws(()=>new AjvDraft4({strict:false}).compile(generateSchema(nullable,false,'3.0')),/nullable/);
// Never is represented as an always-false schema.
assert.equal(JSON.stringify(generateSchema(z.never(),false,'3.1')),JSON.stringify({not:{}}));
let callbacks=0;const callback=()=>{callbacks++;throw Error('callback executed during export')};
for(const schema of [z.union([z.string().transform(callback),z.number()]),z.union([z.preprocess(callback,z.string()),z.number()]),z.union([z.string(),z.number()]).transform(callback)])validator.openapi(schema);
assert.equal(callbacks,0);
console.log(`PASS ${comparisons} actual Zod/AJV union comparisons over ${exports} OAS3.0/3.1 exports; overlap, finite enum duplicates/mixed/empty, annotation/discriminator controls and zero callbacks`);
