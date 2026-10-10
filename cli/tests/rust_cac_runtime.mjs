import {probeMcp,mcpRpc} from './rust_mcp_probe.mjs';
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign, createHmac, randomUUID } from 'node:crypto';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
const [binary, sdkPath] = process.argv.slice(2);
assert.ok(binary && sdkPath);
const secret = 'generated-cac-hmac-test-secret';
const issuer = 'https://trusted-issuer.example';
const audience = 'cac-test';
const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'cac-test-key', alg: 'RS256', use: 'sig' };
let jwksFetches = 0;
const jwks = http.createServer((_req, res) => { jwksFetches++; res.setHeader('content-type', 'application/json'); res.end(JSON.stringify({ keys: [jwk] })); });
await new Promise(resolve => jwks.listen(0, '127.0.0.1', resolve));
let child;
try {
  const env = { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('OTEL_'))),
    HOST: '127.0.0.1', SERVICE_PORT: '0', HMAC_SECRET_KEY: secret,
    JWKS_PUBLIC_KEY_URL: `http://127.0.0.1:${jwks.address().port}/jwks`, JWT_ISSUER: issuer, JWT_AUDIENCE: audience };
  child = spawn(binary, [], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '', closed = false;
  child.stdout.on('data', d => { stdout += d; });
  child.stderr.on('data', d => { stderr += d; });
  const exit = new Promise(resolve => child.on('close', code => { closed = true; resolve(code); }));
  let base;
  for (let i = 0; i < 200; i++) {
    base = stdout.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
    if (base) break;
    assert.ok(!closed, stderr);
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  assert.ok(base, stderr);
  function hmac(path, timestamp = new Date().toISOString(), method = 'GET') {
    const nonce = randomUUID();
    const signature = createHmac('sha256', secret).update(`${method}\n${path}\nundefined${timestamp}\n${nonce}`).digest('base64');
    return `HMAC keyId=default ts=${timestamp} nonce=${nonce} signature=${signature}`;
  }
  function jwt(roles = ['system'], overrides = {}) {
    const header = Buffer.from(JSON.stringify({ alg: 'RS256', kid: jwk.kid, typ: 'JWT' })).toString('base64url');
    const payload = Buffer.from(JSON.stringify({ sub: 'cac-system-user', exp: Math.floor(Date.now()/1000)+300, roles, iss: issuer, aud: audience, ...overrides })).toString('base64url');
    const signature = sign('RSA-SHA256', Buffer.from(`${header}.${payload}`), privateKey).toString('base64url');
    return `Bearer ${header}.${payload}.${signature}`;
  }
  await probeMcp(base,['describeCodeSet','lookupProcedureCode']);
  const exportMcp=await mcpRpc(base,'tools/call',{name:'exportUserData',arguments:{params:{userId:'user'}}},{authorization:jwt()});assert.equal(exportMcp.isError,true);assert.match(exportMcp.content[0].text,/404/);
  const forbiddenMcp=await mcpRpc(base,'tools/call',{name:'exportUserData',arguments:{params:{userId:'user'}}},{authorization:jwt(['admin'])});assert.equal(forbiddenMcp.isError,true);assert.match(forbiddenMcp.content[0].text,/403/);

  const deniedMcp=await mcpRpc(base,'tools/call',{name:'lookupProcedureCode',arguments:{params:{code:'PROC-001'}}});assert.equal(deniedMcp.isError,true);
  const foundMcp=await mcpRpc(base,'tools/call',{name:'lookupProcedureCode',arguments:{params:{code:'PROC-001'},headers:{authorization:hmac('/codeSet/PROC-001')}}});assert.equal(foundMcp.isError,false,JSON.stringify(foundMcp));assert.deepEqual(JSON.parse(foundMcp.content[0].text),{code:'PROC-001',description:'Office Visit'});
  const health = await fetch(`${base}/health`); assert.equal(health.status, 200); await health.arrayBuffer();
  for (const path of ['/codeSet', '/codeSet/PROC-001', '/compliance/export/user']) {
    const response = await fetch(`${base}${path}`); assert.equal(response.status, 401); await response.arrayBuffer();
  }
  const descriptor = await fetch(`${base}/codeSet`, { headers: { authorization: hmac('/codeSet') } });
  assert.deepEqual(await descriptor.json(), { codeSetType: 'mock', licensed: false });
  const code = await fetch(`${base}/codeSet/PROC-001`, { headers: { authorization: hmac('/codeSet/PROC-001') } });
  assert.deepEqual(await code.json(), { code: 'PROC-001', description: 'Office Visit' });
  const unknown = await fetch(`${base}/codeSet/unknown`, { headers: { authorization: hmac('/codeSet/unknown') } });
  assert.equal(unknown.status, 404); assert.equal(await unknown.text(), "Procedure code 'unknown' not found");
  for (const authorization of [hmac('/wrong'), hmac('/codeSet', '2020-01-01T00:00:00.000Z'), hmac('/codeSet').replace('keyId=default','keyId=unknown')]) {
    const response = await fetch(`${base}/codeSet`, { headers: { authorization } }); assert.equal(response.status, 401); await response.arrayBuffer();
  }
  for (const [method, path, text] of [['GET','/compliance/export/user','User not found or no PII data to export'], ['DELETE','/compliance/erase/user','User not found or no PII data to erase']]) {
    const response = await fetch(`${base}${path}`, { method, headers: { authorization: jwt() } });
    assert.equal(response.status, 404); assert.equal(await response.text(), text);
    const denied = await fetch(`${base}${path}`, { method, headers: { authorization: jwt(['admin']) } });
    assert.equal(denied.status, 403); await denied.arrayBuffer();
  }
  for (const overrides of [{ iss: 'https://attacker.example' }, { aud: 'other' }, { exp: 1 }, { nbf: Math.floor(Date.now()/1000)+600 }]) {
    const response = await fetch(`${base}/compliance/export/user`, { headers: { authorization: jwt(['system'], overrides) } });
    assert.equal(response.status, 401); await response.arrayBuffer();
  }
  assert.equal(jwksFetches, 1, 'shared JWKS verifier must cache keys across requests');
  const sdk = await import(pathToFileURL(sdkPath));
  const client = sdk.createForkLaunchSdk({ serviceBaseUrls: { cac: base } });
  const described = await client.cac.codeSet.describeCodeSet({ headers: { authorization: hmac('/codeSet') } });
  assert.equal(described.code, 200); assert.deepEqual(described.response, { codeSetType: 'mock', licensed: false });
  const found = await client.cac.codeSet.lookupProcedureCode({ params: { code: 'PROC-003' }, headers: { authorization: hmac('/codeSet/PROC-003') } });
  assert.equal(found.code, 200); assert.equal(found.response.description, 'Diagnostic Lab Panel');
  const missing = await client.cac.codeSet.lookupProcedureCode({ params: { code: 'unknown' }, headers: { authorization: hmac('/codeSet/unknown') } });
  assert.equal(missing.code, 404); assert.equal(missing.response, "Procedure code 'unknown' not found");
  const exported = await client.cac.compliance.exportUserData({ params: { userId: 'user' }, headers: { authorization: jwt() } });
  assert.equal(exported.code, 404); assert.equal(exported.response, 'User not found or no PII data to export');
  const rust = spawn('cargo', ['test', '-p', 'cac-sdk', '--test', 'runtime', '--', '--nocapture'], {
    env: { ...process.env, CAC_BASE_URL: base, CAC_DESCRIPTOR_HEADER: hmac('/codeSet'),
      CAC_LOOKUP_HEADER: hmac('/codeSet/PROC-002'), CAC_UNKNOWN_HEADER: hmac('/codeSet/unknown'), CAC_JWT_HEADER: jwt() },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let rustOutput = '';
  rust.stdout.on('data', d => { rustOutput += d; }); rust.stderr.on('data', d => { rustOutput += d; });
  const rustTimer = setTimeout(() => rust.kill('SIGKILL'), 120000);
  const rustCode = await new Promise(resolve => rust.on('close', resolve));
  clearTimeout(rustTimer); assert.equal(rustCode, 0, rustOutput);
  assert.equal(jwksFetches, 1);
  child.kill('SIGTERM');
  const timer = setTimeout(() => child.kill('SIGKILL'), 15000);
  assert.equal(await exit, 0, stderr); clearTimeout(timer);
  assert.ok(!stderr.includes(secret));
  console.log('PASS generated CAC HMAC signatures, JWKS caching, issuer/audience/expiry/nbf checks, system-role authorization, TypeScript/Rust SDK calls and SIGTERM');
} finally {
  if (child && child.exitCode === null) child.kill('SIGKILL');
  await new Promise(resolve => jwks.close(resolve));
}
