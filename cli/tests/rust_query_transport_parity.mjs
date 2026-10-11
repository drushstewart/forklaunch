import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile,realpath,mkdtemp,writeFile,rm} from 'node:fs/promises';
import path from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {execFileSync} from 'node:child_process';
import vm from 'node:vm';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const require=createRequire(import.meta.url);
const tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
const ts=require(require.resolve('typescript',{paths:[path.dirname(await realpath(tsc))]}));
const temporary=await mkdtemp(path.join(tmpdir(),'fl-query-parity-'));
let target;
async function fetcher(url){target=new URL(url).pathname+new URL(url).search;return new Response('{"ok":true}',{status:200,headers:{'content-type':'application/json'}});}
async function load(file,imports={}) {
 const compiled=ts.transpileModule(await readFile(path.join(root,file),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const exports={};vm.runInNewContext(compiled,{exports,require(spec){if(spec in imports)return imports[spec];throw Error('unexpected runtime import '+spec);},fetch:fetcher,Buffer,URL,URLSearchParams,Response,FormData,Blob});return exports;
}
try {
 const {safeStringify}=await load('framework/common/src/safeStringify.ts');
 const {openApiCompliantPath}=await load('framework/common/src/openApiCompliantPath.ts');
 const {unpackRouters}=await load('framework/core/src/http/router/unpackRouters.ts');
 const {getSdkPath}=await load('framework/universal-sdk/src/core/resolvePath.ts',{'./regex':{generateStringFromRegex(){throw Error('unexpected regex');}}});
 class ZodSchemaValidator {schemify(value){return value;}}
 class FastMCP {tools=[];addTool(tool){this.tools.push(tool);}}
 const {generateMcpServer}=await load('framework/core/src/http/mcpGenerator/mcpGenerator.ts',{'@forklaunch/common':{safeStringify,isRecord:value=>value!==null&&typeof value==='object'&&!Array.isArray(value),isNever:()=>false},'@forklaunch/validator/zod':{ZodSchemaValidator,string:{startsWith:()=>({})}},fastmcp:{FastMCP},'../router/discriminateBody':{discriminateBody(){throw Error('unexpected body');},discriminateResponseBodies:()=>({200:{contentType:'application/json'}})},'../router/unpackRouters':{unpackRouters}});
 const app={basePath:'/',routers:[],routes:[{method:'get',path:'/query/:id',contractDetails:{name:'Read Query',summary:'Read query',params:{id:{}},query:{},responses:{200:{}},options:{mcp:true}}}]};
 const mcp=generateMcpServer(new ZodSchemaValidator(),'http','127.0.0.1',9000,'1.0.0',app);
 assert.equal(mcp.tools.length,1);
 const {UniversalSdk}=await load('framework/universal-sdk/src/universalSdk.ts',{'@forklaunch/common':{safeStringify,openApiCompliantPath,safeParse:JSON.parse},ajv:{default:class {}},'ajv-formats':{default(){}},'./core/coerceSpecialTypes':{coerceSpecialTypes:value=>value},'./core/mapContentType':{mapContentType:value=>value},'./core/openApi':{refreshOpenApi(){throw Error('unexpected refresh');}},'./core/resolvePath':{getSdkPath}});
 const universal=new UniversalSdk('http://127.0.0.1:9000',{validate:()=>true},{},undefined);
 universal.registryOpenApiJson={latest:{paths:{'/query/{id}':{get:{responses:{200:{content:{'application/json':{schema:{}}}}}}}}}};
 const queries=[{}, {q:'a+b & 水'},{ids:['a,b','x+y','水']},{ids:[]},{flags:[true,false]},{prices:[0,100,1.25]},{filter:{active:false,ids:['x','y']}},{nullable:null},{q:'%2F / ? # = &',title:'"quoted"'},{empty:'',flag:false,zero:0}];
 const cases=queries.flatMap(query=>['plain','a/b + 水','%2F?x#y','!*\'()~'].map(id=>({params:{id},query})));
 await writeFile(path.join(temporary,'cases.json'),JSON.stringify(cases));
 execFileSync('cargo',['test','--quiet','--manifest-path','framework-rs/Cargo.toml','-p','forklaunch-mcp','-p','forklaunch-sdk-typescript','--all-features','native_query_transport'],{cwd:root,env:{...process.env,FORKLAUNCH_TEST_QUERY_TRANSPORT_INPUT:path.join(temporary,'cases.json'),FORKLAUNCH_TEST_QUERY_TRANSPORT_OUTPUT:temporary},encoding:'utf8',timeout:180000});
 const native=JSON.parse(await readFile(path.join(temporary,'mcp-targets.json'),'utf8'));
 const template=await readFile(path.join(root,'cli/src/templates/sdk/typescript/index.ts'),'utf8');
 const runtime=template.slice(0,template.indexOf('{{#operation_types}}'))+template.slice(template.indexOf('function pathWithParams'),template.indexOf('export function createForkLaunchSdk'))+'\nexport {request};\n';
 await writeFile(path.join(temporary,'cli-sdk.ts'),runtime);
 await writeFile(path.join(temporary,'consumer.ts'),`import { QueryClient } from './query-sdk';\nimport {request} from './cli-sdk';\nnew QueryClient('').readQuery({params:{id:'id'},query:{ids:['a','b'],filter:{active:false},nullable:null}});\nvoid request({}, {service:'ecommerce',router:'product',operation:'query',method:'GET',path:'/query/:id',input:{params:{id:'id'},query:{ids:['a','b'],filter:{active:false},nullable:null}}});\n`);
 await writeFile(path.join(temporary,'tsconfig.json'),JSON.stringify({compilerOptions:{strict:true,target:'ES2022',module:'CommonJS',lib:['ES2022','DOM'],types:[],outDir:'compiled'},include:['*.ts']}));
 execFileSync(tsc,['--project',path.join(temporary,'tsconfig.json')],{encoding:'utf8',timeout:120000});
 const {QueryClient}=require(path.join(temporary,'compiled/query-sdk.js'));const client=new QueryClient('http://127.0.0.1:9000',fetcher);
 const cli=require(path.join(temporary,'compiled/cli-sdk.js'));
 for(let index=0;index<cases.length;index++) {
   const input=cases[index];
   await mcp.tools[0].execute(input);assert.equal(target,native[index],`actual TS MCP ${JSON.stringify(input)}`);
   await universal.execute('/query/:id','get','latest',input);assert.equal(target,native[index],`actual TS universal SDK ${JSON.stringify(input)}`);
   await client.readQuery(input);assert.equal(target,native[index],`native TS SDK ${JSON.stringify(input)}`);
   await cli.request({baseUrl:'http://127.0.0.1:9000',fetch:fetcher},{service:'ecommerce',router:'product',operation:'query',method:'GET',path:'/query/:id',input});assert.equal(target,native[index],`CLI TS SDK ${JSON.stringify(input)}`);
 }
 console.log(`PASS ${cases.length} actual TS MCP/universal SDK and native MCP/generated SDK URL cases: JSON arrays/objects/null, empty arrays, Unicode and single percent encoding`);
} finally {await rm(temporary,{recursive:true,force:true});}
