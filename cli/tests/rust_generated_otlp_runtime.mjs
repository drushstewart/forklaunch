import { probeMcp } from './rust_mcp_probe.mjs';
import assert from 'node:assert/strict';
import http from 'node:http';
import { spawn } from 'node:child_process';
const binary = process.argv[2];
assert.ok(binary, 'generated service executable required');
const requests = [];
const receiver = http.createServer((req, res) => {
  const chunks = [];
  req.on('data', c => chunks.push(c));
  req.on('end', () => { requests.push({ path: req.url, data: JSON.parse(Buffer.concat(chunks)) }); res.setHeader('content-type', 'application/json'); res.end('{}'); });
});
await new Promise(resolve => receiver.listen(0, '127.0.0.1', resolve));
let child;
try {
  const env = { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('OTEL_') && !key.startsWith('MCP_'))),
    HOST: '127.0.0.1', SERVICE_PORT: '0', RUST_LOG: 'debug', OTEL_EXPORTER_OTLP_PROTOCOL: 'http/json',
    OTEL_EXPORTER_OTLP_ENDPOINT: `http://127.0.0.1:${receiver.address().port}`,
    OTEL_BSP_SCHEDULE_DELAY: '600000', OTEL_BLRP_SCHEDULE_DELAY: '600000', OTEL_METRIC_EXPORT_INTERVAL: '600000' };
  child = spawn(binary, [], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '', stderr = '', closed = false;
  child.stdout.on('data', d => { stdout += d; });
  child.stderr.on('data', d => { stderr += d; });
  const exit = new Promise(resolve => child.on('close', code => { closed = true; resolve(code); }));
  let address;
  for (let i = 0; i < 200; i++) {
    address = stdout.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/)?.[1];
    if (address) break;
    assert.ok(!closed, stderr);
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  assert.ok(address, stderr);
  await probeMcp(address);
  const correlation = 'telemetry-fixture-correlation';
  const traceId = '1234567890abcdef1234567890abcdef';
  const response = await fetch(`${address}/health`, { headers: { traceparent: `00-${traceId}-0123456789abcdef-01`, 'x-correlation-id': correlation } });
  assert.equal(response.status, 200);
  await response.arrayBuffer();
  // A response error emits a structured application log through the same runtime.
  const missing = await fetch(`${address}/health`, { method: 'POST' });
  assert.equal(missing.status, 405);
  await missing.arrayBuffer();
  child.kill('SIGTERM');
  const timeout = setTimeout(() => child.kill('SIGKILL'), 20000);
  assert.equal(await exit, 0, stderr);
  clearTimeout(timeout);
  const spans = requests.filter(r => r.path === '/v1/traces').flatMap(r => r.data.resourceSpans.flatMap(x => x.scopeSpans.flatMap(s => s.spans)));
  assert.ok(spans.some(s => s.name === 'GET /health' && s.traceId === traceId && s.parentSpanId === '0123456789abcdef'), JSON.stringify(spans));
  const metrics = requests.filter(r => r.path === '/v1/metrics').flatMap(r => r.data.resourceMetrics.flatMap(x => x.scopeMetrics.flatMap(s => s.metrics)));
  const points = metric => (metric.sum ?? metric.histogram ?? metric.gauge)?.dataPoints ?? [];
  const count = metrics.find(m => m.name === 'http.server.request.count');
  assert.ok(count);
  assert.ok(points(count).some(p => p.attributes.some(a => a.key === 'http.response.status_code' && String(a.value.intValue) === '405')));
  assert.ok(metrics.some(m => m.name === 'http.server.response.body.size' && points(m).some(p => Number(p.sum) > 0)));
  assert.ok(metrics.some(m => m.name === 'http.server.request.duration' && points(m).length > 0));
  const active = metrics.find(m => m.name === 'http.server.active_requests');
  assert.ok(active);
  assert.ok(points(active).every(p => Number(p.asInt) === 0), JSON.stringify(active));
  assert.ok(metrics.every(m => points(m).every(p => !p.attributes.some(a => a.key === 'forklaunch.correlation_id' || JSON.stringify(a).includes(correlation)))));
  assert.ok(spans.some(s => s.traceId === traceId && s.attributes.some(a => a.key === 'forklaunch.correlation_id' && a.value.stringValue === correlation)));
  const logs = requests.filter(r => r.path === '/v1/logs').flatMap(r => r.data.resourceLogs.flatMap(x => x.scopeLogs.flatMap(s => s.logRecords)));
  assert.ok(logs.some(l => l.body.stringValue === 'request returned an error response'), JSON.stringify(logs));
  console.log('PASS generated HTTP/MCP service: OTLP traces/W3C correlation, status/size/duration metrics, zero active requests, bounded metric dimensions, error logs and SIGTERM flush');
} finally {
  if (child && child.exitCode === null) child.kill('SIGKILL');
  await new Promise(resolve => receiver.close(resolve));
}
