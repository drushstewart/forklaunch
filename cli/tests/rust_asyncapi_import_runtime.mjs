import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const [root, binary] = process.argv.slice(2);
const require = createRequire(
  path.join(process.env.FORKLAUNCH_TEST_VALIDATOR_DEPENDENCIES, 'package.json'),
);
const { WebSocketServer } = require('ws');
const parserRequire = createRequire(
  require.resolve(process.env.ASYNCAPI_PARSER_PATH ?? '@asyncapi/parser'),
);
const { Parser } = parserRequire('@asyncapi/parser');
const parsed = await new Parser().parse(
  await readFile(
    path.join(root, 'src/realtime/sdk/typescript/asyncapi.json'),
    'utf8',
  ),
);
assert.ok(parsed.document, JSON.stringify(parsed.diagnostics));
assert.equal(
  parsed.diagnostics.filter((diagnostic) => diagnostic.severity === 0).length,
  0,
);
const server = new WebSocketServer({
  host: '127.0.0.1',
  port: 0,
  path: '/chat',
});
await once(server, 'listening');
let received = 0;
server.on('connection', (socket) => {
  socket.on('message', (data) => {
    const value = JSON.parse(data.toString());
    if (value.kind === 'request') assert.equal(value.message, 'hello 水 🦀');
    else {
      assert.equal(value.kind, 'other');
      assert.equal(value.count, 2);
    }
    received++;
    socket.send(JSON.stringify({ kind: 'reply', accepted: true }));
  });
});
let client, child;
try {
  const { SocketClient } = await import(
    pathToFileURL(
      path.join(root, 'src/client-sdk/typescript-websocket/realtime/dist/socket_chat.js'),
    )
  );
  const url = `ws://127.0.0.1:${server.address().port}/chat`;
  client = await SocketClient.connect(url);
  await assert.rejects(client.send({ kind: 'other', count: 0 }));
  await assert.rejects(client.send({ kind: 'reply', accepted: true }));
  for (const value of [
    { kind: 'request', message: 'hello 水 🦀' },
    { kind: 'other', count: 2 },
  ]) {
    await client.send(value);
    assert.deepEqual(await client.recv({ timeoutMs: 5000 }), {
      type: 'message',
      value: { kind: 'reply', accepted: true },
    });
  }
  await client.close(1000);
  child = spawn(binary, [url], { stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', (data) => (output += data));
  child.stderr.on('data', (data) => (output += data));
  const timer = setTimeout(() => child.kill('SIGKILL'), 30000);
  try {
    assert.equal((await once(child, 'close'))[0], 0, output);
  } finally {
    clearTimeout(timer);
  }
  assert.equal(received, 4);
  console.log(
    'PASS AsyncAPI server directions, multi-message constraints and live Rust/TypeScript peers',
  );
} finally {
  child?.kill('SIGKILL');
  for (const socket of server.clients) socket.terminate();
  await new Promise((resolve) => server.close(resolve));
}
