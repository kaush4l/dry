// node --test companion/   The app served, pairings (expiry, revoke), the event
// log and resume, custom tools (and injection attempts), MCP, the browser
// (CDP; skipped when no Chrome is found), stop-all.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { createCompanion } from './dry-companion.mjs';

let c, base, token, root, app, tmp;
const ORIGIN = 'http://localhost:8765';

async function call(method, p, body, headers = {}, tok = token) {
  const r = await fetch(base + p, {
    method,
    headers: { origin: ORIGIN, ...(tok ? { 'x-dry-token': tok } : {}), ...(body !== undefined ? { 'content-type': 'application/json' } : {}), ...headers },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* not json */ }
  return { status: r.status, headers: r.headers, json, text };
}

async function pair() {
  const r = await call('POST', '/v1/pair', { code: c.pairCode() }, {}, null);
  assert.equal(r.status, 200);
  return r.json;
}

before(async () => {
  tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dry-companion-more-')));
  root = path.join(tmp, 'root');
  app = path.join(tmp, 'dist');
  const proj = path.join(root, 'web');
  fs.mkdirSync(path.join(proj, 'tools'), { recursive: true });
  fs.writeFileSync(path.join(proj, 'package.json'), '{"name":"web"}');
  // A tool that prints its argv and its env input, one per line.
  fs.writeFileSync(path.join(proj, 'echo.js'), 'console.log(JSON.stringify({argv: process.argv.slice(2), env: process.env.DRY_INPUT_WHO ?? null, secret: process.env.DRY_SECRET_TOKEN ?? null}))');
  fs.writeFileSync(path.join(proj, 'tools', 'greet.json'), JSON.stringify({
    name: 'greet', description: 'Say hello.', command: ['node', 'echo.js', '--who', '{who}', '{times}'],
    inputs: [{ name: 'who', type: 'string', required: true, description: 'whom' }, { name: 'times', type: 'integer' }],
  }));
  fs.writeFileSync(path.join(proj, 'tools', 'slow.md'), '# slow\nSleeps.\n\ncommand: node -e setTimeout(()=>{},5000)\ntimeout: 300ms\n');
  fs.writeFileSync(path.join(proj, 'tools', 'evil.json'), JSON.stringify({ name: 'evil', command: ['sh', '-c', '{x}'], inputs: [{ name: 'x' }] }));
  fs.writeFileSync(path.join(proj, 'tools', 'inside.json'), JSON.stringify({ name: 'inside', command: ['echo', 'hi {x}'], inputs: [{ name: 'x' }] }));
  fs.writeFileSync(path.join(proj, 'tools', 'md.md'), '# hello\nSays hello to someone.\n\ncommand: node echo.js {who}\ninputs:\n- who (string, required): who to greet\n');
  fs.mkdirSync(path.join(app, 'app'), { recursive: true });
  fs.writeFileSync(path.join(app, 'index.html'), '<!doctype html><title>landing</title>');
  fs.writeFileSync(path.join(app, 'app', 'index.html'), '<!doctype html><title>Todo</title><h1>Todo</h1><button onclick="document.title=\'clicked\';console.error(\'boom\')">Add</button><input placeholder="new item">');
  fs.writeFileSync(path.join(app, 'app', 'x.wasm'), Buffer.from([0, 97, 115, 109]));
  fs.writeFileSync(path.join(tmp, 'secret.txt'), 'outside');
  process.env.DRY_SECRET_TOKEN = 'do-not-leak';
  c = createCompanion({ port: 0, roots: [root], folder: root, on: ['terminal'], config: path.join(tmp, 'companion.json'), quiet: true, store: path.join(tmp, 'store'), app, ttlMs: 60 * 60 * 1000, lmstudio: 'http://127.0.0.1:9', ollama: 'http://127.0.0.1:9' });
  const port = await c.listen();
  base = `http://127.0.0.1:${port}`;
  token = (await pair()).token;
});

after(async () => { await c.close(); });

test('the app is served from the companion, with the right types, confined to its folder', async () => {
  const landing = await call('GET', '/', undefined, { origin: '' }, null);
  assert.equal(landing.status, 200);
  assert.match(landing.headers.get('content-type'), /text\/html/);
  const page = await call('GET', '/app/', undefined, {}, null);
  assert.match(page.text, /<title>Todo/);
  assert.equal(page.headers.get('cross-origin-opener-policy'), 'same-origin');
  const wasm = await call('GET', '/app/x.wasm', undefined, {}, null);
  assert.equal(wasm.headers.get('content-type'), 'application/wasm');
  const redirect = await fetch(base + '/app', { redirect: 'manual' });
  assert.equal(redirect.status, 301);
  assert.equal((await call('GET', '/app/..%2f..%2fsecret.txt', undefined, {}, null)).status, 404, 'no way out of the app folder');
  assert.equal((await call('GET', '/v1/projects', undefined, {}, null)).status, 401, 'the API still needs a token');
});

test('pairings: listed, revoked, and they expire', async () => {
  const other = await pair();
  assert.match(other.id, /^p\d+$/);
  assert.ok(other.expires > Date.now() / 1000 + 3000);
  const list = await call('GET', '/v1/pairings');
  assert.ok(list.json.pairings.some((p) => p.id === other.id && !p.current));
  assert.ok(list.json.pairings.some((p) => p.current));
  assert.equal((await call('GET', '/v1/projects', undefined, {}, other.token)).status, 200);
  assert.equal((await call('POST', '/v1/pairings/revoke', { id: other.id })).status, 200);
  assert.equal((await call('GET', '/v1/projects', undefined, {}, other.token)).status, 401, 'revoked');
  assert.match(c.command('pairings'), /p\d+/);

  const short = createCompanion({ port: 0, roots: [root], folder: root, config: path.join(tmp, 'companion.json'), quiet: true, ttlMs: 150, app: false });
  const port = await short.listen();
  const pr = await fetch(`http://127.0.0.1:${port}/v1/pair`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code: short.pairCode() }) }).then((r) => r.json());
  const ask = () => fetch(`http://127.0.0.1:${port}/v1/projects`, { headers: { 'x-dry-token': pr.token } }).then((r) => r.status);
  assert.equal(await ask(), 200);
  await new Promise((r) => setTimeout(r, 250));
  assert.equal(await ask(), 401, 'expired');
  await short.close();
});

test('the event log: a page resumes from its last id and gets the exit it missed', async () => {
  const proj = path.join(root, 'web');
  const first = await call('GET', '/v1/events/log');
  const last = first.json.last;
  const s = await call('POST', '/v1/exec/start', { cwd: proj, command: 'echo built', key: 'e1/terminal/7' });
  for (let i = 0; i < 50; i++) {
    if ((await call('GET', `/v1/exec/session?id=${s.json.id}`)).json.status === 'exited') break;
    await new Promise((r) => setTimeout(r, 50));
  }
  const log = await call('GET', `/v1/events/log?since=${last}`);
  const exit = log.json.events.find((e) => e.event === 'exit' && e.data.key === 'e1/terminal/7');
  assert.ok(exit, 'the exit is in the log');
  // A page that was away opens the stream from its last id: the exit comes first.
  const ctl = new AbortController();
  const r = await fetch(`${base}/v1/events?since=${last}`, { headers: { 'x-dry-token': token }, signal: ctl.signal });
  const reader = r.body.getReader();
  let seen = '';
  while (!seen.includes('"key":"e1/terminal/7"')) { const { value, done } = await reader.read(); if (done) break; seen += new TextDecoder().decode(value); }
  ctl.abort();
  assert.match(seen, new RegExp(`id: ${exit.id}\\nevent: exit`));
  // From the exit's own id: nothing old again.
  const none = await call('GET', `/v1/events/log?since=${exit.id}`);
  assert.ok(!none.json.events.some((e) => e.data.key === 'e1/terminal/7'));
});

test('custom tools: listed in capabilities; bad definitions refused; inputs are whole arguments', async () => {
  const caps = await call('GET', '/v1/capabilities');
  const names = caps.json.tools.map((t) => t.name).sort();
  assert.deepEqual(names, ['web.greet', 'web.hello', 'web.slow']);
  const errs = caps.json.tool_errors.map((e) => e.error).join(' | ');
  assert.match(errs, /cannot follow -c/, 'sh -c {x} refused');
  assert.match(errs, /whole argument/, '"hi {x}" refused');
  const greet = caps.json.tools.find((t) => t.name === 'web.greet');
  assert.deepEqual(greet.inputSchema.required, ['who']);
  const hello = caps.json.tools.find((t) => t.name === 'web.hello');
  assert.equal(hello.description, 'Says hello to someone.');

  const run = async (args) => (await call('POST', '/v1/tools/call', { name: 'web.greet', arguments: args }));
  const ok = await run({ who: 'Ada', times: 2 });
  assert.equal(ok.json.ok, true);
  assert.deepEqual(JSON.parse(ok.json.output), { argv: ['--who', 'Ada', '2'], env: 'Ada', secret: null });
  for (const evil of ['Ada; touch /tmp/dry-pwned', '$(touch /tmp/dry-pwned)', '`touch /tmp/dry-pwned`', 'a\nb', '--exec=x', '"; exit 1; "']) {
    const r = await run({ who: evil });
    assert.equal(r.json.ok, true, evil);
    assert.deepEqual(JSON.parse(r.json.output).argv, ['--who', evil], `${evil} stays one argument`);
  }
  assert.ok(!fs.existsSync('/tmp/dry-pwned'));
  assert.equal((await run({ who: 'a', times: '2; rm' })).status, 400, 'an integer is an integer');
  assert.equal((await run({ who: 'a\0b' })).status, 400, 'no zero bytes');
  assert.equal((await run({ who: 'a', extra: 'x' })).status, 400, 'no unknown inputs');
  assert.equal((await run({})).status, 400, 'required');
  assert.equal((await run({ who: { $gt: '' } })).status, 400, 'text is text');
  assert.equal((await call('POST', '/v1/tools/call', { name: 'web.evil', arguments: { x: 'id' } })).status, 404, 'a refused tool cannot be called');
  const slow = await call('POST', '/v1/tools/call', { name: 'web.slow', arguments: {} });
  assert.equal(slow.json.timed_out, true);
  assert.equal((await call('POST', '/v1/tools/call', { name: 'web.hello', arguments: { who: 'Bo' } })).json.ok, true);
});

async function mcp(body, headers = {}, tok = token) {
  return call('POST', '/mcp', body, { accept: 'application/json, text/event-stream', authorization: `Bearer ${tok}`, ...headers }, null);
}

test('MCP: the same tools over streamable HTTP, legacy and modern requests, the token as a bearer', async () => {
  assert.equal((await mcp({ jsonrpc: '2.0', id: 1, method: 'tools/list' }, {}, 'nope')).status, 401);
  const init = await mcp({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 't', version: '1' } } });
  assert.equal(init.json.result.protocolVersion, '2025-06-18');
  assert.equal((await mcp({ jsonrpc: '2.0', method: 'notifications/initialized' })).status, 202);
  const list = await mcp({ jsonrpc: '2.0', id: 2, method: 'tools/list' }, { 'mcp-protocol-version': '2025-06-18' });
  const names = list.json.result.tools.map((t) => t.name);
  assert.ok(names.includes('web.greet') && names.includes('browser_open'));
  const called = await mcp({ jsonrpc: '2.0', id: 3, method: 'tools/call', params: { name: 'web.greet', arguments: { who: 'MCP' } } });
  assert.equal(called.json.result.isError, false);
  assert.match(called.json.result.content[0].text, /"MCP"/);
  // Modern (2026-07-28): per-request _meta, mirrored in headers.
  const meta = { 'io.modelcontextprotocol/protocolVersion': '2026-07-28', 'io.modelcontextprotocol/clientInfo': { name: 't', version: '1' }, 'io.modelcontextprotocol/clientCapabilities': {} };
  const h = { 'mcp-protocol-version': '2026-07-28', 'mcp-method': 'tools/call', 'mcp-name': 'web.greet' };
  const modern = await mcp({ jsonrpc: '2.0', id: 4, method: 'tools/call', params: { name: 'web.greet', arguments: { who: 'M' }, _meta: meta } }, h);
  assert.equal(modern.status, 200);
  assert.equal(modern.json.result.isError, false);
  const mismatch = await mcp({ jsonrpc: '2.0', id: 5, method: 'tools/call', params: { name: 'web.greet', arguments: { who: 'M' }, _meta: meta } }, { ...h, 'mcp-name': 'web.hello' });
  assert.equal(mismatch.status, 400);
  assert.equal(mismatch.json.error.code, -32020);
  const old = await mcp({ jsonrpc: '2.0', id: 6, method: 'tools/list', params: { _meta: { ...meta, 'io.modelcontextprotocol/protocolVersion': '1900-01-01' } } }, { 'mcp-protocol-version': '1900-01-01', 'mcp-method': 'tools/list' });
  assert.equal(old.json.error.code, -32022);
  assert.ok(old.json.error.data.supported.includes('2026-07-28'));
  const discover = await mcp({ jsonrpc: '2.0', id: 7, method: 'server/discover', params: { _meta: meta } }, { 'mcp-protocol-version': '2026-07-28', 'mcp-method': 'server/discover' });
  assert.ok(discover.json.result.supportedVersions.includes('2026-07-28'));
  assert.equal((await mcp({ jsonrpc: '2.0', id: 8, method: 'nope', params: { _meta: meta } }, { 'mcp-protocol-version': '2026-07-28', 'mcp-method': 'nope' })).status, 404);
  assert.equal((await call('GET', '/mcp', undefined, { authorization: `Bearer ${token}` }, null)).status, 405);
  const evil = await call('POST', '/mcp', { jsonrpc: '2.0', id: 9, method: 'tools/list' }, { origin: 'https://evil.example', authorization: `Bearer ${token}` }, null);
  assert.equal(evil.status, 403, 'Origin checked');
});

test('the browser: only this machine\'s pages', async () => {
  for (const url of ['https://example.com/', 'file:///etc/passwd', 'http://192.168.1.1/', 'javascript:alert(1)', 'http://localhost.evil.example/']) {
    const r = await call('POST', '/v1/browser/open', { url, page: 't' });
    assert.equal(r.status, 403, url);
  }
});

const chrome = ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Chromium.app/Contents/MacOS/Chromium', process.env.CHROME_PATH].find((p) => p && fs.existsSync(p));

test('the browser: open, read, click, type, screenshot (the machine\'s Chrome over CDP)', { skip: !chrome || typeof WebSocket !== 'function' ? 'no Chrome or no global WebSocket' : false, timeout: 60000 }, async () => {
  const url = `${base}/app/`;
  const open = await call('POST', '/v1/browser/open', { url, page: 't' });
  assert.equal(open.status, 200, open.text);
  assert.equal(open.json.title, 'Todo');
  assert.match(open.json.text, /heading "Todo"/);
  assert.match(open.json.text, /button "Add"/);
  assert.ok(fs.existsSync(open.json.screenshot) && open.json.screenshot.endsWith('.png'));
  assert.ok(open.json.jpeg.length > 100, 'a small jpeg, base64');
  const click = await call('POST', '/v1/browser/click', { target: 'Add', page: 't' });
  assert.match(click.json.did, /clicked button "Add"/);
  assert.equal(click.json.title, 'clicked');
  assert.ok(click.json.errors.some((e) => e.includes('boom')), 'console errors');
  const typed = await call('POST', '/v1/browser/type', { target: 'new item', text: 'milk', page: 't' });
  assert.match(typed.json.text, /textbox "new item" = "milk"/);
  assert.equal((await call('POST', '/v1/browser/click', { target: 'Nothing like this', page: 't' })).status, 404);
  const stop = await call('POST', '/v1/stop-all');
  assert.equal(stop.json.browser, true, 'stop-all closes the browser');
});

test('stop-all stops running sessions', async () => {
  const proj = path.join(root, 'web');
  const s = await call('POST', '/v1/exec/start', { cwd: proj, command: 'sleep 30', mode: 'background' });
  const r = await call('POST', '/v1/stop-all');
  assert.ok(r.json.stopped >= 1);
  for (let i = 0; i < 60; i++) {
    if ((await call('GET', `/v1/exec/session?id=${s.json.id}`)).json.status === 'exited') break;
    await new Promise((res) => setTimeout(res, 100));
  }
  assert.equal((await call('GET', `/v1/exec/session?id=${s.json.id}`)).json.status, 'exited');
  void http;
});

test('evidence: runs the project\'s dry evidence, reads its summary, serves only its images', async () => {
  const proj = path.join(root, 'ev');
  fs.mkdirSync(path.join(proj, 'evidence'), { recursive: true });
  fs.writeFileSync(path.join(proj, 'evidence.md'), '| scenario | route |\n|-|-|\n| home | / |\n');
  fs.writeFileSync(path.join(proj, 'evidence', 'home@1280x800.png'), Buffer.from([137, 80, 78, 71]));
  // A stand-in for `dry`: records its arguments, writes a failed run.
  const fake = path.join(tmp, 'fake-dry.sh');
  fs.writeFileSync(fake, `#!/bin/sh\necho "$@" > "${path.join(tmp, 'dry-args')}"\nprintf '# Evidence\\n\\n1 shot · 1 changed (home@1280x800 2.00%%) · fail\\n' > evidence/summary.md\necho '{"shots":[{"name":"home@1280x800"}],"status":"fail"}' > evidence/manifest.json\necho 'evidence: 1 shot · 1 changed · fail'\nexit 1\n`);
  fs.chmodSync(fake, 0o755);
  const e = createCompanion({ port: 0, roots: [root], folder: root, config: path.join(tmp, 'companion.json'), quiet: true, store: path.join(tmp, 'store-ev'), app: false, dry: fake, lmstudio: 'http://127.0.0.1:9', ollama: 'http://127.0.0.1:9' });
  const port = await e.listen();
  const tok = (await (await fetch(`http://127.0.0.1:${port}/v1/pair`, { method: 'POST', headers: { origin: ORIGIN, 'content-type': 'application/json' }, body: JSON.stringify({ code: e.pairCode() }) })).json()).token;
  const go = async (method, p, body) => {
    const r = await fetch(`http://127.0.0.1:${port}${p}`, { method, headers: { origin: ORIGIN, 'x-dry-token': tok, ...(body ? { 'content-type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
    return { status: r.status, type: r.headers.get('content-type'), body: Buffer.from(await r.arrayBuffer()) };
  };
  try {
    assert.match((await go('GET', `/v1/evidence?project=${encodeURIComponent(proj)}`)).body.toString(), /no evidence yet/);
    const run = await go('POST', '/v1/evidence/run', { project: proj, expect: 'home, settings@390x844' });
    assert.equal(run.status, 200);
    const r = JSON.parse(run.body.toString());
    assert.equal(r.code, 1);
    assert.equal(r.line, '1 shot · 1 changed (home@1280x800 2.00%) · fail');
    assert.equal(fs.readFileSync(path.join(tmp, 'dry-args'), 'utf8').trim(), `evidence --project ${proj} --quiet --expect home,settings@390x844`);
    assert.equal((await go('POST', '/v1/evidence/run', { project: proj, expect: 'a;rm -rf /' })).status, 400, 'no shell in expect');
    assert.equal((await go('POST', '/v1/evidence/run', { project: path.join(root, 'web') })).status, 404, 'no evidence.md');
    assert.equal((await go('POST', '/v1/evidence/run', { project: tmp })).status, 403, 'only inside a root');
    const img = await go('GET', `/v1/evidence/file?project=${encodeURIComponent(proj)}&name=home%401280x800.png`);
    assert.equal(img.type, 'image/png');
    assert.equal(img.body[1], 80);
    assert.equal((await go('GET', `/v1/evidence/file?project=${encodeURIComponent(proj)}&name=..%2Fevidence.md`)).status, 400);
    const tools = await fetch(`http://127.0.0.1:${port}/mcp`, { method: 'POST', headers: { origin: ORIGIN, 'x-dry-token': tok, 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'evidence_read', arguments: { project: proj } } }) });
    const t = await tools.json();
    assert.match(t.result.content[0].text, /^evidence: 1 shot · 1 changed/);
  } finally { await e.close(); }
});
