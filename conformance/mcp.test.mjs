// Conformance §9–§10: the handshake over MCP, the manifest, argv-only
// inputs, switches, the stream (list_changed, heartbeat, leaving).
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { start, until, FIXTURES } from './harness.mjs';

const CAP = (id, extra = '') => `# ${id}
A capability for the conformance suite: it echoes what it is given.

snapshot: node echo.js snap
interval: 1s

## tool: say
Say the text back, exactly as it came.
command: node echo.js {text} --env
inputs:
- text (string, required): what to say
${extra}`;

let c;
const hello = { runtime: 'conformance', platform: 'node', version: '1', protocol: 'dry-capabilities/1' };
before(async () => {
  c = await start({
    name: 'mcp',
    caps: {
      'echo.md': CAP('echo'),
      'flip.md': '# flip\nA switch that acts: an actuator.\n\n## tool: flip\nFlip it.\ncommand: node echo.js flipped\napproval: yes\n',
      'broken.md': '# broken\nDoes not read.\n\n## tool: x\ncommand: sh -c {code}\ninputs:\n- code (string, required): no\n',
    },
    setup: ({ capsDir }) => fs.copyFileSync(path.join(FIXTURES, 'echo.js'), path.join(capsDir, 'echo.js')),
  });
  await c.pair();
});
after(async () => { await c?.stop(); });

test('initialize: hello both ways; listChanged; the hello is kept, and replaced on re-register', async () => {
  const init = await c.rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'conformance', version: '1' }, _meta: { hello } });
  assert.equal(init.status, 200);
  assert.equal(init.result._meta.hello.protocol, 'dry-capabilities/1');
  assert.equal(typeof init.result._meta.hello.runtime, 'string');
  assert.equal(init.result.capabilities.tools.listChanged, true);
  const p = (await c.call('GET', '/v1/pairings')).json.pairings.find((x) => x.current);
  assert.deepEqual(p.hello, hello);
  await c.rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'conformance', version: '2' }, _meta: { hello: { ...hello, version: '2' } } });
  assert.equal((await c.call('GET', '/v1/pairings')).json.pairings.find((x) => x.current).hello.version, '2', 're-register replaces the hello');
  assert.deepEqual((await c.rpc('ping')).result, {});
});

test('tools/list: each capability\'s tools carry _meta.capability, state and approval; a broken file brings none', async () => {
  const { tools } = (await c.rpc('tools/list')).result;
  const by = (n) => tools.find((t) => t.name === n);
  assert.deepEqual([by('echo.say')._meta.capability, by('echo.say')._meta.state, by('echo.say')._meta.approval], ['echo', 'present', false]);
  assert.equal(by('echo.say').inputSchema.required[0], 'text');
  assert.equal(by('flip.flip')._meta.approval, true, 'an actuator says so');
  assert.equal(by('terminal.run')._meta.capability, 'terminal');
  assert.equal(by('terminal.run')._meta.approval, true);
  assert.equal(by('file.read')._meta.capability, 'file');
  assert.ok(!tools.some((t) => t.name.startsWith('broken.')));
});

test('resources: one per capability; an actuator starts switched off, saying how to switch it on', async () => {
  const { resources } = (await c.rpc('resources/list')).result;
  const r = (id) => resources.find((x) => x._meta.capability === id);
  assert.equal(r('echo').uri, 'dry://capability/echo');
  assert.deepEqual([r('echo')._meta.state, r('echo')._meta.interval], ['present', 1]);
  assert.equal(r('flip')._meta.state, 'absent');
  assert.match(r('flip')._meta.why, /switched off.*on flip/);
  assert.equal(r('terminal')._meta.state, 'absent');
  assert.ok(r('file'));
  const read = await c.rpc('resources/read', { uri: 'dry://capability/echo' });
  assert.equal(read.result.contents[0].text, 'snap');
});

test('a declared tool: inputs are whole arguments, never a shell (an injection stays one argument)', async () => {
  const attack = 'x; touch /tmp/dry-conformance-pwned $(id) `id` && echo no';
  const r = await c.rpc('tools/call', { name: 'echo.say', arguments: { text: attack } });
  assert.equal(r.result.isError, false);
  assert.equal(r.result.content[0].text, `${attack}|${attack}`, 'one argument, and DRY_INPUT_TEXT');
  assert.equal(fs.existsSync('/tmp/dry-conformance-pwned'), false);
  const missing = await c.rpc('tools/call', { name: 'echo.say', arguments: {} });
  assert.equal(missing.result.isError, true);
  const extra = await c.rpc('tools/call', { name: 'echo.say', arguments: { text: 'a', other: 'b' } });
  assert.equal(extra.result.isError, true);
  const nul = await c.rpc('tools/call', { name: 'echo.say', arguments: { text: 'a\u0000b' } });
  assert.equal(nul.result.isError, true);
});

test('a switched-off capability\'s call is refused with capability_off', async () => {
  const r = await c.rpc('tools/call', { name: 'flip.flip', arguments: {} });
  assert.equal(r.result.isError, true);
  assert.equal(r.result._meta.code, 'capability_off');
  const t = await c.rpc('tools/call', { name: 'terminal.run', arguments: { command: 'echo hi' } });
  assert.equal(t.result.isError, true);
  assert.equal(t.result._meta.code, 'capability_off');
});

test('JSON-RPC errors: an unknown tool, an unknown method, malformed requests', async () => {
  assert.equal((await c.rpc('tools/call', { name: 'nope.nothing', arguments: {} })).error.code, -32602);
  assert.equal((await c.rpc('nope/method')).error.code, -32601);
  const bad = await fetch(`${c.base}/mcp`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${c.token}` }, body: '{not json' });
  assert.equal((await bad.json()).error.code, -32700);
  const inv = await fetch(`${c.base}/mcp`, { method: 'POST', headers: { 'content-type': 'application/json', authorization: `Bearer ${c.token}` }, body: JSON.stringify({ id: 1, method: 'ping' }) });
  assert.equal((await inv.json()).error.code, -32600);
  const noauth = await fetch(`${c.base}/mcp`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }) });
  assert.equal(noauth.status, 401);
});

test('the stream: heartbeats, list_changed for a new capability, and it closes when the pairing is revoked', async () => {
  const peer = await c.pair(false);
  const s = c.stream('/mcp', { token: peer.token });
  await until(() => s.status === 200, 'the stream to open');
  await until(() => s.text.split('\n').filter((l) => l.startsWith(': ping')).length >= 2, 'two heartbeats (--heartbeat 1)', 8000);
  fs.writeFileSync(path.join(c.capsDir, 'fresh.md'), CAP('fresh'));
  await until(() => s.text.includes('notifications/tools/list_changed'), 'list_changed after a capability file was added', 15000);
  const { tools } = (await c.rpc('tools/list', {}, { token: peer.token })).result;
  assert.ok(tools.some((t) => t.name === 'fresh.say'), 'listing again registers the new one');
  assert.equal((await c.call('POST', '/v1/pairings/revoke', { id: peer.id })).status, 200);
  await until(() => s.closed, 'the stream to close when its pairing is revoked', 5000);
});
