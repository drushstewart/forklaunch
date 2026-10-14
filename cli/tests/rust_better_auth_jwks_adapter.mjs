import assert from 'node:assert/strict';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const dependencies=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies,'Set FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES');
const moduleUrl=relative=>pathToFileURL(path.join(dependencies,'node_modules',relative));
const {getJwksAdapter}=await import(moduleUrl('better-auth/dist/plugins/jwt/adapter.mjs'));
const rows=[
 {id:'legacy',createdAt:new Date(1000),alg:null},
 {id:'expired',createdAt:new Date(4000),alg:'EdDSA',expiresAt:new Date(0)},
 {id:'rsa',createdAt:new Date(3000),alg:'RS256'},
 {id:'ed',createdAt:new Date(2000),alg:'EdDSA'},
];
const ctx={owned:'context'};
let checks=0;
for(const customGet of [false,true]) for(const customCreate of [false,true]) {
 const calls=[];
 const base={findMany:async args=>{assert.equal(args.model,'jwks');calls.push('base-list');return rows},findOne:async args=>{calls.push('base-one');return rows.find(row=>row.id===args.where[0].value)},create:async args=>{calls.push('base-create');assert.ok(args.data.createdAt instanceof Date);return {...args.data,id:'base-created'}}};
 const adapter=getJwksAdapter(base,{adapter:{...(customGet?{getJwks:async context=>{assert.equal(context,ctx);calls.push('get');return rows}}:{}),...(customCreate?{createJwk:async (key,context)=>{assert.equal(context,ctx);assert.equal(key.createdAt,undefined);calls.push('create');return {...key,id:'custom-created'}}}:{})}});
 assert.equal((await adapter.getAllKeys(ctx)).length,4);
 assert.equal((await adapter.getLatestKey(ctx)).id,'rsa');
 assert.equal((await adapter.getLatestKeyByAlg(ctx,'EdDSA')).id,'ed');
 assert.equal((await adapter.getLatestKeyByAlg(ctx,'RS256')).id,'rsa');
 assert.equal((await adapter.getKeyById(ctx,'expired')).id,'expired'); // pinned lookup doesn't filter expiry here
 assert.equal(await adapter.getKeyById(ctx,'missing'),undefined);
 assert.equal((await adapter.createJwk(ctx,{publicKey:'public',privateKey:'private'})).id,customCreate?'custom-created':'base-created');
 if(customGet)assert.ok(!calls.some(value=>value.startsWith('base-')&&value!=='base-create'));
 if(customCreate)assert.ok(!calls.includes('base-create'));
 checks+=7;
}
console.log(`${checks} actual independent JWKS lookup/create hook/default cases PASS`);

const {signJWT}=await import(moduleUrl('better-auth/dist/plugins/jwt/sign.mjs'));
const {generateKeyPair,exportJWK,jwtVerify}=await import(moduleUrl('jose/dist/webapi/index.js'));
const pair=await generateKeyPair('EdDSA',{extractable:true});
const replacement={id:'adapter-returned-key',alg:'EdDSA',crv:'Ed25519',createdAt:new Date(),publicKey:JSON.stringify(await exportJWK(pair.publicKey)),privateKey:JSON.stringify(await exportJWK(pair.privateKey))};
let reads=0,creates=0,captured;
const context={context:{options:{baseURL:'https://owned.example.test'},adapter:{findMany:async()=>{reads++;return [];},create:async()=>{throw new Error('unexpected default insert')}}}};
const options={jwks:{disablePrivateKeyEncryption:true,keyPairConfig:{alg:'EdDSA'},rotationInterval:2},adapter:{createJwk:async(record,ctx)=>{assert.equal(ctx,context);creates++;captured=record;return replacement;}}};
const token=await signJWT(context,{options,payload:{sub:'owned-subject'}});
const verified=await jwtVerify(token,pair.publicKey);
assert.equal(verified.protectedHeader.kid,replacement.id);assert.equal(verified.payload.sub,'owned-subject');
assert.ok(captured.createdAt instanceof Date);assert.ok(captured.expiresAt instanceof Date);
assert.ok(captured.expiresAt.getTime()-captured.createdAt.getTime()>=2000&&captured.expiresAt.getTime()-captured.createdAt.getTime()<2050);
assert.notEqual(captured.privateKey,replacement.privateKey);assert.equal(creates,1);assert.ok(reads>=1);
const failed={...options,adapter:{createJwk:async()=>{throw new Error('owned create failure')}}};
await assert.rejects(signJWT(context,{options:failed,payload:{sub:'owned'}}),/owned create failure/);
const invalid={...options,adapter:{createJwk:async()=>({...replacement,privateKey:'invalid'})}};
await assert.rejects(signJWT(context,{options:invalid,payload:{sub:'owned'}}));
console.log('PASS actual JWKS create hook: supplied expiration/createdAt, returned replacement id+key used for verified signature, independent default reads, callback failure, invalid returned material failure');

// Publication re-reads the chosen source after creation and fails if still empty.
const {betterAuthReference}=await import('./fixtures/rust_better_auth_reference.mjs');
const {fileURLToPath}=await import('node:url');
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const {betterAuth}=await import(moduleUrl('better-auth/dist/index.mjs'));
let emptyReads=0;
const reference=await betterAuthReference(root,{jwt:{adapter:{getJwks:async()=>{emptyReads++;return null;}}}});
const auth=betterAuth({...reference.config,logger:{disabled:true}});
const response=await auth.handler(new Request('http://localhost:3000/api/auth/jwks'));
assert.equal(response.status,500);assert.equal(emptyReads,2);
console.log('PASS actual JWKS publication rejects a custom read source still empty after independent default creation');
let fallbackReads=0;
const fallbackOptions={jwks:{disablePrivateKeyEncryption:true},adapter:{getJwks:async()=>++fallbackReads===1?[]:[replacement],createJwk:async()=>{throw new Error('fallback must reuse returned key');}}};
const fallbackToken=await signJWT(context,{options:fallbackOptions,payload:{sub:'fallback'}});
assert.equal(fallbackReads,2);assert.equal((await jwtVerify(fallbackToken,pair.publicKey)).payload.sub,'fallback');
console.log('PASS actual custom reader is invoked again for latest-any fallback and its second snapshot is authoritative');

const {privateKey: omittedPrivate, ...publicOnly}=replacement;
assert.ok(omittedPrivate);
const publicReference=await betterAuthReference(root,{jwt:{jwks:{disablePrivateKeyEncryption:true},adapter:{getJwks:async()=>[publicOnly]}}});
const publicAuth=betterAuth({...publicReference.config,logger:{disabled:true}});
const published=await publicAuth.handler(new Request('http://localhost:3000/api/auth/jwks'));
assert.equal(published.status,200);
const publishedBody=await published.json();
assert.equal(publishedBody.keys[0].kid,replacement.id);
assert.equal(publishedBody.keys[0].d,undefined);
const {SignJWT}=await import(moduleUrl('jose/dist/webapi/index.js'));
const publicToken=await new SignJWT({sub:'public-owner'}).setProtectedHeader({alg:'EdDSA',kid:replacement.id}).setIssuer('http://localhost:3000').setAudience('http://localhost:3000').sign(pair.privateKey);
assert.equal((await publicAuth.api.verifyJWT({body:{token:publicToken}})).payload.sub,'public-owner');
await assert.rejects(signJWT(context,{options:{jwks:{disablePrivateKeyEncryption:true},adapter:{getJwks:async()=>[publicOnly]}},payload:{sub:'owner'}}));
await assert.rejects(signJWT(context,{options:{jwks:{disablePrivateKeyEncryption:true},adapter:{getJwks:async()=>[],createJwk:async()=>publicOnly}},payload:{sub:'owner'}}));
console.log('PASS actual public-only key: publication and verification work; signing fails for both read-selected and create-returned public-only records');
