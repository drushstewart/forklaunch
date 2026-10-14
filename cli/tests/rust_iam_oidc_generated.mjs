import assert from 'node:assert/strict';
import http from 'node:http';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {dirname,join} from 'node:path';
import {generateKeyPairSync,sign,createHash} from 'node:crypto';
import {probeMcp} from './rust_mcp_probe.mjs';

export async function probeGeneratedOidc(start,binary) {
 const {privateKey,publicKey}=generateKeyPairSync('rsa',{modulusLength:2048});
 const jwk={...publicKey.export({format:'jwk'}),kid:'provider-fixture',alg:'RS256',use:'sig'};
 let issuer,nonce,challenge,exchanges=0;
 const provider=http.createServer(async(req,res)=>{
  res.setHeader('content-type','application/json');
  if(req.url==='/.well-known/openid-configuration')res.end(JSON.stringify({issuer,
   authorization_endpoint:issuer+'/authorize',token_endpoint:issuer+'/token',jwks_uri:issuer+'/jwks',
   response_types_supported:['code'],subject_types_supported:['public'],
   id_token_signing_alg_values_supported:['RS256'],token_endpoint_auth_methods_supported:['none']}));
  else if(req.url==='/jwks')res.end(JSON.stringify({keys:[jwk]}));
  else if(req.url==='/token'&&req.method==='POST'){
   let raw='';for await(const chunk of req)raw+=chunk;
   const form=new URLSearchParams(raw);
   if(form.get('code')!=='owned-code'||form.get('client_id')!=='generated-fixture'||
      form.get('redirect_uri')!=='https://iam.example.test/auth/browser/oidc/callback'||
      createHash('sha256').update(form.get('code_verifier')??'').digest('base64url')!==challenge){
    res.writeHead(400);res.end('{}');return;
   }
   exchanges++;
   const header=Buffer.from(JSON.stringify({alg:'RS256',kid:jwk.kid})).toString('base64url');
   const now=Math.floor(Date.now()/1000);
   const payload=Buffer.from(JSON.stringify({iss:issuer,sub:'provider-user',aud:'generated-fixture',
    exp:now+300,iat:now,nonce,roles:['system']})).toString('base64url');
   const token=header+'.'+payload+'.'+sign('RSA-SHA256',Buffer.from(header+'.'+payload),privateKey).toString('base64url');
   res.end(JSON.stringify({access_token:'provider-access-only',token_type:'Bearer',id_token:token}));
  }else {res.writeHead(404);res.end('{}');}
 });
 await new Promise(resolve=>provider.listen(0,'127.0.0.1',resolve));
 issuer=`http://127.0.0.1:${provider.address().port}`;
 const config={SESSION_OIDC_ISSUER:issuer,SESSION_OIDC_CLIENT_ID:'generated-fixture'};
 let current;
 const origin='https://iam.example.test';
 async function request(path,{method='GET',cookie='',proof='',body}={}){
  return fetch(current.baseUrl+'/auth/browser/'+path,{method,headers:{origin,
   ...(cookie?{cookie}:{}),...(proof?{'x-csrf-token':proof}:{}),
   ...(body!==undefined?{'content-type':'application/json'}:{})},
   body:body===undefined?undefined:JSON.stringify(body),signal:AbortSignal.timeout(15000)});
 }
 const cookieOf=response=>response.headers.get('set-cookie')?.split(';')[0];
 async function stop(){if(current){current.child.kill('SIGTERM');assert.deepEqual(await current.exited,[0,null]);current=undefined;}}
 try{
  await promisify(execFile)(join(dirname(binary),'examples','seed'),['--bind-oidc',issuer],{timeout:30000});
  current=await start('a',false,config);
  await probeMcp(current.baseUrl,['listUsers']);
  const initial=await request('csrf');assert.equal(initial.status,200);
  const cookie=cookieOf(initial),proof=(await initial.json()).csrfToken;assert.ok(cookie);
  assert.equal((await request('oidc/start',{method:'POST',cookie,proof:'wrong'})).status,403);
  const begun=await request('oidc/start',{method:'POST',cookie,proof});assert.equal(begun.status,200);
  const authorization=new URL((await begun.json()).authorizationUrl);
  assert.equal(authorization.origin,issuer);assert.equal(authorization.searchParams.get('code_challenge_method'),'S256');
  nonce=authorization.searchParams.get('nonce');challenge=authorization.searchParams.get('code_challenge');
  const state=authorization.searchParams.get('state');assert.ok(nonce&&challenge&&state);
  await stop();current=await start('a',false,config);
  const relay=await request('oidc/callback?code=secret&state=secret');assert.equal(relay.status,200);
  assert.equal(relay.headers.get('set-cookie'),null);assert.equal(relay.headers.get('referrer-policy'),'no-referrer');
  const body={code:'owned-code',state};
  const result=await request('oidc/callback',{method:'POST',cookie,proof,body});assert.equal(result.status,200,await result.clone().text());
  const authenticated=cookieOf(result),rotated=(await result.json()).csrfToken;
  assert.ok(authenticated);assert.notEqual(authenticated,cookie);assert.notEqual(rotated,proof);
  const session=await request('session',{cookie:authenticated});assert.equal(session.status,200);
  const claims=await session.json();assert.equal(claims.sub,'user-1');assert.deepEqual(claims.permissions,['users:read']);
  assert.ok(!claims.roles.includes('system'),'Provider claims cannot grant local roles');
  assert.equal((await request('oidc/callback',{method:'POST',cookie:authenticated,proof:rotated,body})).status,401);
  assert.equal(exchanges,1,'Consumed state must not replay a provider exchange');
  assert.equal((await request('logout',{method:'POST',cookie:authenticated,proof:rotated})).status,204);
  assert.equal((await request('session',{cookie:authenticated})).status,401);
  console.log('PASS generated OIDC discovery/PKCE/code exchange, durable pending restart, local grants, cookie rotation, replay denial, logout and HTTP/MCP startup');
 }finally{
  await stop();await new Promise(resolve=>{provider.close(resolve);provider.closeAllConnections();});
 }
}
