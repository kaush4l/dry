// Conformance §6–§7: the folder's file primitives, path refusals, secrets,
// .dry, the snapshot and the store.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { start } from './harness.mjs';

let c;
const big = 'x'.repeat(1_100_000);
before(async () => {
  c = await start({
    name: 'workspace',
    files: {
      'a.txt': 'alpha\n', 'big.txt': big, 'bin.dat': Buffer.from([0, 1, 2, 3, 255]), 'src/b.rs': 'fn main() {}\n', 'src/deep/c.md': '# c\n',
      '.env': 'TOKEN=1\n', 'src/.env.local': 'X=1\n', 'id_rsa': 'key', 'deploy.pem': 'pem', '.aws/config': 'x', '.git/HEAD': 'ref: refs/heads/main\n', 'node_modules/m/i.js': '1',
    },
    setup: ({ tmp, ws }) => {
      fs.writeFileSync(path.join(tmp, 'outside.txt'), 'outside');
      fs.symlinkSync(path.join(tmp, 'outside.txt'), path.join(ws, 'link-out'));
      fs.symlinkSync(tmp, path.join(ws, 'dir-out'));
      fs.symlinkSync(path.join(ws, 'a.txt'), path.join(ws, 'link-in'));
    },
  });
  await c.pair();
});
after(async () => { await c?.stop(); });

const err = async (r, status, code) => {
  const v = await r;
  assert.equal(v.status, status, v.text);
  assert.equal(v.json?.code, code, v.text);
  assert.equal(typeof v.json.error, 'string');
};

test('the workspace: its folder, name, protocol', async () => {
  const r = await c.call('GET', '/v1/workspace');
  assert.equal(r.status, 200);
  assert.equal(r.json.protocol, 1);
  assert.equal(fs.realpathSync(r.json.folder), fs.realpathSync(c.ws));
  assert.equal(r.json.name, 'ws');
  assert.equal(r.json.snapshot, null);
  assert.equal(typeof r.json.switches.file, 'boolean');
});

test('list: own files first by name, then folders breadth first; never .git, .dry, build folders, secrets, links out', async () => {
  const r = await c.call('GET', '/v1/fs/list');
  assert.equal(r.status, 200, r.text);
  assert.deepEqual(r.json.files, ['a.txt', 'big.txt', 'bin.dat', 'link-in', 'src/b.rs', 'src/deep/c.md']);
  assert.equal(r.json.truncated, false);
  const sub = await c.call('GET', '/v1/fs/list?path=src');
  assert.deepEqual(sub.json.files, ['b.rs', 'deep/c.md']);
  const cut = await c.call('GET', '/v1/fs/list?max=2');
  assert.deepEqual(cut.json.files, ['a.txt', 'big.txt']);
  assert.equal(cut.json.truncated, true);
  await err(c.call('GET', '/v1/fs/list?path=..'), 403, 'path_outside');
  await err(c.call('GET', '/v1/fs/list?path=dir-out'), 403, 'path_outside');
});

test('read: text, binary, too large, missing, a folder', async () => {
  const a = await c.call('GET', '/v1/fs/read?path=a.txt');
  assert.deepEqual([a.status, a.json.kind, a.json.text, a.json.bytes], [200, 'text', 'alpha\n', 6]);
  const sub = await c.call('GET', '/v1/fs/read?path=src/./deep/../b.rs');
  assert.equal(sub.json.text, 'fn main() {}\n');
  assert.equal(sub.json.path, 'src/b.rs');
  const bin = await c.call('GET', '/v1/fs/read?path=bin.dat');
  assert.deepEqual([bin.json.kind, bin.json.bytes], ['binary', 5]);
  const large = await c.call('GET', '/v1/fs/read?path=big.txt');
  assert.deepEqual([large.json.kind, large.json.bytes], ['too_large', big.length]);
  const inside = await c.call('GET', '/v1/fs/read?path=link-in');
  assert.equal(inside.json.text, 'alpha\n', 'a link that stays inside reads');
  await err(c.call('GET', '/v1/fs/read?path=nope.txt'), 404, 'not_found');
  await err(c.call('GET', '/v1/fs/read?path=src'), 400, 'not_a_file');
});

test('paths that leave the folder are path_outside: .., absolute, links out', async () => {
  for (const p of ['../outside.txt', 'src/../../outside.txt', '/etc/passwd', 'link-out', 'dir-out/outside.txt', 'C:\\Windows\\win.ini']) {
    await err(c.call('GET', `/v1/fs/read?path=${encodeURIComponent(p)}`), 403, 'path_outside');
  }
  await err(c.call('POST', '/v1/fs/write', { path: '../escape.txt', text: 'x' }), 403, 'path_outside');
  await err(c.call('POST', '/v1/fs/write', { path: 'link-out', text: 'x' }), 403, 'path_outside');
  await err(c.call('POST', '/v1/fs/write', { path: 'dir-out/new.txt', text: 'x' }), 403, 'path_outside');
  assert.equal(fs.readFileSync(path.join(c.tmp, 'outside.txt'), 'utf8'), 'outside', 'nothing outside was written');
  assert.equal(fs.existsSync(path.join(c.tmp, 'escape.txt')), false);
  assert.equal(fs.existsSync(path.join(c.tmp, 'new.txt')), false);
});

test('secret-looking files are never read or written: secret_path', async () => {
  for (const p of ['.env', 'src/.env.local', 'id_rsa', 'deploy.pem', '.aws/config', 'new/.npmrc']) {
    await err(c.call('GET', `/v1/fs/read?path=${encodeURIComponent(p)}`), 403, 'secret_path');
  }
  await err(c.call('POST', '/v1/fs/write', { path: '.env', text: 'x' }), 403, 'secret_path');
  assert.equal(fs.readFileSync(path.join(c.ws, '.env'), 'utf8'), 'TOKEN=1\n');
});

test('.dry is dry\'s own state; .git is written only by git: dry_state', async () => {
  await err(c.call('GET', '/v1/fs/read?path=.dry/companion.json'), 403, 'dry_state');
  await err(c.call('POST', '/v1/fs/write', { path: '.dry/state/x.md', text: 'x' }), 403, 'dry_state');
  await err(c.call('POST', '/v1/fs/write', { path: '.git/config', text: 'x' }), 403, 'dry_state');
});

test('write makes folders, replaces whole files; stat says file, dir or missing', async () => {
  const w = await c.call('POST', '/v1/fs/write', { path: 'notes/today/n.md', text: 'hello ✓\n' });
  assert.equal(w.status, 200, w.text);
  assert.deepEqual([w.json.ok, w.json.path, w.json.bytes], [true, 'notes/today/n.md', Buffer.byteLength('hello ✓\n')]);
  assert.equal(fs.readFileSync(path.join(c.ws, 'notes/today/n.md'), 'utf8'), 'hello ✓\n');
  assert.equal((await c.call('GET', '/v1/fs/read?path=notes/today/n.md')).json.text, 'hello ✓\n');
  await c.call('POST', '/v1/fs/write', { path: 'notes/today/n.md', text: 'two' });
  assert.equal(fs.readFileSync(path.join(c.ws, 'notes/today/n.md'), 'utf8'), 'two');
  await err(c.call('POST', '/v1/fs/write', { path: 'n.md' }), 400, 'bad_request');
  const f = await c.call('GET', '/v1/fs/stat?path=a.txt');
  assert.deepEqual([f.json.kind, f.json.size], ['file', 6]);
  assert.ok(f.json.modified > 1_600_000_000);
  assert.equal((await c.call('GET', '/v1/fs/stat?path=src')).json.kind, 'dir');
  assert.equal((await c.call('GET', '/v1/fs/stat?path=nope')).json.kind, 'missing');
  await err(c.call('GET', '/v1/fs/stat?path=../x'), 403, 'path_outside');
  await err(c.call('GET', '/v1/fs/stat?path=.env'), 403, 'secret_path');
});

test('the snapshot round trip: checked, kept as given, in .dry/snapshot.json', async () => {
  await err(c.call('POST', '/v1/workspace/snapshot', { snapshot: { version: 2 } }), 400, 'bad_snapshot');
  await err(c.call('POST', '/v1/workspace/snapshot', { snapshot: [1] }), 400, 'bad_snapshot');
  const snap = { version: 1, saved: 1795400000, focus: 'e1/files:a.txt', engines: [{ engine: 'e1', name: 'dev', artifacts: [{ name: 'files', kind: 'file', root: '.', open: [{ path: 'a.txt', focus: [1, 2] }], edits: [], text: 'kind: file\nroot: .\nopen: a.txt 1-2\n' }] }] };
  const w = await c.call('POST', '/v1/workspace/snapshot', { snapshot: snap });
  assert.equal(w.status, 200, w.text);
  assert.equal(w.json.path, '.dry/snapshot.json');
  assert.deepEqual((await c.call('GET', '/v1/workspace')).json.snapshot, snap);
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(c.ws, '.dry', 'snapshot.json'), 'utf8')), snap);
  assert.ok(!(await c.call('GET', '/v1/fs/list')).json.files.some((f) => f.startsWith('.dry')), '.dry is never listed');
});

test('the store: texts by path in .dry/state, confined', async () => {
  const put = await c.call('POST', '/v1/store', { put: { 'app.md': 'mode: dark', 'engines/e1/spec/goal.md': 'g' } });
  assert.equal(put.status, 200, put.text);
  assert.deepEqual((await c.call('GET', '/v1/store')).json.texts, { 'app.md': 'mode: dark', 'engines/e1/spec/goal.md': 'g' });
  assert.equal(fs.readFileSync(path.join(c.ws, '.dry', 'state', 'engines', 'e1', 'spec', 'goal.md'), 'utf8'), 'g');
  await c.call('POST', '/v1/store', { remove: ['app.md'] });
  assert.deepEqual(Object.keys((await c.call('GET', '/v1/store')).json.texts), ['engines/e1/spec/goal.md']);
  for (const bad of ['../x.md', '/abs.md', 'a/../../b.md', 'a//b.md']) {
    assert.equal((await c.call('POST', '/v1/store', { put: { [bad]: 'x' } })).status, 400, bad);
  }
});
