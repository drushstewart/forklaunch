// Compare actual Express content parsing with native version body matching.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {Readable} from 'node:stream';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const dependencies=process.env.FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES;
assert.ok(dependencies,'Set FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES');
const require=createRequire(join(dependencies,'package.json'));
const ts=require('typescript');
function load(path,imports={}) {
 const exports={};
 const source=ts.transpileModule(readFileSync(join(root,path),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText;
 vm.runInNewContext(source,{exports,Buffer,require(name){assert.ok(name in imports,`unexpected import ${name}`);return imports[name];}});
 return exports;
}
const {contentParse}=load('framework/express/src/middleware/content.parse.middleware.ts',{
 '@forklaunch/common':load('framework/common/src/guards/isNever.ts'),
 '@forklaunch/core/http':load('framework/core/src/http/router/discriminateBody.ts'),
 express:require('express'),busboy:require('busboy')
});
const cases = [
 ['json',Buffer.from('{}')], ['json',Buffer.from('[]')], ['json',Buffer.alloc(0)],
 ['json',Buffer.from(' null')], ['json',Buffer.from('true')], ['json',Buffer.from('42')], ['json',Buffer.from('"text"')],
 ['json',Buffer.from(' \t\r\n{"value":1}')], ['json',Buffer.from(' ')], ['json',Buffer.from('{')],
 ['json',Buffer.from('\ufeff{"value":"水"}')], ['json',Buffer.from('\ufeff')], ['json',Buffer.from('\ufeff\ufeff{}')],
 ['json',Buffer.from([123,34,120,34,58,34,255,34,125])], ['json',Buffer.from([123,34,120,34,58,34,0xe6,0xb0,34,125])],
 ['text',Buffer.from('水')], ['text',Buffer.alloc(0)], ['text',Buffer.from('\ufefftext')], ['text',Buffer.from('\ufeff\ufefftext')],
 ['text',Buffer.from([255])], ['text',Buffer.from([0xe6,0xb0])], ['text',Buffer.from([0xc0,0xaf])], ['text',Buffer.from([0xed,0xa0,0x80])],
];
const fixtures=[],expected=[];
for(const [kind,bytes] of cases) {
 const req=Readable.from([bytes]);
 req.headers={'content-type':kind==='json'?'application/json':'text/plain','content-length':String(bytes.length)};
 req.contractDetails={versions:{first:{body:{[kind]:{}}}}}; req.schemaValidator={};
 let failure;
 await new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>reject(Error('parser deadline')),5000);
  contentParse()(req,{},error=>{clearTimeout(timer);error?reject(error):resolve();});
 }).catch(error=>{failure=error;});
 const versions=[{name:'fallback',schema:true}];
 if(failure) expected.push([{status:failure.status}]);
 else {
  assert.deepEqual(req._rawBody,bytes);
  versions.push({name:'exact',schema:{const:req.body}});
  expected.push([{version:'exact',matched:['fallback','exact'],status:200}]);
 }
 fixtures.push({kind,wireBytes:true,versions,cases:[[...bytes]]});
}
const native=JSON.parse(execFileSync('cargo',['run','--quiet','--manifest-path',join(root,'framework-rs/Cargo.toml'),'-p','forklaunch-http','--example','version_selection'],{input:JSON.stringify(fixtures),encoding:'utf8',env:{...process.env,CARGO_INCREMENTAL:'0'},timeout:300000}));
for(const [i,result] of native.entries()) assert.deepEqual(result.cases,expected[i],`${cases[i][0]} ${cases[i][1].toString('hex')}`);
console.log(`PASS ${fixtures.length} actual TypeScript/native JSON and text wire-parser comparisons: strict roots, empty bodies, BOM and malformed UTF-8`);
