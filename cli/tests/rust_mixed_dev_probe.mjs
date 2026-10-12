import assert from 'node:assert/strict';
import { probeMcp } from './rust_mcp_probe.mjs';
const [native, typescript, mcp, expected] = process.argv.slice(2);
await probeMcp(native);
assert.equal(
  (await (await fetch(`${typescript}/legacy-api`)).json()).message,
  expected
);
let session,
  id = 0;
async function rpc(method, params = {}) {
  const response = await fetch(mcp, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      accept: 'application/json, text/event-stream',
      ...(session ? { 'mcp-session-id': session } : {})
    },
    body: JSON.stringify({ jsonrpc: '2.0', id: ++id, method, params }),
    signal: AbortSignal.timeout(10000)
  });
  const text = await response.text();
  assert.equal(response.status, 200, text);
  session = response.headers.get('mcp-session-id') ?? session;
  const data = response.headers
    .get('content-type')
    ?.includes('text/event-stream')
    ? text
        .split('\n')
        .filter((line) => line.startsWith('data:'))
        .map((line) => JSON.parse(line.slice(5)))
        .find((item) => item.id === id)
    : JSON.parse(text);
  assert.ok(data && !data.error, text);
  return data.result;
}
assert.ok(
  (
    await rpc('initialize', {
      protocolVersion: '2024-11-05',
      capabilities: {},
      clientInfo: { name: 'mixed-dev', version: '1' }
    })
  ).capabilities.tools
);
const { tools } = await rpc('tools/list');
const tool = tools.find((tool) => tool.name === 'Legacy Api Get');
assert.ok(tool);
const result = await rpc('tools/call', { name: tool.name, arguments: {} });
assert.ok(!result.isError, JSON.stringify(result));
assert.ok(
  result.content.some(
    (item) => item.type === 'text' && item.text.includes(expected)
  ),
  JSON.stringify(result)
);
console.log(`PASS mixed development HTTP/MCP responses: ${expected}`);
