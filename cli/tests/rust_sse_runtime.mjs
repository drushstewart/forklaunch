import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once, getEventListeners } from 'node:events';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { probeMcp } from './rust_mcp_probe.mjs';
const server = spawn(process.argv[2], [], { env: { ...process.env, HOST: '127.0.0.1', SERVICE_HOST: '127.0.0.1', SERVICE_PORT: '0' }, stdio: ['ignore', 'pipe', 'pipe'] });
const exited = once(server, 'exit');
exited.catch(() => {});
let errors = '';
server.stderr.on('data', chunk => errors += chunk);
function deadline(promise, ms = 3000) {
  let timeout;
  const timeoutError = Error('SSE fixture deadline');
  return Promise.race([promise, new Promise((_, reject) => { timeout = setTimeout(() => reject(timeoutError), ms); })]).finally(() => clearTimeout(timeout));
}
try {
  const baseUrl = await deadline(new Promise((resolve, reject) => {
    let output = '';
    server.stdout.on('data', chunk => { output += chunk; const match = output.match(/listening on (http:\/\/[^\s]+)/); if (match) resolve(match[1]); });
    server.once('exit', () => reject(Error(`SSE service startup failed: ${errors}`)));
  }), 15000);
  await probeMcp(baseUrl, ['watchEvents']);
  const { createForkLaunchSdk: factory } = await import(pathToFileURL(process.argv[4]));
  const fetcher = process.env.FORKLAUNCH_TEST_FETCH_MODULE ? createRequire(import.meta.url)(process.env.FORKLAUNCH_TEST_FETCH_MODULE) : undefined;
  const createForkLaunchSdk = config => factory({ ...config, ...(fetcher ? { fetch: fetcher } : {}) });
  let hooks = 0;
  const config = { baseUrl, headers: { authorization: 'Bearer stream-secret' }, beforeRequest: meta => { hooks++; return meta; } };
  const sdk = createForkLaunchSdk(config);
  const call = (client, mode, options = {}) => client.streams.events.watchEvents({ params: { mode }, ...options });
  const stats = async () => (await fetch(`${baseUrl}/stream-stats`)).json();
  const before = await stats();
  assert.equal((await call(createForkLaunchSdk({ baseUrl }), 'finite')).code, 401);
  assert.equal((await stats()).started, before.started);
  for (const mode of ['finite', 'multiline']) {
    const response = await call(sdk, mode);
    assert.equal(response.code, 200);
    const events = [];
    for await (const event of response.response) events.push(event);
    assert.deepEqual(events, [ { data: { label: '水 🦀' }, id: 'first' }, { data: mode === 'finite' ? 2 : false, id: mode === 'finite' ? 'first' : '' } ]);
  }
  const slow = (await call(sdk, 'slow')).response[Symbol.asyncIterator]();
  assert.deepEqual((await deadline(slow.next())).value, { data: { label: '水 🦀' }, id: 'first' });
  await deadline(slow.return());
  assert.equal((await slow.next()).done, true);
  const pendingStream = (await call(sdk, 'slow')).response[Symbol.asyncIterator]();
  await pendingStream.next();
  await pendingStream.next();
  const pendingRead = pendingStream.next();
  const pendingOutcome = pendingRead.then(value => ({ value }), error => ({ error }));
  await new Promise(resolve => setTimeout(resolve, 25));
  await deadline(pendingStream.return());
  const outcome = await deadline(pendingOutcome);
  assert.ok(outcome.error || outcome.value.done, 'Cancellation must terminate an outstanding read');
  const completedSignal = new AbortController();
  const completed = await call(sdk, 'finite', { signal: completedSignal.signal });
  for await (const event of completed.response) assert.ok(event);
  assert.equal(getEventListeners(completedSignal.signal, 'abort').length, 0, 'Completed streams release caller-signal listeners');
  const unstarted = (await call(sdk, 'slow')).response[Symbol.asyncIterator]();
  await deadline(unstarted.return());
  const thrown = (await call(sdk, 'slow')).response[Symbol.asyncIterator]();
  await assert.rejects(thrown.throw(Error('owned cancellation')), /owned cancellation/);
  const truncated = (await call(sdk, 'truncated')).response[Symbol.asyncIterator]();
  assert.equal((await truncated.next()).done, true);
  const oversized = (await call(createForkLaunchSdk({ ...config, maxSseEventBytes: 32 }), 'oversize')).response[Symbol.asyncIterator]();
  await assert.rejects(oversized.next(), /exceeds configured limit/);
  assert.equal((await oversized.next()).done, true);
  const late = (await call(createForkLaunchSdk({ ...config, maxSseEventBytes: 128 }), 'late-oversize')).response[Symbol.asyncIterator]();
  assert.deepEqual((await late.next()).value, { data: { label: '水 🦀' } });
  await assert.rejects(late.next(), /exceeds configured limit/);
  const invalidLimit = (await call(createForkLaunchSdk({ ...config, maxSseEventBytes: 0 }), 'slow')).response[Symbol.asyncIterator]();
  await assert.rejects(invalidLimit.next(), /limit must be positive/);
  await assert.rejects(call(sdk, 'wrong'), /Expected text\/event-stream/);
  const broken = (await call(sdk, 'broken')).response[Symbol.asyncIterator]();
  assert.equal((await broken.next()).done, false);
  assert.equal((await broken.next()).done, false);
  await assert.rejects(broken.next());
  const abort = new AbortController();
  const aborted = (await call(sdk, 'slow', { signal: abort.signal })).response[Symbol.asyncIterator]();
  await aborted.next();
  abort.abort();
  await assert.rejects(deadline((async () => { while (!(await aborted.next()).done) {} })()));
  assert.ok(hooks > 5);
  const rust = spawn(process.argv[3], [baseUrl], { stdio: ['ignore', 'pipe', 'pipe'] });
  let rustOutput = '';
  rust.stdout.on('data', chunk => rustOutput += chunk);
  rust.stderr.on('data', chunk => rustOutput += chunk);
  const rustDeadline = setTimeout(() => rust.kill('SIGKILL'), 20000);
  try { const [code, signal] = await once(rust, 'exit'); assert.equal(code, 0, `${signal} ${rustOutput}`); assert.match(rustOutput, /PASS Rust SDK incremental SSE/); }
  finally { clearTimeout(rustDeadline); }
  await deadline((async () => {
    while (true) {
      const current = await stats();
      if (current.started === current.closed) break;
      await new Promise(resolve => setTimeout(resolve, 25));
    }
  })());
  server.kill('SIGTERM');
  const [code, signal] = await deadline(exited, 10000);
  assert.equal(code, 0, `${signal} ${errors}`);
  console.log(`Transport: ${fetcher ? 'cross-fetch Node stream' : 'SDK default fetch'}`);
  console.log('PASS live generated TypeScript/Rust SSE clients: delivery before EOF, split Unicode, multiline/ID reset, auth before work, event limits, EOF, wrong media, transport failures, abort/early cancellation, resource release and graceful HTTP/MCP shutdown');
} finally {
  if (server.exitCode === null && server.signalCode === null) { server.kill('SIGKILL'); await exited.catch(() => {}); }
}
