// Conformance §4: the Host header, the Origin allowlist, the preflight.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { start } from './harness.mjs';

let c;
before(async () => { c = await start({ name: 'origins' }); await c.pair(); });
after(async () => { await c?.stop(); });

test('a foreign origin is 403 origin_refused, also with a token; null too', async () => {
  for (const origin of ['https://evil.example', 'http://localhost.evil.example', 'null', 'http://192.168.1.5:8080']) {
    const r = await c.call('GET', '/v1/workspace', undefined, { headers: { origin } });
    assert.equal(r.status, 403, origin);
    assert.equal(r.json.code, 'origin_refused', origin);
    assert.equal(r.headers.get('access-control-allow-origin'), null, 'no CORS grant for it');
  }
});

test('an allowed origin is echoed, never *, and varies by Origin', async () => {
  for (const origin of ['http://localhost:5173', 'http://127.0.0.1:9999', 'https://kaush4l.github.io']) {
    const r = await c.call('GET', '/v1/health', undefined, { headers: { origin } });
    assert.equal(r.status, 200, origin);
    assert.equal(r.headers.get('access-control-allow-origin'), origin);
    assert.match(r.headers.get('vary') || '', /Origin/i);
  }
});

test('the preflight: 204, the headers, Private-Network', async () => {
  const r = await c.raw('OPTIONS', '/v1/fs/read', {
    host: `127.0.0.1:${c.port}`, origin: 'https://kaush4l.github.io', 'access-control-request-method': 'GET',
    'access-control-request-headers': 'x-dry-token', 'access-control-request-private-network': 'true',
  });
  assert.equal(r.status, 204);
  assert.equal(r.headers['access-control-allow-origin'], 'https://kaush4l.github.io');
  assert.equal(r.headers['access-control-allow-private-network'], 'true');
  const allowed = (r.headers['access-control-allow-headers'] || '').toLowerCase();
  for (const h of ['content-type', 'authorization', 'x-dry-token', 'last-event-id', 'mcp-protocol-version', 'accept']) assert.ok(allowed.includes(h), h);
  assert.match(r.headers['access-control-allow-methods'] || '', /POST/);
  const bad = await c.raw('OPTIONS', '/v1/fs/read', { host: `127.0.0.1:${c.port}`, origin: 'https://evil.example' });
  assert.equal(bad.status, 403);
});

test('a Host that is not loopback is 403 host_refused (DNS rebinding)', async () => {
  for (const host of [`evil.example:${c.port}`, `127.0.0.1:${c.port + 1}`, 'localhost']) {
    const r = await c.raw('GET', '/v1/health', { host });
    assert.equal(r.status, 403, host);
    assert.equal(r.json.code, 'host_refused', host);
  }
  for (const host of [`127.0.0.1:${c.port}`, `localhost:${c.port}`]) assert.equal((await c.raw('GET', '/v1/health', { host })).status, 200, host);
});
