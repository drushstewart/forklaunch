import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync,writeFileSync,mkdtempSync,rmSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {Readable} from 'node:stream';
import {tmpdir} from 'node:os';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import vm from 'node:vm';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const dependencies=process.env.FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES;
assert.ok(dependencies,'Set FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES to the installed canonical-iam fixture');
const require=createRequire(join(dependencies,'package.json'));
const ts=require('typescript');
function load(path,imports={}){
 const exports={};
 const source=ts.transpileModule(readFileSync(join(root,path),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText;
 vm.runInNewContext(source,{exports,Buffer,require(name){assert.ok(name in imports,`unexpected import ${name}`);return imports[name];}});
 return exports;
}
const discriminator=load('framework/core/src/http/router/discriminateBody.ts');
const common=load('framework/common/src/guards/isNever.ts');
const {contentParse}=load('framework/express/src/middleware/content.parse.middleware.ts',{
 '@forklaunch/common':common,'@forklaunch/core/http':discriminator,express:require('express'),busboy:require('busboy')
});
function wire(parts){return Buffer.concat(parts.flatMap(([name,file,bytes,mime])=>[
 Buffer.from(`--boundary\r\nContent-Disposition: form-data; name="${name}"${file===null?'':`; filename="${file}"`}\r\n${mime?`Content-Type: ${mime}\r\n`:''}\r\n`),Buffer.from(bytes),Buffer.from('\r\n')
]).concat(Buffer.from('--boundary--\r\n')));}
async function parse(bytes){
 const req=Readable.from([bytes]);req.headers={'content-type':'multipart/form-data; boundary=boundary'};
 req.contractDetails={body:{multipartForm:{}}};req.schemaValidator={};
 await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('parser deadline')),5000);contentParse()(req,{},error=>{clearTimeout(timer);error?reject(error):resolve();});});
 return Object.fromEntries(Object.entries(req.body).map(([key,value])=>[key,Buffer.isBuffer(value)?[...value]:value]));
}
const cases=[
 [['label',null,'hello 水'],['file','a.bin',[0,255,128]]],
 [['label',null,'first'],['label',null,'last'],['file','empty.bin',[]]],
 [['label',null,'ok'],['file','a.bin',[1]],['file','b.bin',[2,3]]],
 [['label',null,'line\r\nbreak'],['file','../name.bin',[0]]],
 [['label',null,'ok'],['file',null,[0,255],'application/octet-stream']],
 [['label',null,'ok'],['file','',[],'application/octet-stream']],
 [['label',null,'ok'],['file','text.txt','hello 水','text/plain']],
];
const temporary=mkdtempSync(join(tmpdir(),'fl-multipart-extract-'));
try{
 const fixtures=[];
 for(const parts of cases){const bytes=wire(parts);fixtures.push({bytes:[...bytes],expected:await parse(bytes)});}
 const path=join(temporary,'cases.json');writeFileSync(path,JSON.stringify(fixtures));
 execFileSync('cargo',['test','--manifest-path',join(root,'framework-rs/Cargo.toml'),'-p','forklaunch-axum','--features','multipart','multipart_matches_typescript_parser','--','--nocapture'],{env:{...process.env,CARGO_INCREMENTAL:'0',FORKLAUNCH_TEST_MULTIPART_CASES:path},stdio:'inherit',timeout:300000});
 console.log(`PASS ${cases.length} actual TypeScript content-parser / native typed multipart comparisons`);
}finally{rmSync(temporary,{recursive:true,force:true});}
