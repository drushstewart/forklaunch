import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
import path from 'node:path';
import {readFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const deps=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES+'/node_modules';
const {betterAuthReference}=await import(pathToFileURL(root+'/cli/tests/fixtures/rust_better_auth_reference.mjs'));
const {betterAuth}=await import(pathToFileURL(deps+'/better-auth/dist/index.mjs'));
const {twoFactor}=await import(pathToFileURL(deps+'/better-auth/dist/plugins/two-factor/index.mjs'));
const {generateBackupCodes}=await import(pathToFileURL(deps+'/better-auth/dist/plugins/two-factor/backup-codes/index.mjs'));
const {symmetricDecrypt}=await import(pathToFileURL(deps+'/better-auth/dist/crypto/index.mjs'));
const secret='owned-backup-builtin-secret-at-least32';
const password='owned-factor-password';
const originalError=console.error;console.error=()=>{};
const observations=[];
async function setup(mode,codes=['one','two']) {
 const ref=await betterAuthReference(root);ref.database.twoFactor=[];let auth;
 function configure(storeBackupCodes){auth=betterAuth({...ref.config,secret,plugins:[...ref.config.plugins,twoFactor({skipVerificationOnEnable:true,backupCodeOptions:{...(storeBackupCodes===undefined?{}:{storeBackupCodes}),customBackupCodesGenerate:()=>codes}})],rateLimit:{enabled:false},logger:{disabled:true}});}
 configure(mode);const jar=new Map();
 async function call(route,body={}){const r=await auth.handler(new Request('http://localhost:3000/api/auth'+route,{method:'POST',headers:{origin:'http://localhost:3000','content-type':'application/json',cookie:[...jar].map(([k,v])=>`${k}=${v}`).join('; ')},body:JSON.stringify(body)}));for(const c of r.headers.getSetCookie()){const pair=c.split(';')[0],i=pair.indexOf('=');jar.set(pair.slice(0,i),pair.slice(i+1));}const t=await r.text();return {status:r.status,body:t?JSON.parse(t):null};}
 assert.equal((await call('/sign-up/email',{name:'Codec',firstName:'Codec',lastName:'User',email:'backup@example.test',password})).status,200);
 assert.equal((await call('/two-factor/enable',{password})).status,200);
 return{ref,call,configure,context:()=>auth.$context,stored:()=>ref.database.twoFactor[0].backupCodes};
}
for(const [from,to,status] of [['plain','encrypted',500],['encrypted','plain',401]]){
 const s=await setup(from);const before=s.stored();s.configure(to);const failed=await s.call('/two-factor/verify-backup-code',{code:'one'});assert.equal(failed.status,status);assert.equal(s.stored(),before);s.configure(from);assert.equal((await s.call('/two-factor/verify-backup-code',{code:'one'})).status,200);observations.push({from,to,status,preserved:true,restoreWorks:true});
}
{
 const s=await setup(undefined,['same','same','é🔐','']);assert.deepEqual(JSON.parse(await symmetricDecrypt({key:secret,data:s.stored()})),['same','same','é🔐','']);
 assert.equal((await s.call('/two-factor/verify-backup-code',{code:'same'})).status,200);assert.deepEqual(JSON.parse(await symmetricDecrypt({key:secret,data:s.stored()})),['é🔐','']);
 assert.equal((await s.call('/two-factor/verify-backup-code',{code:''})).status,200);observations.push({default:'encrypted',duplicates:'all removed',empty:'valid'});
}
{
 const codes=Array.from({length:100},()=> '\0'.repeat(128));const plain=await generateBackupCodes(secret,{storeBackupCodes:'plain',customBackupCodesGenerate:()=>codes});assert.equal(Buffer.byteLength(plain.encryptedBackupCodes),77101);const encrypted=await generateBackupCodes(secret,{storeBackupCodes:'encrypted',customBackupCodesGenerate:()=>codes});assert.equal(encrypted.encryptedBackupCodes.length,154282);assert.deepEqual(JSON.parse(await symmetricDecrypt({key:secret,data:encrypted.encryptedBackupCodes})),codes);observations.push({maximumCodes:100,maximumUtf8CodeBytes:128,jsonEscapedBytes:77101,ciphertextHexBytes:154282});
}
const {xchacha20poly1305}=await import(pathToFileURL(deps+'/@noble/ciphers/chacha.js'));
const vectors=JSON.parse(readFileSync(root+'/blueprint-rs/crates/implementations/iam/forklaunch-iam-better-auth/src/two_factor/backup_encryption_vectors.json','utf8'));
for(const vector of vectors){const nonce=Uint8Array.from({length:24},(_,i)=>i);const key=createHash('sha256').update(vector.secret).digest();const plain=JSON.stringify(vector.codes);assert.equal(Buffer.concat([nonce,xchacha20poly1305(key,nonce).encrypt(Buffer.from(plain))]).toString('hex'),vector.ciphertext);assert.deepEqual(JSON.parse(await symmetricDecrypt({key:vector.secret,data:vector.ciphertext})),vector.codes);}
observations.push({nodeRustVectors:vectors.length});
for(const mode of ['plain','encrypted']) {
 const state=await setup(mode);const row=state.ref.database.twoFactor[0];const original=row.backupCodes;
 for(const bad of mode==='plain'?['not json','null','{}']:['not hex','0'.repeat(80)]) {
  row.backupCodes=bad;const before=state.stored();const result=await state.call('/two-factor/verify-backup-code',{code:'one'});assert.equal(result.status,mode==='plain'&&bad!=='{}'?401:500);assert.equal(state.stored(),before);
 }
 row.backupCodes=original;assert.equal((await state.call('/two-factor/verify-backup-code',{code:'one'})).status,200);observations.push({mode,malformedPreserved:true});
}
{
 const state=await setup('encrypted');const before=state.stored();const ctx=await state.context();const initial=ctx.secretConfig;
 ctx.secretConfig='wrong-owned-backup-secret-at-least32';assert.equal((await state.call('/two-factor/verify-backup-code',{code:'one'})).status,500);assert.equal(state.stored(),before);ctx.secretConfig=initial;assert.equal((await state.call('/two-factor/verify-backup-code',{code:'one'})).status,200);observations.push({wrongKey:'500-preserved-restore-valid'});
}
{
 const state=await setup('encrypted');const ctx=await state.context();const {symmetricEncrypt}=await import(pathToFileURL(deps+'/better-auth/dist/crypto/index.mjs'));
 const keys={keys:new Map([[7,secret]]),currentVersion:7};const field=await symmetricEncrypt({key:keys,data:JSON.stringify(['one','two'])});
 state.ref.database.twoFactor[0].backupCodes=field;ctx.secretConfig={keys:new Map([[8,'different-owned-secret-at-least32']]),currentVersion:8};
 assert.equal((await state.call('/two-factor/verify-backup-code',{code:'one'})).status,500);assert.equal(state.stored(),field);
 ctx.secretConfig={keys:new Map([[8,'different-owned-secret-at-least32'],[7,secret]]),currentVersion:8};assert.equal((await state.call('/two-factor/verify-backup-code',{code:'one'})).status,200);assert(state.stored().startsWith('$ba$8$'));observations.push({versionRetirement:'500-preserved',retained:'success-reencoded-current'});
}
console.error=originalError;console.log('PASS builtin backup storage audit',JSON.stringify(observations));
