// Actual extracted TypeScript method bodies; owned collaborators, no ORM bootstrap claim.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createRequire} from 'node:module';
import vm from 'node:vm';
const require=createRequire(import.meta.url);
const ts=require(process.env.TSC_BIN?path.resolve(fs.realpathSync(process.env.TSC_BIN),'../../lib/typescript.js'):'typescript');
function method(file) {
 const source=ts.createSourceFile(file,fs.readFileSync(file,'utf8'),ts.ScriptTarget.Latest,true);
 let selected;
 function visit(n){if(ts.isMethodDeclaration(n)&&n.name?.getText(source)==='expireCheckoutSession')selected=n;ts.forEachChild(n,visit);}
 visit(source);assert.ok(selected,file);
 const printed=ts.createPrinter().printNode(ts.EmitHint.Unspecified,selected,source);
 const js=ts.transpileModule(`export class Probe { ${printed} }`,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const box={exports:{}};vm.runInNewContext(js,box);return box.exports.Probe;
}
const Base=method('blueprint/implementations/billing/base/services/checkoutSession.service.ts');
const Stripe=method('blueprint/implementations/billing/stripe/services/checkoutSession.service.ts');
let cases=0;
for(const id of ['00000000-0000-4000-8000-000000000001','cs_test_owned'])for(const failure of ['none','provider','missing-cache','delete']){
 const calls=[]; const base=new Base(); base.createCacheKey=x=>`checkout:${x}`;
 base.cache={async readRecord(key){calls.push(['read',key]);return failure==='missing-cache'?null:{value:{id}};},async deleteRecord(key){calls.push(['delete',key]);if(failure==='delete')throw Error('delete');}};
 const stripe=new Stripe();stripe.baseCheckoutSessionService=base;stripe.stripeClient={checkout:{sessions:{async expire(target){calls.push(['provider',target]);if(failure==='provider')throw Error('provider');return{id:target,object:'checkout.session',status:'expired'};}}}};
 let error;try{await stripe.expireCheckoutSession({id});}catch(e){error=e.message;}
 assert.deepEqual(calls[0],['provider',id]);
 if(failure==='provider'){assert.equal(error,'provider');assert.equal(calls.length,1);}else{
  assert.deepEqual(calls[1],['read',`checkout:${id}`]);
  if(failure==='missing-cache'){assert.equal(error,'Session not found');assert.equal(calls.length,2);}else{
   assert.deepEqual(calls[2],['delete',`checkout:${id}`]);assert.equal(calls.length,3);assert.equal(error,failure==='delete'?'delete':undefined);
  }
 }
 cases++;
}
console.log(`PASS ${cases} actual TS Checkout expiration cases: caller ID forwarded unchanged, provider expiration precedes cache existence/read/delete, local failure after remote effect, no ORM writes`);
