// Actual request-auth/context/final-error middleware; owned Express wiring only.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFileSync,realpathSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
import vm from 'node:vm';
import {randomUUID} from 'node:crypto';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const require=createRequire(import.meta.url),deps=process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES;
const ts=require(require.resolve('typescript',{paths:[path.dirname(realpathSync(process.env.TSC_BIN))]}));
const express=require(path.join(deps,'node_modules/express'));
function source(relative) {
 const exports={};const code=ts.transpileModule(readFileSync(path.join(root,relative),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 vm.runInNewContext(code,{exports,Date,Buffer,process,require(spec){
  if(spec==='@forklaunch/common')return {...source('framework/common/src/safeStringify.ts'),...source('framework/common/src/guards/isNever.ts'),getEnvVar:()=>undefined};
  if(spec==='@forklaunch/validator')return source('framework/validator/src/shared/utils/prettyPrintParseErrors.ts');
  if(spec.endsWith('/pinoLogger'))return {meta:value=>value};
  if(spec.endsWith('/telemetry/constants'))return {ATTR_CORRELATION_ID:'correlation.id',ATTR_SERVICE_NAME:'service.name'};
  if(spec.endsWith('/openTelemetryCollector'))return {httpRequestsInFlightCounter:{add(){}}};
  if(spec==='jose')return {jwtVerify(){throw Error('unselected JWT');}};
  if(spec==='crypto')return require('node:crypto');
  if(spec==='uuid')return {v4:randomUUID};
  if(spec.startsWith('.'))return source(path.relative(root,path.resolve(root,path.dirname(relative),spec+'.ts')));
  return require(path.join(deps,'node_modules',spec));
 }});return exports;
}
for(const family of ['paymentLink','subscription','plan','checkoutSession']) {
 const text=readFileSync(path.join(root,`blueprint/billing-stripe/api/controllers/${family}.controller.ts`),'utf8');
 assert.match(text,/access: 'internal'/);assert.match(text,/hmac:\s*{\s*secretKeys:\s*{\s*default: HMAC_SECRET_KEY/);
}
const portal=readFileSync(path.join(root,'blueprint/billing-stripe/api/controllers/billingPortal.controller.ts'),'utf8');assert.match(portal,/access: 'protected'/);assert.match(portal,/jwt:/);
const auth=source('framework/core/src/http/middleware/request/auth.middleware.ts');
const context=source('framework/core/src/http/middleware/request/createContext.middleware.ts');
const hmac=source('framework/core/src/http/createHmacToken.ts');
const file=path.join(root,'framework/express/src/expressApplication.ts');const tree=ts.createSourceFile(file,readFileSync(file,'utf8'),ts.ScriptTarget.Latest,true);let handler;
function visit(node){if(ts.isVariableDeclaration(node)&&node.name.getText(tree)==='errorHandler')handler=node.initializer;ts.forEachChild(node,visit);}visit(tree);assert(handler);
const expression=ts.createPrinter().printNode(ts.EmitHint.Expression,handler,tree);
const box={exports:{},express,console:{error(){}},meta:value=>value,ATTR_HTTP_RESPONSE_STATUS_CODE:'http.response.status_code',...source('framework/core/src/http/guards/isForklaunchRequest.ts')};
vm.runInNewContext(ts.transpileModule(`exports.handler=(function(){return ${expression};}).call({openTelemetryCollector:{error(){}}});`,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,box);
const app=express();app.use(express.json({verify(req,_res,buffer){req._rawBody=buffer;}}));
let calls=0;app.use((req,res,next)=>{req.contractDetails={access:'internal',auth:{hmac:{secretKeys:{default:'owned-hmac-secret'}}}};context.createContext({})(req,res,next);});
const guarded=async(req,res,next)=>{try{await auth.parseRequestAuth(req,res,next);}catch(error){next(error);}};
app.put('/owned',guarded,(_req,res)=>{calls++;res.json({ok:true});});
const mounted=express.Router();mounted.put(['/:id','/'],guarded,(_req,res)=>{calls++;res.json({ok:true});});mounted.get(['/:id','/'],guarded,(_req,res)=>{calls++;res.json({ok:true});});app.use('/payment-link',mounted);const outer=express.Router();outer.use('/payment-link',mounted);app.use('/outer',outer);app.use(box.exports.handler);
const server=await new Promise(resolve=>{const server=app.listen(0,'127.0.0.1',()=>resolve(server));});
function header(stamp=new Date(),nonce='repeat-nonce') {return `HMAC keyId=default ts=${stamp.toISOString()} nonce=${nonce} signature=${hmac.createHmacToken({method:'PUT',path:'/owned',body:'{"amount":"12"}',timestamp:stamp,nonce,secretKey:'owned-hmac-secret'})}`;}
const valid=header();
function representedTimestamp(value){const stamp=new Date(value),nonce='representation';return `HMAC keyId=default ts=${value} nonce=${nonce} signature=${hmac.createHmacToken({method:'PUT',path:'/owned',body:'{"amount":"12"}',timestamp:stamp,nonce,secretKey:'owned-hmac-secret'})}`;}

const cases=[['missing',undefined,401,'Authorization token required.'],['empty','',401,'Invalid Authorization token format.'],['no-parts','HMAC',401,'Invalid Authorization token format.'],['missing-parts','HMAC keyId=default',403,'Invalid Authorization token.'],['wrong-prefix',valid.replace('HMAC','OTHER'),403,'Invalid Authorization token.'],['wrong-label',valid.replace('keyId=','key='),401,'Invalid Authorization token format.'],['bad-signature',valid.replace(/signature=.*/, 'signature=invalid'),403,'Invalid Authorization signature.'],['unknown-key',valid.replace('keyId=default','keyId=other'),500],['invalid-date',valid.replace(/ts=\S+/, 'ts=not-a-date'),500],['valid',valid,200],['repeated',valid,200],['old',header(new Date(Date.now()-3600000)),200],['future',header(new Date(Date.now()+3600000)),200],['extra-part',valid+' ignored=extra',200],['reordered',valid.replace(/keyId=(\S+) ts=(\S+)/,'ts=$2 keyId=$1'),401,'Invalid Authorization token format.'],['empty-nonce',valid.replace(/nonce=\S+/,'nonce='),401,'Invalid Authorization token format.'],['empty-date',valid.replace(/ts=\S+/,'ts='),401,'Invalid Authorization token format.'],['date-only',representedTimestamp('2026-10-15'),200],['offset',representedTimestamp('2026-10-15T07:23:45.678+02:00'),200],['short-fraction',representedTimestamp('2026-10-15T07:23:45.6Z'),200],['long-fraction',representedTimestamp('2026-10-15T07:23:45.678999Z'),200],['nonce-equals',header(new Date(),'a=b'),200],['remaining-legacy-date',representedTimestamp('10/15/2026'),200],['remaining-offsetless-date',representedTimestamp('2026-10-15T07:23:45'),200],['full-timeclip-min',representedTimestamp('-271821-04-20T00:00:00.000Z'),200],['full-timeclip-max',representedTimestamp('+275760-09-13T00:00:00.000Z'),200]];
try {
 let count=0;
 for(const [name,authorization,status,body] of cases)for(const correlation of [undefined,'auth-correlation','']) {
  calls=0;const headers={'content-type':'application/json'};if(authorization!==undefined)headers.authorization=authorization;if(correlation!==undefined)headers['x-correlation-id']=correlation;
  const response=await fetch(`http://127.0.0.1:${server.address().port}/owned`,{method:'PUT',headers,body:'{"amount":"12"}'});
  assert.equal(response.status,status,name);assert.equal(calls,status===200?1:0,name);
  const text=await response.text();if(body)assert.equal(text,body,name);else if(status===500)assert.equal(text,`Internal server error:\n\nCorrelation id: ${response.headers.get('x-correlation-id')}`,name);else assert.equal(text,'{"ok":true}',name);
  count++;
 }
 for(const signedPath of ['/owned','/payment-link/owned']) {
  const stamp=new Date(),nonce='mounted';const signature=hmac.createHmacToken({method:'PUT',path:signedPath,body:'{"amount":"12"}',timestamp:stamp,nonce,secretKey:'owned-hmac-secret'});
  calls=0;const response=await fetch(`http://127.0.0.1:${server.address().port}/payment-link/owned`,{method:'PUT',headers:{'content-type':'application/json',authorization:`HMAC keyId=default ts=${stamp.toISOString()} nonce=${nonce} signature=${signature}`},body:'{"amount":"12"}'});
  assert.equal(response.status,signedPath==='/owned'?200:403,`mounted signed path ${signedPath}`);assert.equal(calls,signedPath==='/owned'?1:0);await response.text();count++;
 }
 for(const [wirePath,signedPath,status] of [['/payment-link/abc%2Fdef?x=1','/abc%2Fdef',200],['/payment-link/abc%2Fdef?x=1','/abc/def',403],['/payment-link/owned?x=1','/owned?x=1',403],['/payment-link/owned/','/owned/',200],['/payment-link/owned/','/owned',403],['/payment-link','/',200],['/payment-link/','/',200],['/payment-link/owned//','/owned//',404],['/PAYMENT-LINK/owned','/owned',200]]) {
  const stamp=new Date(),nonce='path';const signature=hmac.createHmacToken({method:'PUT',path:signedPath,body:'{"amount":"12"}',timestamp:stamp,nonce,secretKey:'owned-hmac-secret'});
  calls=0;const response=await fetch(`http://127.0.0.1:${server.address().port}${wirePath}`,{method:'PUT',headers:{'content-type':'application/json',authorization:`HMAC keyId=default ts=${stamp.toISOString()} nonce=${nonce} signature=${signature}`},body:'{"amount":"12"}'});
  assert.equal(response.status,status,`${wirePath} signed ${signedPath}`);assert.equal(calls,status===200?1:0);await response.text();count++;
 }
 for(const raw of [undefined,'{"z":1,"a":2}',' { "amount" : "12" } ','{"value":"雪"}']) {
  const stamp=new Date(),nonce='raw';const signature=hmac.createHmacToken({method:'PUT',path:'/owned',body:raw,timestamp:stamp,nonce,secretKey:'owned-hmac-secret'});
  const headers={authorization:`HMAC keyId=default ts=${stamp.toISOString()} nonce=${nonce} signature=${signature}`};if(raw!==undefined)headers['content-type']='application/json';
  calls=0;const response=await fetch(`http://127.0.0.1:${server.address().port}/owned`,{method:'PUT',headers,body:raw});assert.equal(response.status,200);assert.equal(calls,1);await response.text();count++;
 }
 for(const wirePath of ['/payment-link/owned','/payment-link/owned/','/payment-link','/payment-link/','/outer/payment-link/owned/']) {
  const signedPath=wirePath.includes('owned')?(wirePath.endsWith('/')?'/owned/':'/owned'):'/';const stamp=new Date(),nonce='head';
  const signature=hmac.createHmacToken({method:'HEAD',path:signedPath,timestamp:stamp,nonce,secretKey:'owned-hmac-secret'});
  calls=0;const response=await fetch(`http://127.0.0.1:${server.address().port}${wirePath}`,{method:'HEAD',headers:{authorization:`HMAC keyId=default ts=${stamp.toISOString()} nonce=${nonce} signature=${signature}`}});assert.equal(response.status,200);assert.equal(calls,1);assert.equal(await response.text(),'');count++;
 }
 for(const wirePath of ['/payment-link/owned','/payment-link/owned/']) {
  calls=0;const response=await fetch(`http://127.0.0.1:${server.address().port}${wirePath}`,{method:'OPTIONS'});assert.equal(response.status,200);assert.deepEqual(response.headers.get('allow').split(',').map(x=>x.trim()).sort(),['GET','HEAD','PUT']);assert.equal(calls,0);await response.text();count++;
 }
 console.log(`PASS ${count} actual HMAC HTTP context/auth/final-error cases: missing/empty/format/signature/key/date, accepted repeated/old/future/extra-part; raw Buffer signing. Server registry and protected handler are owned; no live provider.`);
}finally{await new Promise((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
