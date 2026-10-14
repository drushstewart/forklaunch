// Executes actual repository HTTP clients/signers with a pinned TypeScript compiler.
import {createRequire} from 'node:module';
import {readFileSync,realpathSync} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import vm from 'node:vm';
export function actualHttpSources(root) {
 const require=createRequire(import.meta.url);
const tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
const ts=require(require.resolve('typescript',{paths:[path.dirname(realpathSync(tsc))]})),modules=new Map();
function load(file){
 if(modules.has(file))return modules.get(file);
 const exports={};modules.set(file,exports);
 const source=ts.transpileModule(readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText.replaceAll('import.meta.url',JSON.stringify(pathToFileURL(file).href));
 vm.runInNewContext(source,{exports,__filename:file,Buffer,Date,URL,Headers,Response,TextDecoder,process,fetch,require(spec){
  if(spec==='@forklaunch/common')return load(path.join(root,'framework/common/src/safeStringify.ts'));
  if(spec.startsWith('.'))return load(path.resolve(path.dirname(file),spec+'.ts'));
  return require(spec);
 }});return exports;
}
return {load};
}
