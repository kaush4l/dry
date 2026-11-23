// The conformance suite's harness (docs/companion-protocol.md §13): starts a
// companion from outside with the contract's command line, reads its
// `listening` line and its pairing code file, and talks to it over HTTP and
// MCP only. Any companion (Node, Rust, Python) is started the same way:
//
//   DRY_COMPANION_CMD="dry companion" node --test conformance/
//
// Black box: nothing here imports a companion.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const HERE = path.dirname(fileURLToPath(import.meta.url));
export const FIXTURES = path.join(HERE, 'fixtures');
const DEFAULT_CMD = `node ${JSON.stringify(path.join(HERE, '..', 'companion', 'dry-companion.mjs'))}`;

/// The command that starts the companion under test, as an argument list.
export function command() {
  return words(process.env.DRY_COMPANION_CMD || DEFAULT_CMD);
}

/// A command line split into words: spaces separate, "..." and '...' group.
export function words(line) {
  const out = [];
  let cur = null, q = null;
  for (const ch of line) {
    if (q) { if (ch === q) q = null; else cur += ch; continue; }
    if (ch === '"' || ch === "'") { q = ch; cur ??= ''; continue; }
    if (/\s/.test(ch)) { if (cur != null) out.push(cur); cur = null; continue; }
    cur = (cur ?? '') + ch;
  }
  if (cur != null) out.push(cur);
  return out;
}

export function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); });
  });
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/// Wait until `f()` is truthy (polling), or fail saying `what`.
export async function until(f, what, ms = 10000) {
  const t0 = Date.now();
  for (;;) {
    const v = await f();
    if (v) return v;
    if (Date.now() - t0 > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(50);
  }
}

/// A scratch folder with `files` in it (`{"a/b.txt": "text"}`; a Buffer for bytes).
export function scratch(name, files = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `dry-conf-${name}-`));
  for (const [rel, body] of Object.entries(files)) {
    const full = path.join(dir, ...rel.split('/'));
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, body);
  }
  return fs.realpathSync(dir);
}

/// Start a companion on a scratch folder. `files`: the workspace's files;
/// `caps`: capability files; `providers`: providers.json's entries; `on`:
/// capabilities switched on; `env`: more environment; `setup(tmp)`: before it starts.
export async function start({ name = 'c', files = {}, caps = {}, providers = [], on = [], off = [], env = {}, setup } = {}) {
  const tmp = scratch(name);
  const ws = path.join(tmp, 'ws');
  fs.mkdirSync(ws);
  for (const [rel, body] of Object.entries(files)) {
    const full = path.join(ws, ...rel.split('/'));
    fs.mkdirSync(path.dirname(full), { recursive: true });
    fs.writeFileSync(full, body);
  }
  const capsDir = path.join(tmp, 'caps');
  fs.mkdirSync(capsDir);
  for (const [rel, body] of Object.entries(caps)) fs.writeFileSync(path.join(capsDir, rel), body);
  const providersFile = path.join(tmp, 'providers.json');
  fs.writeFileSync(providersFile, JSON.stringify({ version: 1, providers }, null, 2));
  if (setup) await setup({ tmp, ws, capsDir, providersFile });
  const port = await freePort();
  const codeFile = path.join(tmp, 'code');
  const [bin, ...pre] = command();
  const args = [...pre, '--folder', ws, '--port', String(port), '--yes', '--pair-code-file', codeFile, '--config', path.join(tmp, 'companion.json'),
    '--capabilities', capsDir, '--providers', providersFile, '--heartbeat', '1', '--no-app'];
  if (on.length) args.push('--on', on.join(','));
  if (off.length) args.push('--off', off.join(','));
  const proc = spawn(bin, args, { cwd: tmp, env: { ...process.env, ...env }, stdio: ['pipe', 'pipe', 'pipe'] });
  let out = '';
  proc.stdout.on('data', (d) => { out += d; });
  proc.stderr.on('data', (d) => { out += d; });
  const exited = new Promise((r) => proc.on('exit', (code, signal) => r({ code, signal })));
  const listening = await Promise.race([
    until(() => /listening http:\/\/127\.0\.0\.1:(\d+)/.exec(out), 'the `listening http://127.0.0.1:<port>` line', 30000),
    exited.then((e) => { throw new Error(`the companion exited (${e.code ?? e.signal}) before listening:\n${out}`); }),
  ]);
  if (Number(listening[1]) !== port) throw new Error(`it listens on ${listening[1]}, not the --port ${port} it was given`);
  const base = `http://127.0.0.1:${port}`;
  await until(() => fs.existsSync(codeFile) && fs.readFileSync(codeFile, 'utf8').trim().length === 6, 'the pairing code file');
  const c = {
    tmp, ws, capsDir, providersFile, port, base, proc,
    output: () => out,
    code: () => fs.readFileSync(codeFile, 'utf8').trim(),
    /// A call: `{status, json, headers, text}`. `token`: sent as x-dry-token.
    async call(method, p, body, { token = c.token, headers = {} } = {}) {
      const h = { ...headers };
      if (token) h['x-dry-token'] = token;
      if (body !== undefined) h['content-type'] = 'application/json';
      const r = await fetch(base + p, { method, headers: h, body: body === undefined ? undefined : typeof body === 'string' ? body : JSON.stringify(body) });
      const text = await r.text();
      let json = null;
      try { json = JSON.parse(text); } catch { /* not JSON */ }
      return { status: r.status, json, headers: r.headers, text };
    },
    /// Pair with the code now: the token (also kept as `c.token` when `keep`).
    async pair(keep = true) {
      const used = c.code();
      const r = await c.call('POST', '/v1/pair', { code: used }, { token: null });
      if (r.status !== 200) throw new Error(`pairing failed: ${r.status} ${r.text}`);
      await until(() => c.code() !== used, 'a new pairing code in the code file');
      if (keep) c.token = r.json.token;
      return r.json;
    },
    /// One JSON-RPC call to /mcp: the whole answer (`result` or `error`).
    async rpc(method, params = {}, { token = c.token, id = 1 } = {}) {
      const r = await fetch(`${base}/mcp`, { method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', authorization: `Bearer ${token}` },
        body: JSON.stringify({ jsonrpc: '2.0', id, method, params }) });
      const text = await r.text();
      try { return { status: r.status, ...JSON.parse(text) }; } catch { return { status: r.status, text }; }
    },
    /// A server-sent stream: `lines` grows as it reads; `stop()` ends it; `ended` resolves when it closes.
    stream(p, { token = c.token, accept = 'text/event-stream' } = {}) {
      const ctl = new AbortController();
      const s = { lines: [], text: '', closed: false };
      s.ended = (async () => {
        try {
          const r = await fetch(base + p, { headers: { accept, authorization: `Bearer ${token}` }, signal: ctl.signal });
          s.status = r.status;
          const reader = r.body.getReader();
          for (;;) {
            const { done, value } = await reader.read();
            if (done) break;
            s.text += Buffer.from(value).toString('utf8');
            s.lines = s.text.split('\n');
          }
        } catch { /* aborted or closed */ }
        s.closed = true;
      })();
      s.stop = () => ctl.abort();
      return s;
    },
    /// A raw request (to set Host and other headers fetch will not).
    raw(method, p, headers = {}) {
      return new Promise((resolve, reject) => {
        const req = http.request({ host: '127.0.0.1', port, method, path: p, headers }, (res) => {
          let body = '';
          res.on('data', (d) => { body += d; });
          res.on('end', () => { let json = null; try { json = JSON.parse(body); } catch { /* not JSON */ } resolve({ status: res.statusCode, headers: res.headers, json, text: body }); });
        });
        req.on('error', reject);
        req.end();
      });
    },
    async stop() {
      if (proc.exitCode == null && proc.signalCode == null) {
        proc.kill('SIGTERM');
        const t = setTimeout(() => proc.kill('SIGKILL'), 5000);
        await exited;
        clearTimeout(t);
      }
      fs.rmSync(tmp, { recursive: true, force: true });
    },
    exited,
  };
  return c;
}

/// A fake OpenAI-compatible server: `/v1/models` lists `models`; a chat
/// answers `<reply> (<model asked>)`. `calls` holds each chat body.
export async function fakeModels(models = ['m1', 'm2'], reply = 'pong') {
  const calls = [];
  const srv = http.createServer((req, res) => {
    let body = '';
    req.on('data', (d) => { body += d; });
    req.on('end', () => {
      if (req.url === '/v1/models') { res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify({ object: 'list', data: models.map((id) => ({ id, object: 'model' })) })); return; }
      if (req.url === '/v1/chat/completions') {
        const b = JSON.parse(body || '{}');
        calls.push(b);
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ id: 'x', object: 'chat.completion', model: b.model, choices: [{ index: 0, message: { role: 'assistant', content: `${reply} (${b.model})` }, finish_reason: 'stop' }] }));
        return;
      }
      res.writeHead(404); res.end();
    });
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  return { url: `http://127.0.0.1:${srv.address().port}`, calls, close: () => new Promise((r) => { srv.close(() => r()); srv.closeAllConnections?.(); }) };
}

/// Whether something answers on a local port now.
export function answers(port) {
  return new Promise((resolve) => {
    const s = net.connect(port, '127.0.0.1');
    s.once('connect', () => { s.destroy(); resolve(true); });
    s.once('error', () => resolve(false));
  });
}
