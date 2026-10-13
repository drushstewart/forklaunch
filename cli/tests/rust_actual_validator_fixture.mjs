// Executes repository validator/mapper sources; only external packages are supplied.
import {createRequire} from 'node:module';
import {readFileSync,realpathSync,existsSync} from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
export function actualValidator(root, resolve = () => undefined, {sameRealm = false} = {}) {
 const localRequire=createRequire(import.meta.url),tsc=process.env.TSC_BIN??path.join(root,'framework/node_modules/.bin/tsc');
 const ts=localRequire(localRequire.resolve('typescript',{paths:[path.dirname(realpathSync(tsc))]}));
 const dependencyRoot=process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES??path.dirname(path.dirname(path.dirname(path.dirname(realpathSync(tsc)))));
 const external=createRequire(path.join(dependencyRoot,'package.json'));
 const modules=new Map();
 function load(file){
  if(!existsSync(file))file=path.join(file.replace(/\.ts$/, ''),'index.ts');
  if(modules.has(file))return modules.get(file);
  const exports={};modules.set(file,exports);
  const compiled=ts.transpileModule(readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  const context={exports,Buffer,Blob,Date,ArrayBuffer,Uint8Array,Map,Set,WeakMap,RegExp,require(spec){
   const override=resolve(spec,file,load);
   if(override!==undefined)return override;
   if(spec==='@forklaunch/common')return load(path.join(root,'framework/common/src/deepCloneWithoutUndefined.ts'));
   if(spec==='@forklaunch/validator')return load(path.join(root,'framework/validator/src/shared/utils/prettyPrintParseErrors.ts'));
   if(spec.startsWith('.'))return load(path.resolve(path.dirname(file),spec+'.ts'));
   return external(spec);
  }};
  // MikroORM tests instanceof Function for lazy enum factories. Its actual
  // source and entity factories must execute in one realm for that check.
  if(sameRealm)vm.runInThisContext(`(function(exports,require){${compiled}\n})`,{filename:file})(exports,context.require);
  else vm.runInNewContext(compiled,context);
  return exports;
 }
 const statics=load(path.join(root,'framework/validator/src/zod/staticSchemaValidator.ts'));
 return {statics,validator:statics.SchemaValidator(),load,ts};
}
