// A small TypeScript HTTP wire fixture. Full ForkLaunch runtime conformance is
// tracked separately; this checks generated clients across language boundaries.
import { createServer } from 'node:http';

const server = createServer((request, response) => {
  const url = new URL(request.url ?? '/', 'http://fixture.invalid');
  if (request.method === 'GET' && url.pathname.startsWith('/status/')) {
    const mode = url.pathname.slice('/status/'.length);
    if (mode === 'empty' || mode === 'reset') {
      response.writeHead(mode === 'empty' ? 204 : 205);
      response.end();
    } else {
      const status = { existing: 200, created: 201, queued: 202 }[mode as 'existing' | 'created' | 'queued'];
      const payload = { existing: { id: 'existing', displayName: 'Existing' }, created: { id: 'created', createdAt: '2026-10-09T00:00:00Z' }, queued: { jobId: 'queued-1' } }[mode as 'existing' | 'created' | 'queued'];
      response.writeHead(status ?? (mode === 'conflict' ? 409 : 400), { 'content-type': status || mode === 'conflict' ? 'application/json' : 'application/problem+json' });
      response.end(JSON.stringify(payload ?? (mode === 'conflict' ? { errorCode: 'conflict', retryAfter: 30 } : { errorCode: 'invalid_mode', message: 'Unknown mode' })));
    }
  } else if (url.pathname.startsWith('/shape/')) {
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify(url.pathname.endsWith('/full') ? {"label": "present", "numbers": {"one": 1, "missing": null}, "metadata": {"id": "meta"}, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": "seven", "list": [1, "two", null]} : {"label": null, "numbers": {"one": 1, "missing": null}, "metadata": null, "open": {"id": "open", "extra": {"nested": true}}, "constrained": {"count": 2, "extra": "text"}, "additional_properties": "explicit", "unknownExtra": [1, false], "any": 7, "list": [1, "two", null]}));
  } else if (url.pathname === '/collision') {
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify({"fooBar": {"first": "one"}, "foo_bar": {"second": 2}, "foo_bar2": {"third": true}, "foo_bar_2": {"fourth": "four"}, "type": "kind", "type_": 9, "gen": "future", "_": "underscore", "quote\"\\": "escaped"}));
  } else if (url.pathname === '/text') {
    response.writeHead(200, { 'content-type': 'text/plain; charset=utf-8' });
    response.end('Hello\nRust 🦀');
  } else if (url.pathname === '/binary') {
    response.writeHead(200, { 'content-type': 'application/octet-stream' });
    response.end(Buffer.from([0, 255, 13, 10, 42]));
  } else if (url.pathname === '/unknown') {
    response.writeHead(200, { 'content-type': 'application/json' });
    response.end(JSON.stringify([1, 'two', { three: true }]));
  } else if (request.method === 'DELETE' && url.pathname.startsWith('/users/')) {
    response.writeHead(204);
    response.end();
  } else if ((request.method === 'HEAD' && url.pathname === '/health') || url.pathname === '/empty') {
    response.writeHead(200);
    response.end();
  } else if (request.method === 'GET' && url.pathname === '/null-json') {
    response.setHeader('content-type', 'application/json');
    response.end('null');
  } else if (request.method === 'GET' && url.pathname === '/health') {
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify({ service: 'accounts', status: 'ok', recordCount: 1, routes: ['health', 'users'] }));
  } else if (request.method === 'GET' && url.pathname === '/users') {
    if (request.headers['x-sdk-consumer'] === 'rust') {
      if (url.searchParams.get('label') !== 'hello world & rust' || request.headers['x-request-header'] !== 'present') {
        response.writeHead(400, { 'content-type': 'text/plain' });
        response.end('Rust SDK query/header transport mismatch');
        return;
      }
    }
    response.setHeader('content-type', 'application/json');
    response.end(JSON.stringify({ route: 'users', summary: { service: 'accounts', total: 1 }, records: [{ id: 'welcome', name: 'accounts is ready' }] }));
  } else {
    response.writeHead(404, { 'content-type': 'text/plain' });
    response.end('Not found');
  }
});
server.listen(0, '127.0.0.1', () => {
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('TCP address required');
  console.log(`accounts listening on http://127.0.0.1:${address.port}`);
});
process.once('SIGTERM', () => server.close(error => { process.exitCode = error ? 1 : 0; }));
