import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, realpath } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(import.meta.url);
const tsc = process.env.TSC_BIN ?? path.join(root,'framework/node_modules/.bin/tsc');
const ts = require(require.resolve('typescript',{paths:[path.dirname(await realpath(tsc))]}));
const registrations=[];
const internal = new Proxy({}, {get:(_target,key)=> (..._args) => {if (key!=="use") registrations.push(String(key).toUpperCase());}});
const baseExports={};
const baseCompiled=ts.transpileModule(await readFile(path.join(root,'framework/core/src/http/router/expressLikeRouter.ts'),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
vm.runInNewContext(baseCompiled,{exports:baseExports,require:()=>new Proxy({},{get:()=>()=>()=>{}}),process:{env:{NODE_ENV:'test'}}},{filename:'actual-baseRouter.ts'});
class BaseRouter extends baseExports.ForklaunchExpressLikeRouter {
  registerMiddlewareHandler(handler,...args) {handler(...args); return this;}
  registerRoute(_method,_path,handler,...args) {handler(...args); return this;}
}
const exports={};
const compiled = ts.transpileModule(await readFile(path.join(root,'framework/express/src/expressRouter.ts'),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const localRequire = name => {
  if (name==='@forklaunch/core/http') return {ForklaunchExpressLikeRouter:BaseRouter};
  if (name==='express') return {default:{Router:()=>internal}};
  if (name==='./middleware/content.parse.middleware') return {contentParse:()=>()=>{}};
  if (name==='./middleware/enrichResponseTransmission.middleware') return {enrichResponseTransmission:()=>{}};
  throw new Error(`Unexpected runtime dependency ${name}`);
};
vm.runInNewContext(compiled,{exports,require:localRequire},{filename:'actual-expressRouter.ts'});
const router = new exports.Router('/',{},{});
const declared = await readFile(path.join(root,'framework/express/index.ts'),'utf8');
const start = declared.indexOf('export const handlers: {');
const handlerBlock = declared.slice(start,declared.indexOf('} = {',start));
const names=[...handlerBlock.matchAll(/^  (\w+): typeof /gm)].map(match=>match[1]).filter(name=>name!=='middleware');
assert.equal(names.length,27);
for (const exported of names) {
  const method = exported==='mSearch'?'m-search':exported==='mkcActivity'?'mkcactivity':exported;
  assert.equal(typeof router[method],'function',`missing actual TS router method ${method}`);
  router[method]('/probe',{name:`Probe ${method}`,responses:{204:{}}},()=>{});
}
const contract=JSON.parse(await readFile(process.argv[2],'utf8'));
const rustMethods=contract.routes.filter(route=>route.path.startsWith('/verbs/')).map(route=>route.method).sort();
assert.deepEqual([...registrations].sort(),rustMethods);
console.log('PASS actual compiled TypeScript router forwards the same 27 HTTP method tokens as Rust');
