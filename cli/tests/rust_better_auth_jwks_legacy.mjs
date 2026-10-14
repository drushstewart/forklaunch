import assert from 'node:assert/strict';
import path from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {betterAuthReference} from './fixtures/rust_better_auth_reference.mjs';
const dependencies=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;
assert.ok(dependencies);
const load=relative=>import(pathToFileURL(path.join(dependencies,'node_modules',relative)));
const {signJWT}=await load('better-auth/dist/plugins/jwt/sign.mjs');
const {betterAuth}=await load('better-auth/dist/index.mjs');
const {generateKeyPair,exportJWK,jwtVerify}=await load('jose/dist/webapi/index.js');
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const context={context:{options:{baseURL:'http://localhost:3000'},adapter:{}}};
let checked=0;
for(const algorithm of ['EdDSA','ES256','RS256']) {
 const pair=await generateKeyPair(algorithm,{extractable:true});
 const publicKey=await exportJWK(pair.publicKey), privateKey=await exportJWK(pair.privateKey);
 for(const metadata of ['absent','null','explicit']) {
  const record={id:'legacy',createdAt:new Date(),publicKey:JSON.stringify(publicKey),privateKey:JSON.stringify(privateKey),...(metadata==='null'?{alg:null}:metadata==='explicit'?{alg:algorithm}:{})};
  const options={jwks:{disablePrivateKeyEncryption:true,keyPairConfig:{alg:algorithm}},adapter:{getJwks:async()=>[record]}};
  const token=await signJWT(context,{options,payload:{sub:'legacy'},signingKeyId:'legacy',signingAlgorithm:algorithm});
  const signed=await jwtVerify(token,pair.publicKey);assert.equal(signed.protectedHeader.alg,algorithm);
  const reference=await betterAuthReference(root,{jwt:options});
  const auth=betterAuth({...reference.config,logger:{disabled:true}});
  const response=await auth.handler(new Request('http://localhost:3000/api/auth/jwks'));
  assert.equal(response.status,200);const body=await response.json();assert.equal(body.keys[0].alg,algorithm);assert.equal(body.keys[0].kid,'legacy');
  assert.equal((await auth.api.verifyJWT({body:{token}})).payload.sub,'legacy');
  checked++;
 }
 // Publication spreads arbitrary embedded metadata after defaults; the native
 // strict shape intentionally rejects this conflicting advertisement instead.
 const record={id:'legacy',createdAt:new Date(),publicKey:JSON.stringify({...publicKey,alg:'conflicting',kid:'wrong'}),privateKey:JSON.stringify(privateKey)};
 const options={jwks:{disablePrivateKeyEncryption:true,keyPairConfig:{alg:algorithm}},adapter:{getJwks:async()=>[record]}};
 const reference=await betterAuthReference(root,{jwt:options});const auth=betterAuth({...reference.config,logger:{disabled:true}});
 const body=await (await auth.handler(new Request('http://localhost:3000/api/auth/jwks'))).json();
 assert.equal(body.keys[0].alg,'conflicting');assert.equal(body.keys[0].kid,'legacy');
 checked++;
}
console.log(`PASS ${checked} actual JOSE legacy cases: absent/null/explicit metadata across EdDSA/ES256/RS256 sign-publish-verify; conflicting publication overrides characterized`);
const invalidPair=await generateKeyPair('EdDSA',{extractable:true});
const invalidPublic=await exportJWK(invalidPair.publicKey),invalidPrivate=await exportJWK(invalidPair.privateKey);
const validRecord={id:'invalid-metadata',alg:'EdDSA',createdAt:new Date(),publicKey:JSON.stringify(invalidPublic),privateKey:JSON.stringify(invalidPrivate)};
const signed=await signJWT(context,{options:{jwks:{disablePrivateKeyEncryption:true},adapter:{getJwks:async()=>[validRecord]}},payload:{sub:'owner'}});
for(const malformed of [{...validRecord,alg:42},{...validRecord,alg:null,publicKey:'invalid JSON'}]) {
 const reference=await betterAuthReference(root,{jwt:{adapter:{getJwks:async()=>[malformed]}}});
 const auth=betterAuth({...reference.config,logger:{disabled:true}});
 assert.equal((await auth.api.verifyJWT({body:{token:signed}})).payload,null);
}
console.log('PASS actual malformed explicit algorithm and legacy public JSON return null verification payload');
const {decodeProtectedHeader}=await load('jose/dist/webapi/index.js');
for(const [overrideOptions,expected] of [[undefined,'ES256'],[{jwks:{}},'EdDSA']]) {
 const reference=await betterAuthReference(root,{jwt:{jwks:{keyPairConfig:{alg:'ES256'}}}});
 const auth=betterAuth({...reference.config,logger:{disabled:true}});
 const result=await auth.api.signJWT({body:{payload:{sub:'owner'},...(overrideOptions?{overrideOptions}:{})}});
 assert.equal(decodeProtectedHeader(result.token).alg,expected);
}
console.log('PASS actual configured ES256 and empty jwks override reset to EdDSA via shallow option merge');
