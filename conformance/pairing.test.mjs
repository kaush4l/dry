// Conformance §5: pairing, tokens, unpair, revoke (docs/companion-protocol.md).
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { start, until } from './harness.mjs';

let c;
before(async () => { c = await start({ name: 'pairing' }); });
after(async () => { await c?.stop(); });

test('health answers without a token: protocol 1, not paired', async () => {
  const r = await c.call('GET', '/v1/health', undefined, { token: null });
  assert.equal(r.status, 200);
  assert.equal(r.json.ok, true);
  assert.equal(r.json.protocol, 1);
  assert.equal(r.json.paired, false);
  assert.equal(typeof r.json.name, 'string');
});

test('the code is printed and written: six characters of the alphabet', async () => {
  assert.match(c.code(), /^[A-HJ-NP-Z2-9]{6}$/);
  assert.match(c.output(), new RegExp(`pairing code: ${c.code()}`));
});

test('a wrong code is 401 wrong_code; no token, or a bad one, is 401 not_paired', async () => {
  const wrong = await c.call('POST', '/v1/pair', { code: 'ZZZZZZ' === c.code() ? 'YYYYYY' : 'ZZZZZZ' }, { token: null });
  assert.equal(wrong.status, 401);
  assert.equal(wrong.json.code, 'wrong_code');
  assert.equal(typeof wrong.json.error, 'string');
  const none = await c.call('GET', '/v1/workspace', undefined, { token: null });
  assert.equal(none.status, 401);
  assert.equal(none.json.code, 'not_paired');
  const bad = await c.call('GET', '/v1/workspace', undefined, { token: 'f'.repeat(64) });
  assert.equal(bad.status, 401);
  assert.equal(bad.json.code, 'not_paired');
  const bearer = await c.call('GET', '/v1/workspace', undefined, { token: null, headers: { authorization: 'Bearer nope' } });
  assert.equal(bearer.status, 401);
});

test('the right code gives a token once; the code changes; both headers carry it', async () => {
  const used = c.code();
  const r = await c.call('POST', '/v1/pair', { code: ` ${used.toLowerCase()} ` }, { token: null });
  assert.equal(r.status, 200, r.text);
  assert.match(r.json.token, /^[0-9a-f]{64}$/);
  assert.match(r.json.id, /^p\d+$/);
  assert.equal(r.json.protocol, 1);
  assert.ok(r.json.expires > Date.now() / 1000 + 60, 'it expires later');
  assert.ok(r.json.ttl > 60);
  await until(() => c.code() !== used, 'a new code');
  const again = await c.call('POST', '/v1/pair', { code: used }, { token: null });
  assert.equal(again.status, 401, 'a code works once');
  assert.equal((await c.call('GET', '/v1/workspace', undefined, { token: r.json.token })).status, 200);
  assert.equal((await c.call('GET', '/v1/workspace', undefined, { token: null, headers: { authorization: `Bearer ${r.json.token}` } })).status, 200);
  const h = await c.call('GET', '/v1/health', undefined, { token: r.json.token });
  assert.equal(h.json.paired, true);
  c.token = r.json.token;
});

test('five wrong codes change the code', async () => {
  const was = c.code();
  for (let i = 0; i < 5; i++) await c.call('POST', '/v1/pair', { code: 'AAAAAA' }, { token: null });
  await until(() => c.code() !== was, 'the code to change after five wrong ones');
  assert.equal((await c.call('POST', '/v1/pair', { code: was }, { token: null })).status, 401);
});

test('pairings list them; revoke ends one; unpair ends this one', async () => {
  const other = await c.pair(false);
  const list = await c.call('GET', '/v1/pairings');
  assert.equal(list.status, 200);
  const ids = list.json.pairings.map((p) => p.id);
  assert.ok(ids.includes(other.id));
  assert.equal(list.json.pairings.filter((p) => p.current).length, 1);
  for (const p of list.json.pairings) for (const k of ['id', 'origin', 'created', 'expires', 'seen', 'current']) assert.ok(k in p, k);
  assert.equal((await c.call('GET', '/v1/workspace', undefined, { token: other.token })).status, 200);
  const rv = await c.call('POST', '/v1/pairings/revoke', { id: other.id });
  assert.equal(rv.status, 200);
  assert.equal((await c.call('GET', '/v1/workspace', undefined, { token: other.token })).status, 401);
  assert.equal((await c.call('POST', '/v1/pairings/revoke', { id: 'p999' })).json.code, 'not_found');
  assert.equal((await c.call('POST', '/v1/unpair')).status, 200);
  assert.equal((await c.call('GET', '/v1/workspace')).status, 401);
});
