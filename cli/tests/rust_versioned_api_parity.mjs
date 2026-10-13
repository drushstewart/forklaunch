// Exercise the repository's actual request parser/validator and native matcher.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {createRequire} from 'node:module';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {actualValidator} from './rust_actual_validator_fixture.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const external=createRequire(path.join(process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES,'package.json'));
const {z}=external('zod/v3');
const {load,validator}=actualValidator(root,(specifier,_file,load)=>specifier==='@forklaunch/common'?{
 ...load(path.join(root,'framework/common/src/deepCloneWithoutUndefined.ts')),
 ...load(path.join(root,'framework/common/src/guards/isRecord.ts')),
}:undefined);
const {parse}=load(path.join(root,'framework/core/src/http/middleware/request/parse.middleware.ts'));
const fixtures=[];
for(const names of [['z-first','a-last'],['2','1'],['10','2'],['01','1'],['4294967295','0']]) {
 const versions=names.map((name,index)=>({name,schema:{type:'object',required:['value'],properties:{value:{type:'string',minLength:index+1}}}}));
 fixtures.push({versions,cases:[{value:'a'},{value:'ab'},{value:''},{value:10},{},null,{value:'abc',extra:true}]});
}
const native=JSON.parse(execFileSync('cargo',['run','--quiet','--manifest-path',path.join(root,'framework-rs/Cargo.toml'),'-p','forklaunch-http','--example','version_selection'],{input:JSON.stringify(fixtures),encoding:'utf8',env:{...process.env,CARGO_INCREMENTAL:'0'}}));
let count=0;
for(const [i,fixture] of fixtures.entries()) {
 const requestSchema=Object.fromEntries(fixture.versions.map(version=>[version.name,z.object({body:z.object({value:z.string().min(version.schema.properties.value.minLength)}),params:z.object({}),query:z.object({}),headers:z.object({})})]));
 assert.deepEqual(native[i].order,Object.keys(requestSchema));
 for(const [j,body] of fixture.cases.entries()) {
  const req={body,params:{},query:{},headers:{},context:{correlationId:'fixture'},contractDetails:{versions:requestSchema},requestSchema,schemaValidator:validator};
  let status=200,next=false;
  const res={type(){return this;},status(code){status=code;return this;},send(){return this;}};
  parse(req,res,()=>{next=true;});
  const expected=status===400?{status}:{status,version:req.version,matched:Array.from(req._parsedVersions)};
  assert.equal(next,status!==400);
  assert.deepEqual(native[i].cases[j],expected,`${fixture.versions.map(v=>v.name)} case ${j}`);
  count++;
 }
}
console.log(`PASS ${count} actual TypeScript/native version-selection cases, declaration and JavaScript integer-key ordering, overlap and rejection`);
