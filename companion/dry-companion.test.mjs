// node --test companion/   (Node 20+, no dependencies)
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import {
  createCompanion, originAllowed, hostAllowed, secretName, needsConfirm, cloneUrlOk, refOk, storePathOk, parseArgs,
} from './dry-companion.mjs';

let c, base, token, root, outside, store;
const ORIGIN = 'https://kaush4l.github.io';

async function call(method, p, body, headers = {}) {
  const r = await fetch(base + p, {
    method,
    headers: { origin: ORIGIN, ...(token ? { 'x-dry-token': token } : {}), ...(body ? { 'content-type': 'application/json' } : {}), ...headers },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* not json */ }
  return { status: r.status, headers: r.headers, json, text };
}

before(async () => {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dry-companion-')));
  root = path.join(tmp, 'root');
  outside = path.join(tmp, 'outside');
  store = path.join(tmp, 'store');
  fs.mkdirSync(path.join(root, 'app', 'src'), { recursive: true });
  fs.mkdirSync(path.join(root, 'loose'), { recursive: true });
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(root, 'app', 'package.json'), '{"name":"app"}');
  fs.writeFileSync(path.join(root, 'app', 'src', 'main.js'), 'console.log("hi")\n');
  fs.writeFileSync(path.join(root, 'app', '.env'), 'TOKEN=nope\n');
  fs.writeFileSync(path.join(outside, 'secret.txt'), 'outside\n');
  fs.symlinkSync(outside, path.join(root, 'app', 'out-link'));
  fs.symlinkSync(path.join(outside, 'secret.txt'), path.join(root, 'app', 'file-link'));
  execFileSync('git', ['init', '-q', path.join(root, 'repo')]);
  c = createCompanion({ port: 0, roots: [root], folder: root, on: ['terminal'], config: path.join(path.dirname(store), 'companion.json'), quiet: true, store, lmstudio: 'http://127.0.0.1:9', ollama: 'http://127.0.0.1:9' });
  const port = await c.listen();
  base = `http://127.0.0.1:${port}`;
});

after(async () => { await c.close(); });

test('origins: the hosted page, local copies and extras only', () => {
  assert.ok(originAllowed('https://kaush4l.github.io'));
  assert.ok(originAllowed('http://localhost:8765'));
  assert.ok(originAllowed('http://127.0.0.1:3000'));
  assert.ok(originAllowed('http://localhost'));
  assert.ok(!originAllowed('https://evil.example'));
  assert.ok(!originAllowed('https://kaush4l.github.io.evil.example'));
  assert.ok(!originAllowed('http://localhost.evil.example:80'));
  assert.ok(!originAllowed('https://localhost:8765'), 'only the http local copies');
  assert.ok(!originAllowed('null'));
  assert.ok(originAllowed('https://dry.example', ['https://dry.example']));
  assert.ok(hostAllowed('127.0.0.1:7777', 7777) && hostAllowed('localhost:7777', 7777));
  assert.ok(!hostAllowed('evil.example:7777', 7777) && !hostAllowed('127.0.0.1:7778', 7777));
});

test('rules: secrets, confirmations, URLs, names', () => {
  for (const n of ['.env', '.env.local', 'id_rsa', 'server.pem', 'credentials.json', '.ssh', '.netrc']) assert.ok(secretName(n), n);
  for (const n of ['environment.rs', 'main.js', 'keys.md']) assert.ok(!secretName(n), n);
  for (const cmd of ['git push', 'cd x && git push origin main', 'git -C . push', 'git clone https://x/y', 'gh pr create', 'gh repo create a']) assert.ok(needsConfirm(cmd), cmd);
  for (const cmd of ['git status', 'git commit -m "push later"', 'echo pushing', 'cargo test']) assert.ok(!needsConfirm(cmd), cmd);
  assert.ok(cloneUrlOk('https://github.com/a/b.git') && cloneUrlOk('git@github.com:a/b.git') && cloneUrlOk('file:///tmp/x.git'));
  assert.ok(!cloneUrlOk('--upload-pack=evil') && !cloneUrlOk('ext::sh -c x') && !cloneUrlOk('/tmp/x'));
  assert.ok(refOk('main') && refOk('feature/x-1') && !refOk('-f') && !refOk('a..b') && !refOk('x.lock'));
  assert.ok(storePathOk('app.md') && storePathOk('engines/0/convo/messages.md'));
  assert.ok(!storePathOk('../x') && !storePathOk('/etc/passwd') && !storePathOk('a//b') && !storePathOk('a/./b'));
  assert.deepEqual(parseArgs(['--port', '7790', '--root', '/x', '--root', '/y']).roots, ['/x', '/y']);
});

test('preflight: CORS and private network access for allowed origins only', async () => {
  const r = await fetch(base + '/v1/projects', {
    method: 'OPTIONS',
    headers: { origin: ORIGIN, 'access-control-request-method': 'GET', 'access-control-request-headers': 'x-dry-token', 'access-control-request-private-network': 'true' },
  });
  assert.equal(r.status, 204);
  assert.equal(r.headers.get('access-control-allow-origin'), ORIGIN);
  assert.equal(r.headers.get('access-control-allow-private-network'), 'true');
  assert.match(r.headers.get('access-control-allow-headers'), /x-dry-token/);
  assert.match(r.headers.get('access-control-allow-methods'), /POST/);
  const bad = await fetch(base + '/v1/projects', { method: 'OPTIONS', headers: { origin: 'https://evil.example' } });
  assert.equal(bad.status, 403);
  assert.equal(bad.headers.get('access-control-allow-origin'), null);
});

test('a page on another origin or host name is refused', async () => {
  const r = await call('GET', '/v1/health', null, { origin: 'https://evil.example' });
  assert.equal(r.status, 403);
  const http = await import('node:http');
  const status = await new Promise((resolve, reject) => {
    const req = http.request(base + '/v1/health', { headers: { host: `evil.example:${new URL(base).port}` } }, (res) => { res.resume(); resolve(res.statusCode); });
    req.on('error', reject);
    req.end();
  });
  assert.equal(status, 403, 'DNS rebinding: another host name');
});

test('pairing: a wrong code fails, the right one gives a token once', async () => {
  token = null;
  assert.equal((await call('GET', '/v1/projects')).status, 401, 'nothing without a token');
  const health = await call('GET', '/v1/health');
  assert.equal(health.status, 200);
  assert.equal(health.json.paired, false);
  assert.equal((await call('POST', '/v1/pair', { code: 'WRONG1' })).status, 401);
  const code = c.pairCode();
  const ok = await call('POST', '/v1/pair', { code: code.toLowerCase() });
  assert.equal(ok.status, 200);
  assert.match(ok.json.token, /^[0-9a-f]{64}$/);
  assert.equal((await call('POST', '/v1/pair', { code })).status, 401, 'the code works once');
  assert.notEqual(c.pairCode(), code, 'a new code for the next page');
  token = ok.json.token;
  assert.equal((await call('GET', '/v1/health')).json.paired, true);
  const bearer = await fetch(base + '/v1/projects', { headers: { authorization: `Bearer ${token}` } });
  assert.equal(bearer.status, 200, 'the token as a Bearer key too (the web app\'s Transport)');
  assert.equal((await call('GET', '/v1/projects', null, { 'x-dry-token': 'f'.repeat(64) })).status, 401);
});

test('five wrong codes change the code', async () => {
  const before = c.pairCode();
  const saved = token;
  token = null;
  for (let i = 0; i < 5; i++) await call('POST', '/v1/pair', { code: 'NOPE00' });
  assert.notEqual(c.pairCode(), before);
  token = saved;
});

test('projects: folders with .git or a manifest, under the roots', async () => {
  const r = await call('GET', '/v1/projects');
  assert.equal(r.status, 200);
  const names = r.json.projects.map((p) => p.name).sort();
  assert.deepEqual(names, ['app', 'repo']);
  assert.ok(r.json.projects.find((p) => p.name === 'repo').git);
  assert.deepEqual(r.json.projects.find((p) => p.name === 'app').markers, ['package.json']);
});

test('files: confined to the roots; links out and secrets refused', async () => {
  const app = path.join(root, 'app');
  const tree = await call('GET', `/v1/files/tree?path=${encodeURIComponent(app)}`);
  const names = tree.json.entries.map((e) => e.name);
  assert.ok(names.includes('src') && names.includes('package.json'));
  assert.ok(!names.includes('.env'), 'secrets are not listed');
  assert.ok(!names.includes('out-link') && !names.includes('file-link'), 'links out are not listed');
  const read = await call('GET', `/v1/files/read?path=${encodeURIComponent(path.join(app, 'src/main.js'))}`);
  assert.deepEqual([read.json.kind, read.json.text], ['text', 'console.log("hi")\n']);
  const miss = async (p) => (await call('GET', `/v1/files/read?path=${encodeURIComponent(p)}`)).json;
  assert.equal((await miss(path.join(app, '../../outside/secret.txt'))).kind, 'missing', 'a .. escape');
  assert.match((await miss(path.join(app, 'file-link'))).why, /outside/, 'a link out');
  assert.match((await miss(path.join(app, 'out-link/secret.txt'))).why, /outside/, 'through a folder link');
  assert.match((await miss(path.join(app, '.env'))).why, /secrets/);
  assert.equal((await miss('/etc/passwd')).kind, 'missing');
  const list = await call('GET', `/v1/files/list?root=${encodeURIComponent(app)}&max=50`);
  assert.deepEqual(list.json.files, ['package.json', 'src/main.js']);

  const w = await call('POST', '/v1/files/write', { path: path.join(app, 'src/new/b.txt'), text: 'b' });
  assert.equal(w.status, 200);
  assert.equal(fs.readFileSync(path.join(app, 'src/new/b.txt'), 'utf8'), 'b');
  assert.equal((await call('POST', '/v1/files/write', { path: path.join(outside, 'x.txt'), text: 'x' })).status, 403);
  assert.equal((await call('POST', '/v1/files/write', { path: path.join(app, 'out-link/x.txt'), text: 'x' })).status, 403);
  assert.equal((await call('POST', '/v1/files/write', { path: path.join(app, '.env.local'), text: 'x' })).status, 403);
  assert.equal((await call('POST', '/v1/files/write', { path: path.join(root, 'repo/.git/config'), text: 'x' })).status, 403);
  assert.equal((await call('POST', '/v1/files/write', { path: path.join(app, 'src/x'), text: 'x', within: path.join(app, 'src/new') })).status, 403);
  assert.ok(!fs.existsSync(path.join(outside, 'x.txt')));
});

test('exec: run waits, a timeout kills, cwd must be in a project', async () => {
  const app = path.join(root, 'app');
  const r = await call('POST', '/v1/exec/run', { cwd: app, command: 'echo hello; echo err >&2; exit 3' });
  assert.equal(r.status, 200);
  assert.equal(r.json.exit, 3);
  assert.match(r.json.output, /hello/);
  assert.match(r.json.output, /err/);
  const t0 = Date.now();
  const slow = await call('POST', '/v1/exec/run', { cwd: app, command: 'sleep 20', timeout_ms: 300 });
  assert.equal(slow.json.timed_out, true);
  assert.equal(slow.json.exit, null);
  assert.ok(Date.now() - t0 < 5000, 'killed at the timeout');
  assert.equal((await call('POST', '/v1/exec/run', { cwd: path.join(root, 'loose'), command: 'true' })).status, 403, 'not a project');
  assert.equal((await call('POST', '/v1/exec/run', { cwd: outside, command: 'true' })).status, 403, 'outside the roots');
  assert.equal((await call('POST', '/v1/exec/run', { cwd: app, command: 'git push origin main' })).status, 428, 'push needs a click');
  const env = await call('POST', '/v1/exec/run', { cwd: app, command: 'env' });
  assert.ok(!/DRY_TEST_SECRET_TOKEN/.test(env.json.output), 'secret env left out');
});

test('exec: start calls back with an exit event; sessions by key', async () => {
  const app = path.join(root, 'app');
  const ctl = new AbortController();
  const stream = await fetch(base + '/v1/events', { headers: { origin: ORIGIN, 'x-dry-token': token }, signal: ctl.signal });
  assert.equal(stream.headers.get('content-type'), 'text/event-stream');
  const reader = stream.body.getReader();
  const started = await call('POST', '/v1/exec/start', { cwd: app, command: 'sleep 0.2; echo done', key: 'e1/term/2' });
  assert.equal(started.json.status, 'running');
  let seen = '';
  while (!/event: exit\ndata: .*"key":"e1\/term\/2"/.test(seen)) {
    const { value, done } = await reader.read();
    if (done) break;
    seen += new TextDecoder().decode(value);
  }
  ctl.abort();
  assert.match(seen, /"exit":0/);
  const s = await call('GET', '/v1/exec/session?key=e1%2Fterm%2F2&tail=5');
  assert.equal(s.json.status, 'exited');
  assert.equal(s.json.tail, 'done');
  const bg = await call('POST', '/v1/exec/start', { cwd: app, command: 'sleep 30', mode: 'background' });
  const stop = await call('POST', '/v1/exec/stop', { id: bg.json.id });
  assert.equal(stop.status, 200);
  for (let i = 0; i < 50; i++) {
    if ((await call('GET', `/v1/exec/session?id=${bg.json.id}`)).json.status === 'exited') break;
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.equal((await call('GET', `/v1/exec/session?id=${bg.json.id}`)).json.status, 'exited');
});

test('git: commit and push to a local bare remote; push needs confirmation', async () => {
  const repo = path.join(root, 'repo');
  const bare = path.join(outside, 'remote.git');
  execFileSync('git', ['init', '-q', '--bare', bare]);
  execFileSync('git', ['-C', repo, 'remote', 'add', 'origin', bare]);
  execFileSync('git', ['-C', repo, 'config', 'user.email', 't@example.com']);
  execFileSync('git', ['-C', repo, 'config', 'user.name', 'T']);
  fs.writeFileSync(path.join(repo, 'a.txt'), 'a\n');
  const st = await call('POST', '/v1/git/status', { path: repo });
  assert.deepEqual(st.json.files.map((f) => f.path), ['a.txt']);
  const cm = await call('POST', '/v1/git/commit', { path: repo, message: 'first' });
  assert.ok(cm.json.ok, cm.json.stderr);
  assert.equal((await call('POST', '/v1/git/push', { path: repo })).status, 428);
  const push = await call('POST', '/v1/git/push', { path: repo, confirmed: true });
  assert.ok(push.json.ok, push.json.stderr);
  const branch = execFileSync('git', ['-C', repo, 'branch', '--show-current']).toString().trim();
  assert.equal(execFileSync('git', ['-C', bare, 'log', '--format=%s', branch]).toString().trim(), 'first');
  assert.equal((await call('POST', '/v1/git/branch', { path: repo, name: '--force' })).status, 400);
  assert.equal((await call('POST', '/v1/git/clone', { url: `file://${bare}`, root, name: 'copy' })).status, 428);
  const cl = await call('POST', '/v1/git/clone', { url: `file://${bare}`, root, name: 'copy', confirmed: true });
  assert.ok(cl.json.ok, cl.json.stderr);
  assert.ok(fs.existsSync(path.join(root, 'copy', 'a.txt')));
});

test('store: texts by path, confined to its folder', async () => {
  assert.equal((await call('POST', '/v1/store', { put: { 'app.md': 'mode: dark', 'engines/0/spec/goal.md': 'g' } })).status, 200);
  assert.deepEqual((await call('GET', '/v1/store')).json.texts, { 'app.md': 'mode: dark', 'engines/0/spec/goal.md': 'g' });
  assert.equal((await call('POST', '/v1/store', { remove: ['app.md'] })).status, 200);
  assert.deepEqual(Object.keys((await call('GET', '/v1/store')).json.texts), ['engines/0/spec/goal.md']);
  assert.equal((await call('POST', '/v1/store', { put: { '../escape.md': 'x' } })).status, 400);
});

test('inference: models listed; a server that is down says so', async () => {
  const m = await call('GET', '/v1/models');
  assert.equal(m.status, 200);
  assert.ok(Array.isArray(m.json.data));
  const r = await call('POST', '/v1/chat/completions', { model: 'lmstudio/x', messages: [{ role: 'user', content: 'hi' }] });
  assert.equal(r.status, 503, 'a declared provider that is down');
  assert.equal(r.json.code, 'provider_down');
  assert.match(r.json.error, /lmstudio is failed/);
});

test('unpair ends the token', async () => {
  assert.equal((await call('POST', '/v1/unpair')).status, 200);
  assert.equal((await call('GET', '/v1/projects')).status, 401);
});
