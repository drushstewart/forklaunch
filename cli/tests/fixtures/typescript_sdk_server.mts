// A small TypeScript HTTP wire fixture. Full ForkLaunch runtime conformance is
// tracked separately; this checks generated clients across language boundaries.
import { createServer } from 'node:http';

const server = createServer((request, response) => {
  const url = new URL(request.url ?? '/', 'http://fixture.invalid');
  if (request.method === 'GET' && url.pathname === '/health') {
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
