// Conformance §6 (terminal), §10 (an actuator starts off), §11 (stop all).
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { start, until, sleep } from './harness.mjs';

const unix = process.platform !== 'win32';
let off, c;
before(async () => {
  off = await start({ name: 'term-off' });
  await off.pair();
  c = await start({ name: 'term', on: ['terminal'], files: { 'sub/x.txt': 'x' }, env: { DRY_CONF_API_TOKEN: 'hidden-value', DRY_CONF_PLAIN: 'shown-value' } });
  await c.pair();
});
after(async () => { await off?.stop(); await c?.stop(); });

test('the terminal is an actuator: off until switched on (403 capability_off)', async () => {
  for (const [m, p, b] of [['POST', '/v1/term/run', { key: 'k/1', command: 'echo hi' }], ['POST', '/v1/term/start', { key: 'k/2', command: 'echo hi' }], ['GET', '/v1/term/tail?key=k/1'], ['POST', '/v1/term/stop', { key: 'k/1' }]]) {
    const r = await off.call(m, p, b);
    assert.equal(r.status, 403, p);
    assert.equal(r.json.code, 'capability_off', p);
    assert.match(r.json.error, /on terminal/);
  }
  assert.equal((await off.call('GET', '/v1/workspace')).json.switches.terminal, false);
  assert.equal((await c.call('GET', '/v1/workspace')).json.switches.terminal, true);
});

test('run waits: exit code, output, its folder', { skip: !unix }, async () => {
  const r = await c.call('POST', '/v1/term/run', { key: 'e1/term/1', command: 'echo hello; echo oops 1>&2; exit 3' });
  assert.equal(r.status, 200, r.text);
  assert.deepEqual([r.json.key, r.json.mode, r.json.status, r.json.exit], ['e1/term/1', 'run', 'exited', 3]);
  assert.match(r.json.output, /hello/);
  assert.match(r.json.output, /oops/);
  const pwd = await c.call('POST', '/v1/term/run', { key: 'e1/term/2', command: 'pwd', cwd: 'sub' });
  assert.match(pwd.json.output.trim(), /\/ws\/sub$/);
  const t = await c.call('GET', '/v1/term/tail?key=e1/term/1&lines=1');
  assert.deepEqual([t.json.status, t.json.exit], ['exited', 3]);
  assert.equal(t.json.tail.trim().split('\n').length, 1);
});

test('the environment keeps secrets out', { skip: !unix }, async () => {
  const r = await c.call('POST', '/v1/term/run', { key: 'e1/term/env', command: 'env' });
  assert.match(r.json.output, /DRY_CONF_PLAIN=shown-value/);
  assert.doesNotMatch(r.json.output, /hidden-value/);
});

test('refused: a cwd outside or not a folder, a bad key, no command, a publishing command', async () => {
  const bad = async (body, status, code) => { const r = await c.call('POST', '/v1/term/run', body); assert.equal(r.status, status, r.text); assert.equal(r.json.code, code); };
  await bad({ key: 'k/1', command: 'ls', cwd: '..' }, 403, 'path_outside');
  await bad({ key: 'k/1', command: 'ls', cwd: '/' }, 403, 'path_outside');
  await bad({ key: 'k/1', command: 'ls', cwd: 'sub/x.txt' }, 400, 'not_a_folder');
  await bad({ key: 'has space', command: 'ls' }, 400, 'bad_key');
  await bad({ command: 'ls' }, 400, 'bad_key');
  await bad({ key: 'k/1', command: '  ' }, 400, 'bad_request');
  await bad({ key: 'k/1', command: 'git push origin main' }, 428, 'needs_confirm');
});

test('start returns at once; its end is an exit event; a stream resumes from an id', { skip: !unix }, async () => {
  const s = c.stream('/v1/events');
  await until(() => s.text.includes('event: hello'), 'the hello event');
  const r = await c.call('POST', '/v1/term/start', { key: 'e1/term/3', command: 'sleep 0.4; echo done' });
  assert.equal(r.status, 200, r.text);
  assert.deepEqual([r.json.status, r.json.mode], ['running', 'start']);
  assert.equal((await c.call('GET', '/v1/term/tail?key=e1/term/3')).json.status, 'running');
  await until(() => /event: exit\ndata: .*"key":"e1\/term\/3"/.test(s.text), 'the exit event of e1/term/3');
  const ids = [...s.text.matchAll(/^id: (\d+)$/gm)].map((m) => Number(m[1]));
  assert.ok(ids.length >= 1, 'events carry ids');
  s.stop();
  const t = await c.call('GET', '/v1/term/tail?key=e1/term/3');
  assert.deepEqual([t.json.status, t.json.exit], ['exited', 0]);
  assert.match(t.json.tail, /done/);
  // Resume: what came after an earlier id comes first.
  const again = c.stream(`/v1/events?since=${ids[0] - 1}`);
  await until(() => /"key":"e1\/term\/3"/.test(again.text), 'the replayed exit');
  again.stop();
  // A background session: no callback to every stream.
  const bg = await c.call('POST', '/v1/term/start', { key: 'e1/term/bg', command: 'echo bg', callback: false });
  assert.equal(bg.json.mode, 'background');
});

test('tail of a key it does not know is unknown (a restarted companion)', async () => {
  const t = await c.call('GET', '/v1/term/tail?key=e9/term/77');
  assert.equal(t.status, 200);
  assert.deepEqual([t.json.status, t.json.exit, t.json.tail], ['unknown', null, '']);
  assert.equal((await c.call('POST', '/v1/term/stop', { key: 'e9/term/77' })).json.code, 'not_found');
});

test('stop ends one; stop-all ends every running command', { skip: !unix }, async () => {
  await c.call('POST', '/v1/term/start', { key: 'e1/term/long', command: 'sleep 30' });
  const st = await c.call('POST', '/v1/term/stop', { key: 'e1/term/long' });
  assert.equal(st.status, 200);
  await until(async () => (await c.call('GET', '/v1/term/tail?key=e1/term/long')).json.status === 'exited', 'the stopped session');
  assert.equal((await c.call('GET', '/v1/term/tail?key=e1/term/long')).json.exit, null, 'stopped: no exit code');
  await c.call('POST', '/v1/term/start', { key: 'e1/term/a', command: 'sleep 30' });
  await c.call('POST', '/v1/term/start', { key: 'e1/term/b', command: 'sleep 30', callback: false });
  await sleep(100);
  const all = await c.call('POST', '/v1/stop-all');
  assert.equal(all.status, 200, all.text);
  assert.ok(all.json.stopped >= 2, all.text);
  assert.equal(typeof all.json.browser, 'boolean');
  for (const k of ['e1/term/a', 'e1/term/b']) await until(async () => (await c.call('GET', `/v1/term/tail?key=${k}`)).json.status === 'exited', `${k} stopped`);
});
