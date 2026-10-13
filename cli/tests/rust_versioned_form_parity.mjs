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
const cases=['value=a','value=a+b','value=%E6%B0%B4','value=','value','value=a&value=b','value[]=a','value[]=a&value[]=b','value=a&value[]=b&value=c','value[]=a&value=b&value[]=c','value%5B%5D=a&value[]=b','1[]=a&1=b&1[]=c','value=%ff+%20','value=%E6%B0','value=%2','value=%zz','value=%5B%ff','value=%00%2B','a.b=x','constructor=x','toString=x','__proto__=ignored&value=ok','=ignored&value=ok','&&value=ok&&','value=a=b','value=%26%3D','value[]=','value[]','value=a&other=b&value=c'];
const fixtures=[];
for(const body of cases) {
 const bytes=Buffer.from(body),req=Readable.from([bytes]);
 req.headers={'content-type':'application/x-www-form-urlencoded','content-length':String(bytes.length)};
 req.contractDetails={versions:{first:{body:{urlEncodedForm:{}}},last:{body:{urlEncodedForm:{}}}}};req.schemaValidator={};
 await new Promise((resolve,reject)=>{
  const timer=setTimeout(()=>reject(Error('parser deadline')),5000);
  contentParse()(req,{},error=>{clearTimeout(timer);error?reject(error):resolve();});
 });
 assert.ok(req.body && typeof req.body==='object',body);
 assert.deepEqual(req._rawBody,bytes);
 fixtures.push({kind:'form',versions:[{name:'fallback',schema:{type:'object'}},{name:'exact',schema:{const:req.body}}],cases:[body]});
}
const native=JSON.parse(execFileSync('cargo',['run','--quiet','--manifest-path',join(root,'framework-rs/Cargo.toml'),'-p','forklaunch-http','--example','version_selection'],{input:JSON.stringify(fixtures),encoding:'utf8',env:{...process.env,CARGO_INCREMENTAL:'0'},timeout:300000}));
for(const [i,result] of native.entries()) assert.deepEqual(result.cases,[{version:'exact',matched:['fallback','exact'],status:200}],cases[i]);
console.log(`PASS ${cases.length} actual TypeScript wire-parser/native URL-encoded version comparisons, including repeats, bracket arrays and malformed escapes`);
