import assert from 'node:assert/strict';
import {normalizeOpenApi,sortKeys} from './fixtures/canonical-iam/normalize_openapi.mjs';
import {spawn,spawnSync} from 'node:child_process';
import {cpSync,mkdirSync,mkdtempSync,readFileSync,writeFileSync,symlinkSync,rmSync} from 'node:fs';
import {createRequire} from 'node:module';
import {tmpdir} from 'node:os';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath,pathToFileURL} from 'node:url';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'../..');
const dependencies=process.env.FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES;
assert.ok(dependencies,'Set FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES (npm install the canonical-iam fixture package)');
const fixture=join(root,'cli/tests/fixtures/canonical-iam');
const require=createRequire(join(dependencies,'package.json'));
const expected=JSON.parse(readFileSync(join(fixture,'package.json'))).dependencies;
for(const [name,version] of Object.entries(expected)) {
 assert.equal(JSON.parse(readFileSync(join(dependencies,'node_modules',name,'package.json'))).version,version,`${name} fixture version`);
}
const {SignJWT,jwtVerify}=await import(pathToFileURL(require.resolve('jose')));
const temporary=mkdtempSync(join(tmpdir(),'fl-canonical-http-'));
const children=[];
function run(command,args){
 const result=spawnSync(command,args,{cwd:temporary,env:{...process.env,CARGO_INCREMENTAL:'0'},encoding:'utf8',timeout:300000,maxBuffer:16*1024*1024});
 assert.equal(result.status,0,`${result.error??''}\n${result.stdout}\n${result.stderr}`);
}
function start(command,args){return new Promise((resolve,reject)=>{
 const child=spawn(command,args,{cwd:temporary,env:{...process.env,OTEL_SDK_DISABLED:'true',NODE_ENV:'test'},stdio:['ignore','pipe','pipe']});
 children.push(child);let output='';
 const timeout=setTimeout(()=>reject(new Error(`server readiness timeout: ${output}`)),30000);
 child.on('error',error=>{clearTimeout(timeout);reject(error);});
 child.on('exit',code=>{clearTimeout(timeout);reject(new Error(`server exited ${code}: ${output}`));});
 for(const stream of [child.stdout,child.stderr])stream.on('data',chunk=>{
  output+=chunk;const match=output.match(/READY (http:\/\/127\.0\.0\.1:\d+)/);
  if(match){clearTimeout(timeout);resolve(match[1]);}
 });
});}
try{
 symlinkSync(join(dependencies,'node_modules'),join(temporary,'node_modules'));
 for(const name of ['core','express','common','validator']){
  const dest=join(temporary,'source',name);mkdirSync(dest,{recursive:true});
  cpSync(join(root,'framework',name,'src'),join(dest,'src'),{recursive:true});
  if(name!=='core')cpSync(join(root,'framework',name,'index.ts'),join(dest,'index.ts'));
  writeFileSync(join(dest,'package.json'),JSON.stringify({name:`@forklaunch/${name}`,type:'module'}));
 }
 cpSync(join(root,'framework/validator/shims'),join(temporary,'source/validator/shims'),{recursive:true});
 writeFileSync(join(temporary,'package.json'),'{"private":true,"type":"module"}');
 writeFileSync(join(temporary,'tsconfig.json'),JSON.stringify({compilerOptions:{baseUrl:'.',paths:{
  '@fixture/openapi':['source/core/src/http/openApiV3Generator/openApiV3Generator.ts'],
  '@forklaunch/common':['source/common/index.ts'],'@forklaunch/validator':['source/validator/index.ts'],
  '@forklaunch/validator/*':['source/validator/src/*/index.ts'],'@forklaunch/core/*':['source/core/src/*/index.ts'],
  '@forklaunch/express':['source/express/index.ts']},target:'ES2022',module:'NodeNext',moduleResolution:'NodeNext',esModuleInterop:true}}));
 cpSync(join(fixture,'service.ts'),join(temporary,'service.ts'));
 cpSync(join(fixture,'openapi.ts'),join(temporary,'openapi.ts'));
 run(join(dependencies,'node_modules/.bin/esbuild'),['openapi.ts','--bundle','--platform=node','--format=esm','--packages=external','--tsconfig=tsconfig.json','--outfile=openapi.mjs']);
 run(process.execPath,['openapi.mjs']);
 run(join(dependencies,'node_modules/.bin/esbuild'),['service.ts','--bundle','--platform=node','--format=esm','--packages=external','--tsconfig=tsconfig.json','--outfile=service.mjs']);
 run('cargo',['build','--quiet','--manifest-path',join(root,'blueprint-rs/Cargo.toml'),'-p','forklaunch-blueprint-sample-service','--example','canonical_iam']);
 const ts=await start(process.execPath,['service.mjs']);
 const rust=await start(join(process.env.CARGO_TARGET_DIR??join(root,'blueprint-rs/target'),'debug/examples/canonical_iam'),[join(temporary,'issued.json')]);
 const key=new TextEncoder().encode('test-only-signing-key');
 const issued=JSON.parse(readFileSync(join(temporary,'issued.json')));
 const verified=await jwtVerify(issued.token,key,{algorithms:['HS256']});
 assert.deepEqual(verified.payload,issued.claims);
 assert.equal(verified.protectedHeader.alg,'HS256');
 const token=await new SignJWT({sub:'tester'}).setProtectedHeader({alg:'HS256'}).setExpirationTime('1h').sign(key);
 const expired=await new SignJWT({sub:'tester',exp:1}).setProtectedHeader({alg:'HS256'}).sign(key);
 const now=Math.floor(Date.now()/1000);
 const recentlyExpired=await new SignJWT({sub:'tester',exp:now-30}).setProtectedHeader({alg:'HS256'}).sign(key);
 const notYetValid=await new SignJWT({sub:'tester',exp:now+7200,nbf:now+3600}).setProtectedHeader({alg:'HS256'}).sign(key);
 const forged=await new SignJWT({sub:'tester'}).setProtectedHeader({alg:'HS256'}).setExpirationTime('1h').sign(new TextEncoder().encode('wrong-key'));
 const users=[{id:'user_1',email:'ada@example.com',display_name:null,disabled:false,roles:[]},{id:'user_2',email:'grace@example.com',display_name:null,disabled:false,roles:[]}];
 const numericDates=[];
 for(const [extra,status] of [[{nbf:null},403],[{nbf:'0'},403],[{iat:null},403],[{iat:'0'},403],[{nbf:0.5,iat:now+3600},200]]){
  const signed=await new SignJWT({sub:'tester',exp:now+7200,...extra}).setProtectedHeader({alg:'HS256'}).sign(key);
  numericDates.push(['/users',signed,status,...(status===200?[{items:users,next_cursor:null}]:[])]);
 }
 const cases=[
  ['/users/user_1',token,200,users[0]],['/users/user_1',issued.token,200,users[0]],['/users/user_2',token,200,users[1]],
  ['/users',token,200,{items:users,next_cursor:null}],
  ['/users?limit=2&cursor=user_1',token,200,{items:users,next_cursor:null}],
  ['/users/missing',token,404,''],['/users',undefined,401],['/users','invalid',403],
  ['/users',expired,403],['/users',forged,403],
  ['/users',recentlyExpired,403],['/users',notYetValid,403],...numericDates
 ];
 for(const [path,bearer,status,body] of cases){
  for(const [name,url] of [['TypeScript',ts],['Rust',rust]]){
   const response=await fetch(url+path,{headers:bearer?{authorization:`Bearer ${bearer}`}:{},signal:AbortSignal.timeout(5000)});
   const text=await response.text();assert.equal(response.status,status,`${name} ${path}: ${text}`);
   if(body!==undefined)assert.deepEqual(text?JSON.parse(text):'',body,`${name} ${path}`);
  }
 }
 const normalized=[];
 for(const [url,path] of [[ts,'/api/openapi'],[rust,'/openapi.json']]){
  const response=await fetch(url+path,{signal:AbortSignal.timeout(5000)});assert.equal(response.status,200);
  const document=await response.json();const spec=document.latest??document;
  if(process.env.FORKLAUNCH_TEST_OPENAPI_OUTPUT){
   mkdirSync(process.env.FORKLAUNCH_TEST_OPENAPI_OUTPUT,{recursive:true});
   writeFileSync(join(process.env.FORKLAUNCH_TEST_OPENAPI_OUTPUT,url===ts?'typescript.json':'rust.json'),JSON.stringify(spec,null,2)+'\n');
  }
  const name=url===ts?'typescript':'rust';
  const snapshot=JSON.stringify(sortKeys(spec),null,2)+'\n';
  const snapshotPath=join(fixture,`${name}.openapi.json`);
  if(process.env.UPDATE_CANONICAL_OPENAPI==='1')writeFileSync(snapshotPath,snapshot);
  assert.equal(snapshot,readFileSync(snapshotPath,'utf8'),`${name} OpenAPI snapshot drift`);
  normalized.push(normalizeOpenApi(spec));
  assert.ok(spec.paths['/users']);assert.ok(spec.paths['/users/{id}']);
  assert.equal(spec.paths['/users/{id}'].get.parameters.find(parameter=>parameter.in==='path').required,true);
  for(const item of Object.values(spec.paths))for(const operation of Object.values(item))
   for(const requirement of operation.security??[])for(const name of Object.keys(requirement))
    assert.ok(spec.components.securitySchemes[name],`unresolved security scheme ${name}`);
 }
 assert.deepEqual(Object.keys(normalized[0]),Object.keys(normalized[1]));
 const differences=[];
 for(const operation of Object.keys(normalized[0])){
  for(const field of ['parameters','security'])assert.deepEqual(normalized[0][operation][field],normalized[1][operation][field],`${operation} ${field}`);
  assert.deepEqual(normalized[0][operation].responses['200'],normalized[1][operation].responses['200'],`${operation} success schema`);
  const left=normalized[0][operation].responses,right=normalized[1][operation].responses;
  for(const status of [...new Set([...Object.keys(left),...Object.keys(right)])].sort()){
   if(JSON.stringify(left[status])!==JSON.stringify(right[status]))differences.push({operation,status,typescript:left[status]??null,rust:right[status]??null});
  }
 }
 const differenceSnapshot=JSON.stringify(differences,null,2)+'\n';
 const differencePath=join(fixture,'openapi-differences.json');
 if(process.env.UPDATE_CANONICAL_OPENAPI==='1')writeFileSync(differencePath,differenceSnapshot);
 assert.equal(differenceSnapshot,readFileSync(differencePath,'utf8'),'remaining OpenAPI response differences changed');
 console.log(`PASS normalized OpenAPI parameters/security/success schemas; ${differences.length} explicit error-response document differences retained`);
 console.log(`PASS canonical actual TypeScript/Rust IAM services: ${cases.length*2} live HTTP outcomes and both OpenAPI endpoints; rejected-JWT statuses match`);
}finally{
 for(const child of children){
  if(child.exitCode!==null||child.signalCode!==null)continue;
  await new Promise(resolve=>{const timer=setTimeout(()=>child.kill('SIGKILL'),5000);child.once('exit',()=>{clearTimeout(timer);resolve();});child.kill('SIGTERM');});
 }
 rmSync(temporary,{recursive:true,force:true});
}
