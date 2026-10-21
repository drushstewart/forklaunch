import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const [root, binary] = process.argv.slice(2);
const require = createRequire(path.join(process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES, 'package.json'));
const { WebSocketServer, WebSocket } = require('ws');
const parserRequire = createRequire(require.resolve(process.env.ASYNCAPI_PARSER_PATH ?? '@asyncapi/parser'));
const { Parser } = parserRequire('@asyncapi/parser');
const parsed = await new Parser().parse(await readFile(path.join(root, '.forklaunch/asyncapi/realtime/asyncapi.json'), 'utf8'));
assert.ok(parsed.document, JSON.stringify(parsed.diagnostics));
assert.equal(parsed.diagnostics.filter(d => d.severity === 0).length, 0);

// Application-owned routing in this serial fixture connects each request to the
// independently opened reply socket. The SDK provides no implicit correlation.
const server = new WebSocketServer({ host: '127.0.0.1', port: 0 });
await once(server, 'listening');
let replySocket, received = 0, replies = 0, child;
const clients = [];
server.on('connection', (socket, request) => {
  assert.ok(['/chat', '/responses'].includes(request.url));
  if (request.url === '/responses') replySocket = socket;
  socket.on('message', data => {
    assert.equal(request.url, '/chat');
    const value = JSON.parse(data.toString());
    if (value.kind === 'request') assert.equal(value.message, 'hello 水 🦀');
    else assert.deepEqual(value, { kind: 'other', count: 2 });
    assert.equal(replySocket?.readyState, WebSocket.OPEN);
    received++;
    replySocket.send(JSON.stringify({ kind: 'reply', accepted: true }));
    replies++;
  });
});
try {
  const moduleUrl = name => pathToFileURL(path.join(root, `src/client-sdk/typescript-websocket/realtime/dist/socket_${name}.js`));
  const { SocketClient: RequestClient } = await import(moduleUrl('chat'));
  const { SocketClient: ReplyClient } = await import(moduleUrl('responses'));
  const base = `ws://127.0.0.1:${server.address().port}`;
  const responses = await ReplyClient.connect(`${base}/responses`); clients.push(responses);
  const requests = await RequestClient.connect(`${base}/chat`); clients.push(requests);
  await assert.rejects(requests.send({ kind: 'other', count: 0 }));
  await assert.rejects(requests.send({ kind: 'reply', accepted: true }));
  await assert.rejects(responses.send({ kind: 'reply', accepted: true }));
  for (const value of [{ kind: 'request', message: 'hello 水 🦀' }, { kind: 'other', count: 2 }]) {
    await requests.send(value);
    assert.deepEqual(await responses.recv({ timeoutMs: 5000 }), { type: 'message', value: { kind: 'reply', accepted: true } });
  }
  await requests.close(1000); await responses.close(1000);
  child = spawn(binary, [base], { stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', data => output += data);
  child.stderr.on('data', data => output += data);
  const timer = setTimeout(() => child.kill('SIGKILL'), 30000);
  try { assert.equal((await once(child, 'close'))[0], 0, output); }
  finally { clearTimeout(timer); }
  assert.equal(received, 4); assert.equal(replies, 4);
  console.log('PASS official parser and imported Rust/TypeScript clients across independent fixed request/reply paths; forbidden directions and payload constraints');
} finally {
  child?.kill('SIGKILL');
  for (const client of clients) {
    try { await client.close(1000); } catch { /* Already closed by the success path. */ }
  }
  for (const socket of server.clients) socket.terminate();
  await new Promise(resolve => server.close(resolve));
}
