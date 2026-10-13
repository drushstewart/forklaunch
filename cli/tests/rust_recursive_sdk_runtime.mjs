import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';
import { probeMcp, mcpRpc } from './rust_mcp_probe.mjs';

const server = spawn(process.argv[2], [], { env: { ...process.env, HOST: '127.0.0.1', SERVICE_HOST: '127.0.0.1', SERVICE_PORT: '0' }, stdio: ['ignore', 'pipe', 'pipe'] });
const exited = once(server, 'exit'); exited.catch(() => {});
let errors = ''; server.stderr.on('data', chunk => errors += chunk);
const timer = setTimeout(() => server.kill('SIGKILL'), 45000);
try {
  const baseUrl = await new Promise((resolve, reject) => {
    let output = '';
    server.stdout.on('data', chunk => { output += chunk; const match = output.match(/listening on (http:\/\/[^\s]+)/); if (match) resolve(match[1]); });
    server.once('exit', () => reject(Error(`Recursive service startup failed: ${errors}`)));
  });
  const tools = await probeMcp(baseUrl, ['readTree', 'writeTree', 'readForest', 'readChoice', 'readMutual']);
  const inputSchema = tools.find(tool => tool.name === 'writeTree').inputSchema;
  assert.equal(inputSchema.properties.body.examples[0].metadata.$ref, '#');
  if (process.env.FORKLAUNCH_TEST_SCHEMA_RESOURCES === 'dynamic') {
    assert.equal(inputSchema.properties.body.properties.children.items.$dynamicRef, '#node');
  } else if (process.env.FORKLAUNCH_TEST_SCHEMA_RESOURCES === 'recursive') {
    assert.equal(inputSchema.properties.body.properties.children.items.$recursiveRef, '#');
  } else {
    assert.equal(inputSchema.properties.body.properties.children.items.$ref, process.env.FORKLAUNCH_TEST_SCHEMA_RESOURCES ? '#' : '#/properties/body');
  }
  for (const [tool, route] of [['readTree', 'trees'], ['readForest', 'forests'], ['readChoice', 'choices'], ['readMutual', 'mutual']]) {
    const http = await (await fetch(`${baseUrl}/${route}`)).json();
    const result = await mcpRpc(baseUrl, 'tools/call', { name: tool, arguments: {} });
    assert.equal(result.isError, false, JSON.stringify(result));
    assert.deepEqual(JSON.parse(result.content[0].text), http);
  }
  const tree = await (await fetch(`${baseUrl}/trees`)).json();
  const echo = await mcpRpc(baseUrl, 'tools/call', { name: 'writeTree', arguments: { body: tree } });
  assert.equal(echo.isError, false, JSON.stringify(echo));
  assert.deepEqual(JSON.parse(echo.content[0].text), tree);
  const { verify } = await import(pathToFileURL(process.argv[4]));
  await verify(baseUrl);
  const rust = spawn(process.argv[3], [baseUrl], { stdio: ['ignore', 'pipe', 'pipe'] });
  let output = ''; rust.stdout.on('data', chunk => output += chunk); rust.stderr.on('data', chunk => output += chunk);
  const rustTimer = setTimeout(() => rust.kill('SIGKILL'), 20000);
  try { const [code, signal] = await once(rust, 'exit'); assert.equal(code, 0, `${signal} ${output}`); assert.match(output, /PASS typed Rust SDK recursive/); }
  finally { clearTimeout(rustTimer); }
  server.kill('SIGTERM');
  const [code, signal] = await exited;
  assert.equal(code, 0, `${signal} ${errors}`);
  console.log('PASS both live recursive SDKs, MCP tool discovery and graceful HTTP/MCP shutdown');
} finally {
  clearTimeout(timer);
  if (server.exitCode === null && server.signalCode === null) { server.kill('SIGKILL'); await exited.catch(() => {}); }
}
