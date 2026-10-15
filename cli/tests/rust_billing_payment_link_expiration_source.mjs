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
 function visit(n){if(ts.isMethodDeclaration(n)&&n.name?.getText(source)==='expirePaymentLink')selected=n;ts.forEachChild(n,visit);}
 visit(source);assert.ok(selected,file);
 const printed=ts.createPrinter().printNode(ts.EmitHint.Unspecified,selected,source);
 const js=ts.transpileModule(`export class Probe { ${printed} }`,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const box={exports:{}};vm.runInNewContext(js,box);return box.exports.Probe;
}
const Base=method('blueprint/implementations/billing/base/services/paymentLink.service.ts');
const Stripe=method('blueprint/implementations/billing/stripe/services/paymentLink.service.ts');
const id='00000000-0000-4000-8000-000000000001';
let cases=0;
for(const backup of [false,true])for(const failure of ['none','provider','local']){
 const calls=[];const base=new Base();base.enableDatabaseBackup=backup;base.openTelemetryCollector={info(){}};base.createCacheKey=x=>`payment-link:${x}`;
 base.cache={async deleteRecord(key){calls.push(['delete',key]);if(failure==='local')throw Error('local');}};
 base.em={async upsert(entity,record){calls.push(['upsert',JSON.parse(JSON.stringify(record))]);return record;},remove(record){calls.push(['remove',JSON.parse(JSON.stringify(record))]);return{async flush(){calls.push(['flush']);}};}};
 base.mappers={PaymentLinkMapper:{entity:class{}}};
 const stripe=new Stripe();stripe.basePaymentLinkService=base;stripe.stripeClient={paymentLinks:{async update(target,options){calls.push(['provider',target,JSON.parse(JSON.stringify(options))]);if(failure==='provider')throw Error('provider');return{id:'plink_actual',active:true};}}};
 let error;try{await stripe.expirePaymentLink({id});}catch(e){error=e.message;}
 assert.deepEqual(calls[0],['provider',id,{metadata:{status:'EXPIRED'}}]);
 // Current configured public UUID is forwarded unchanged: this fixture records
 // that target-resolution gap; the opt-in bound adapter must use sealed aliases.
 if(failure==='provider'){assert.equal(error,'provider');assert.equal(calls.length,1);}else{
  assert.deepEqual(calls.at(-1),['delete',`payment-link:${id}`]);
  assert.equal(error,failure==='local'?'local':undefined);
  if(backup){assert.deepEqual(calls[1],['upsert',{id,status:'EXPIRED'}]);assert.equal(calls[2][0],'remove');assert.equal(calls[3][0],'flush');}
  else assert.equal(calls.length,2);
 }
 cases++;
}
console.log(`PASS ${cases} actual TS PaymentLink expiration method cases: metadata-only first, UUID dispatch gap explicit, cache delete/backup removal, pre/post effect failures`);
