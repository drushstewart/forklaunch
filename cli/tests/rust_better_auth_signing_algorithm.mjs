import assert from 'node:assert/strict';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const prefix=process.env.FORKLAUNCH_TEST_BETTER_AUTH_DEPENDENCIES;assert.ok(prefix);
const load=relative=>import(pathToFileURL(path.join(prefix,'node_modules',relative)));
const {signJWT}=await load('better-auth/dist/plugins/jwt/sign.mjs');
const {generateKeyPair,exportJWK,jwtVerify,decodeProtectedHeader}=await load('jose/dist/webapi/index.js');
const pairs={},records={};
for(const alg of ['EdDSA','ES256']){const pair=await generateKeyPair(alg,{extractable:true});pairs[alg]=pair;records[alg]={id:alg,alg,createdAt:new Date(1000),publicKey:JSON.stringify(await exportJWK(pair.publicKey)),privateKey:JSON.stringify(await exportJWK(pair.privateKey))};}
const ctx={context:{options:{baseURL:'https://owned.test'},adapter:{}}};
const cases=[
 {name:'existing-extra',rows:[records.ES256],alg:'ES256',success:true},
 {name:'missing-unadvertised',rows:[records.EdDSA],alg:'ES256',success:false},
 {name:'advertised-extra',rows:[],alg:'ES256',extra:true,success:true,mint:true},
 {name:'primary',rows:[],alg:'EdDSA',success:true,mint:true},
 {name:'expired-advertised',rows:[{...records.ES256,expiresAt:new Date(0)}],alg:'ES256',extra:true,success:true,mint:true},
 {name:'expired-unadvertised',rows:[{...records.ES256,expiresAt:new Date(0)}],alg:'ES256',success:false},
 {name:'public-only',rows:[{...records.ES256,privateKey:undefined}],alg:'ES256',extra:true,success:false},
 {name:'id-mismatch',rows:[records.EdDSA,records.ES256],alg:'ES256',id:'EdDSA',extra:true,success:false},
 {name:'id-expired',rows:[{...records.ES256,expiresAt:new Date(0)}],alg:'ES256',id:'ES256',extra:true,success:false},
 {name:'returned-authority',rows:[],alg:'ES256',extra:true,success:true,mint:true,returned:'EdDSA'},
];
for(const item of cases){let reads=0,creates=0;const options={jwks:{disablePrivateKeyEncryption:true,keyPairConfig:{alg:'EdDSA'},...(item.extra?{keyPairConfigs:[{alg:'ES256'}]}:{})},adapter:{getJwks:async()=>{reads++;return item.rows;},createJwk:async row=>{creates++;assert.equal(row.alg,item.alg);return records[item.returned??item.alg];}}};const operation=signJWT(ctx,{options,payload:{sub:item.name},signingAlgorithm:item.alg,...(item.id?{signingKeyId:item.id}:{})});if(item.success){const token=await operation;const expected=item.returned??item.alg;assert.equal(decodeProtectedHeader(token).alg,expected);assert.equal((await jwtVerify(token,pairs[expected].publicKey)).payload.sub,item.name);}else await assert.rejects(operation);assert.equal(reads,1,item.name);assert.equal(creates,item.mint?1:0,item.name);}
console.log(`PASS ${cases.length} actual algorithm constraint cases: one read, advertised-only mint, existing extra reuse, expiry, public-only and ID failures, authoritative callback return`);
