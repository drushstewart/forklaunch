import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const dist=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules/better-auth/dist/';
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(dist+'index.mjs'));
const {twoFactor}=await import(pathToFileURL(dist+'plugins/two-factor/index.mjs'));
const observations=[];
async function setup(storage='plain') {
 const ref=await betterAuthReference(root);ref.database.twoFactor=[];
 const auth=betterAuth({...ref.config,secret:'owned-backup-view-secret-at-least32',plugins:[...ref.config.plugins,twoFactor({backupCodeOptions:{storeBackupCodes:storage}})],logger:{disabled:true},rateLimit:{enabled:false}});
 const call=async(userId='owned-target')=>{try{return {value:await auth.api.viewBackupCodes({body:{userId}})}}catch(e){return {status:e.statusCode??e.status,code:e.body?.code}}};
 return {ref,auth,call,set:value=>{ref.database.twoFactor=[{id:'owned-factor',userId:'owned-target',secret:'not-used',backupCodes:value,verified:false}];}};
}
for(const [stored,expected] of [['["one","one","","é"]',{status:true,backupCodes:['one','one','','é']}],['[]',{status:true,backupCodes:[]}],['{}',{status:true,backupCodes:{}}],['"x"',{status:true,backupCodes:'x'}],['false',null],['0',null],['""',null],['null',null],['invalid',null]]) {
 const s=await setup();s.set(stored);const before=JSON.stringify(s.ref.database);const r=await s.call();if(expected)assert.deepEqual(r.value,expected);else assert.equal(r.code,'INVALID_BACKUP_CODE');assert.equal(JSON.stringify(s.ref.database),before);observations.push({stored,code:r.code??'success',noWrites:true});
}
{
 const s=await setup();assert.equal((await s.call()).code,'BACKUP_CODES_NOT_ENABLED');s.set('["one"]');assert.deepEqual((await s.call()).value,{status:true,backupCodes:['one']}); // no session or User record
 const response=await s.auth.handler(new Request('http://localhost:3000/api/auth/two-factor/view-backup-codes',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({userId:'owned-target'})}));assert.equal(response.status,404);observations.push({noSessionNoUser:true,unverifiedFactor:true,http:response.status});
}
{
 let called=0;const s=await setup({encrypt:async s=>s,decrypt:async()=>{called++;throw Error('secret failure')}});s.set('cipher');const before=JSON.stringify(s.ref.database);const r=await s.call();assert.ok(!r.value);assert.equal(called,1);assert.equal(JSON.stringify(s.ref.database),before);observations.push({callbackError:true,noWrites:true});
}
{
 let release,ready;const entered=new Promise(r=>ready=r),wait=new Promise(r=>release=r);const s=await setup({encrypt:async s=>s,decrypt:async()=>{ready();await wait;return '["old"]'}});s.set('oldcipher');const work=s.call();await entered;s.ref.database.twoFactor=[];release();assert.deepEqual((await work).value,{status:true,backupCodes:['old']});observations.push({deletedDuringDecode:'old-result-returned',noAuthorityRecheck:true});
}
{
 const {symmetricEncrypt}=await import(pathToFileURL(dist+'crypto/index.mjs'));
 const s=await setup('encrypted');const context=await s.auth.$context;
 const encoded=await symmetricEncrypt({data:'["owned"]',key:context.secretConfig});s.set(encoded);assert.deepEqual((await s.call()).value,{status:true,backupCodes:['owned']});
 s.set('["owned"]');assert.ok(!(await s.call()).value);
 const plain=await setup();plain.set(encoded);assert.equal((await plain.call()).code,'INVALID_BACKUP_CODE');observations.push({encrypted:true,wrongPolicyPlain:'INVALID_BACKUP_CODE',wrongPolicyEncrypted:'throws'});
}
{
 const s=await setup();s.set('["owned"]');s.ref.database.twoFactor[0].userId='42';const value=await s.auth.api.viewBackupCodes({body:{userId:42}});assert.deepEqual(value,{status:true,backupCodes:['owned']});observations.push({numericUserId:'coerced-string',nativeSubset:'string-only'});
}
console.log('PASS viewBackupCodes actual-source',JSON.stringify(observations));
