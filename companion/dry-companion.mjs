#!/usr/bin/env node
// dry companion: a small local server that gives the dry web app (the
// hosted page or a locally served dist) what a browser cannot do itself:
// this machine's projects, its runtimes, local inference and git.
//
//   node dry-companion.mjs [--folder DIR] [--port 7777] [--yes] [--on ID]... [--off ID]...
//                          [--providers FILE] [--capabilities DIR] [--origin URL]... [--quiet]
//
// Node 20+, no npm dependencies, one file. The wire contract every
// companion keeps (this one, the Rust `dry companion`, the Python script)
// is docs/companion-protocol.md, checked by conformance/; the rest is
// docs/companion.md. In short: it listens on 127.0.0.1 only, answers only
// allowed page origins, and every call but /v1/health and /v1/pair needs
// the session token a page gets by sending the one-time code printed here.
// Tokens live in this process's memory only. The page gets one folder
// (--folder, default the current one): its files and a terminal in it,
// and the page's state in <folder>/.dry/.

import http from 'node:http';
import { spawn, execFile } from 'node:child_process';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import { pathToFileURL, fileURLToPath } from 'node:url';
import readline from 'node:readline';

export const VERSION = '0.1.0';
export const PROTOCOL = 1;
export const DEFAULT_PORT = 7777;
const NAME = 'dry-companion';
const MAX_BODY = 8 * 1024 * 1024;
const MAX_READ = 1_000_000; // the editor's limit (dry_core::workbench::MAX_OPEN)
const MAX_OUTPUT = 256 * 1024; // what a session keeps of its output
const MAX_TIMEOUT = 60 * 60 * 1000;
const MAX_EVENTS = 1000; // the event log a page resumes from
const DEFAULT_TTL_HOURS = 12; // how long a pairing lasts
/// Input types a custom tool may declare.
const TYPES = ['string', 'integer', 'number', 'boolean'];
/// Arguments after which an input would run as code (`sh -c {x}`): refused.
const CODE_FLAGS = ['-c', '-e', '--eval', '-p', '--print', '-Command', '-command', '/c', '/C', '-r', '--exec'];
const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.wasm': 'application/wasm', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp', '.ico': 'image/x-icon',
  '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff': 'font/woff', '.woff2': 'font/woff2', '.txt': 'text/plain; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8', '.map': 'application/json', '.ts': 'text/plain; charset=utf-8', '.gz': 'application/gzip',
  '.bin': 'application/octet-stream', '.img': 'application/octet-stream', '.iso': 'application/octet-stream',
};
const HERE = path.dirname(fileURLToPath(import.meta.url));
const FAILED_PAIRS = 5; // wrong codes before the code changes
const HEARTBEAT_MS = 15000; // a `: ping` on every open stream at least this often
/// The folder dry keeps its own files in, inside the workspace folder.
export const DRY_DIR = '.dry';
/// A terminal session's key: what the page names it by (`<engine>/<artifact>/<n>`).
export const KEY_RE = /^[A-Za-z0-9._:\/-]{1,200}$/;
/// The error code a status has when a call does not name a finer one (docs/companion-protocol.md §2).
export const CODES = { 400: 'bad_request', 401: 'not_paired', 403: 'forbidden', 404: 'not_found', 405: 'method', 409: 'exists', 413: 'too_large', 428: 'needs_confirm', 500: 'internal', 502: 'upstream', 503: 'unavailable', 504: 'timeout' };
/// The providers.json a companion starts from when it has none (the repo's
/// providers.json is the same, with the examples).
export const PROVIDERS_TEMPLATE = {
  version: 1,
  providers: [],
  examples: [
    { name: 'claude', kind: 'cli', cli: 'claude', model: 'sonnet' },
    { name: 'codex', kind: 'cli', cli: 'codex' },
    { name: 'copilot', kind: 'cli', cli: 'copilot', path: '/usr/local/bin/copilot' },
    { name: 'lmstudio', kind: 'port', url: 'http://127.0.0.1:1234' },
    { name: 'ollama', kind: 'port', url: 'http://127.0.0.1:11434' },
    { name: 'local', kind: 'native', runner: 'auto', model: 'mlx-community/Qwen3-4B-4bit' },
  ],
};

/// Folders never listed (dry_core::workbench::HIDDEN).
export const HIDDEN = new Set(['.git', 'target', 'node_modules', '.DS_Store']);
/// Files that mark a folder as a project.
export const MARKERS = [
  '.git', 'Cargo.toml', 'package.json', 'pyproject.toml', 'requirements.txt', 'setup.py', 'pom.xml',
  'build.gradle', 'build.gradle.kts', 'go.mod', 'Gemfile', 'composer.json', 'deno.json', 'CMakeLists.txt',
  'Makefile', 'Package.swift', 'mix.exs', 'pubspec.yaml',
];
/// The runtimes looked for on PATH, and how each says its version.
export const RUNTIMES = {
  git: ['--version'], gh: ['--version'], node: ['--version'], python3: ['--version'], cargo: ['--version'],
  mvn: ['-v'], java: ['-version'], docker: ['--version'], podman: ['--version'], claude: ['--version'],
  copilot: ['--version'],
};

// ---- rules, pure (exported for the tests) ----

/// Origins a page may call from: the hosted app, any local copy, and the extras given.
export function originAllowed(origin, extra = []) {
  if (!origin || origin === 'null') return false;
  if (origin === 'https://kaush4l.github.io') return true;
  if (/^http:\/\/(localhost|127\.0\.0\.1|\[::1\])(:\d{1,5})?$/.test(origin)) return true;
  return extra.includes(origin.replace(/\/$/, ''));
}

/// The Host header must name this server by a loopback name, so a page on
/// another name that resolves here (DNS rebinding) is refused.
export function hostAllowed(host, port) {
  if (!host) return false;
  return [`127.0.0.1:${port}`, `localhost:${port}`, `[::1]:${port}`].includes(host.toLowerCase());
}

/// Whether a file name says it holds secrets (dry_core::file::secret_file, plus key folders).
export function secretName(name) {
  const n = name.toLowerCase();
  return n === '.env' || n.startsWith('.env.') || ['.netrc', '.npmrc', '.pypirc', '.pgpass', '.git-credentials'].includes(n)
    || n.startsWith('id_rsa') || n.startsWith('id_ed25519') || n.startsWith('id_ecdsa') || n.startsWith('credentials')
    || n.startsWith('secrets.') || ['.pem', '.key', '.p12', '.pfx', '.keystore'].some((e) => n.endsWith(e))
    || ['.ssh', '.gnupg', '.aws', '.kube', '.docker'].includes(n);
}

/// Whether an environment variable's name says it holds a secret (dry_core::terminal::secret_env).
export function secretEnv(name) {
  const n = name.toUpperCase();
  return ['KEY', 'TOKEN', 'SECRET', 'PASSWORD', 'PASSWD', 'CREDENTIAL', 'AUTH', 'COOKIE', 'SESSION'].some((w) => n.includes(w));
}

/// Commands that publish or fetch from outside: only through the git
/// endpoints, with a person's click (docs/companion.md, "Confirmation").
export function needsConfirm(command) {
  return /(^|[\s;&|(`])git(\s+-\S+(\s+[^-\s]\S*)?)*\s+(push|clone)\b/.test(command)
    || /(^|[\s;&|(`])gh\s+(repo\s+(create|clone|delete|fork)|pr\s+(create|merge)|release\s+create)\b/.test(command);
}

/// A clone URL the git endpoint accepts.
export function cloneUrlOk(url) {
  return typeof url === 'string' && !url.startsWith('-') && !/\s/.test(url)
    && (/^https:\/\/[^/]+\/.+/.test(url) || /^ssh:\/\/[^/]+\/.+/.test(url) || /^[\w.-]+@[\w.-]+:[^\s]+$/.test(url) || /^file:\/\/\/.+/.test(url));
}

/// A remote or branch name passed to git (not an option, no odd characters).
export function refOk(name) {
  return typeof name === 'string' && /^[A-Za-z0-9][A-Za-z0-9._/-]{0,199}$/.test(name) && !name.includes('..') && !name.endsWith('.lock') && !name.endsWith('/');
}

/// A store path: relative, `/`-separated, no `.` or `..` parts.
export function storePathOk(p) {
  return typeof p === 'string' && p.length > 0 && p.length < 512 && !p.startsWith('/') && !p.includes('\\') && !p.includes('\0')
    && p.split('/').every((s) => s !== '' && s !== '.' && s !== '..');
}

export function within(real, base) {
  return real === base || real.startsWith(base.endsWith(path.sep) ? base : base + path.sep);
}

// ---- providers.json (docs/companion-protocol.md §8), pure ----

export const PROVIDER_KINDS = ['cli', 'port', 'native'];
export const CLIS = ['claude', 'codex', 'copilot'];
export const RUNNERS = ['auto', 'mlx', 'llama.cpp'];
/// How to get each program the providers name: printed when it is missing (never run).
export const INSTALL = {
  claude: 'npm install -g @anthropic-ai/claude-code',
  codex: 'npm install -g @openai/codex',
  copilot: 'npm install -g @github/copilot',
  mlx: 'pip install mlx-lm   (Apple silicon; gives mlx_lm.server)',
  'llama.cpp': 'brew install llama.cpp, or build https://github.com/ggml-org/llama.cpp (-DGGML_CUDA=ON on NVIDIA, -DGGML_VULKAN=ON for Vulkan)',
};
const MODEL_RE = /^[A-Za-z0-9._:@\/+-]{1,200}$/;

/// One providers.json entry, checked and normalised, or an Error saying why.
export function checkProvider(e) {
  if (!e || typeof e !== 'object' || Array.isArray(e)) throw new Error('a provider is an object');
  const name = String(e.name ?? '');
  if (!/^[a-z0-9][a-z0-9_-]{0,39}$/.test(name)) throw new Error(`name: ${JSON.stringify(e.name ?? null)} is not a provider name (lower case letters, digits, - and _)`);
  if (!PROVIDER_KINDS.includes(e.kind)) throw new Error(`${name}: kind is one of ${PROVIDER_KINDS.join(', ')}`);
  const out = { name, kind: e.kind };
  const str = (k, re, why) => {
    if (e[k] == null || e[k] === '') return;
    if (typeof e[k] !== 'string' || (re && !re.test(e[k])) || e[k].includes('\0')) throw new Error(`${name}: ${k} ${why}`);
    out[k] = e[k];
  };
  str('model', MODEL_RE, 'is a model name or path');
  str('path', null, 'is the program\'s path');
  if (out.path && !path.isAbsolute(out.path.replace(/^~(?=$|\/)/, os.homedir()))) throw new Error(`${name}: path is an absolute path to the program`);
  if (e.kind === 'cli') {
    if (!CLIS.includes(e.cli ?? name)) throw new Error(`${name}: cli is one of ${CLIS.join(', ')}`);
    out.cli = e.cli ?? name;
  } else if (e.kind === 'port') {
    let u;
    try { u = new URL(String(e.url ?? '')); } catch { throw new Error(`${name}: url is the server's address (http://127.0.0.1:1234)`); }
    if (u.protocol !== 'http:' && u.protocol !== 'https:') throw new Error(`${name}: url is http:// or https://`);
    out.url = u.href.replace(/\/+$/, '').replace(/\/v1$/, '');
  } else {
    const runner = e.runner ?? 'auto';
    if (!RUNNERS.includes(runner)) throw new Error(`${name}: runner is one of ${RUNNERS.join(', ')}`);
    out.runner = runner;
    if (!out.model) throw new Error(`${name}: a native provider needs a model (a local file or folder, or a model id already downloaded)`);
    if (e.port != null) {
      if (!Number.isInteger(e.port) || e.port < 1024 || e.port > 65535) throw new Error(`${name}: port is 1024-65535`);
      out.port = e.port;
    }
  }
  return out;
}

/// A providers.json text: its entries, and the ones that did not read (with why).
export function parseProviders(text) {
  let v;
  try { v = JSON.parse(text); } catch (e) { return { providers: [], errors: [{ entry: null, error: `providers.json is not JSON: ${e.message}` }] }; }
  if (!v || typeof v !== 'object' || !Array.isArray(v.providers)) return { providers: [], errors: [{ entry: null, error: 'providers.json needs "providers": [ ... ]' }] };
  const providers = [], errors = [];
  for (const e of v.providers) {
    try {
      const p = checkProvider(e);
      if (providers.some((x) => x.name === p.name)) throw new Error(`a second provider named ${p.name}`);
      providers.push(p);
    } catch (err) { errors.push({ entry: e?.name ?? null, error: err.message }); }
  }
  return { providers, errors };
}

/// The runner a native provider uses on this machine: Apple silicon runs
/// mlx (mlx_lm.server), NVIDIA llama.cpp with CUDA, anything else llama.cpp
/// on the CPU (or Vulkan where it was built with it).
export function chooseRunner(runner, { platform = os.platform(), arch = os.arch(), nvidia = false } = {}) {
  if (runner && runner !== 'auto') return { runner, accel: runner === 'mlx' ? 'metal' : nvidia ? 'cuda' : platform === 'darwin' ? 'metal' : 'cpu' };
  if (platform === 'darwin' && arch === 'arm64') return { runner: 'mlx', accel: 'metal' };
  return { runner: 'llama.cpp', accel: nvidia ? 'cuda' : 'cpu' };
}

/// The runner's program name and its arguments: the model it serves on a
/// local port. Never a download flag (llama.cpp's -hf): the model is on disk.
export function runnerArgv(runner, bin, model, port) {
  if (runner === 'mlx') return [bin, '--model', model, '--host', '127.0.0.1', '--port', String(port)];
  return [bin, '-m', model, '--host', '127.0.0.1', '--port', String(port)];
}

export const RUNNER_BIN = { mlx: 'mlx_lm.server', 'llama.cpp': 'llama-server' };

// ---- the workspace folder (docs/companion-protocol.md §6), pure ----

/// A path the page gives for the folder: relative, `/`-separated, no part
/// that leaves it; `''` or `.` is the folder itself. Its parts, or why not.
export function folderParts(p) {
  if (typeof p !== 'string' || p.includes('\0')) return { error: 'a path is text', code: 'bad_request' };
  const t = p.replace(/\\/g, '/');
  if (t.startsWith('/') || /^[A-Za-z]:/.test(t)) return { error: `${p} is absolute: paths are relative to the folder`, code: 'path_outside' };
  const parts = [];
  for (const s of t.split('/')) {
    if (s === '' || s === '.') continue;
    if (s === '..') { if (!parts.length) return { error: `${p} leads outside the folder`, code: 'path_outside' }; parts.pop(); continue; }
    parts.push(s);
  }
  for (const s of parts) {
    if (secretName(s)) return { error: `${s} may hold secrets, so it is never read, written or listed`, code: 'secret_path' };
  }
  if (parts[0] === DRY_DIR) return { error: `${DRY_DIR}/ is dry's own state: it goes through /v1/store and /v1/workspace`, code: 'dry_state' };
  return { parts };
}

// ---- declared capabilities (docs/capabilities.md §3), pure ----

const CAP_ID = /^[a-z0-9_-]{1,64}$/;
const INPUT_NAME = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;

/// `60s`, `5m`, `500ms`, `90` as seconds (rounded up), or null.
export function capDuration(v) {
  const m = /^(\d+)\s*(ms|s|m|h)?$/.exec(String(v ?? '').trim());
  if (!m) return null;
  const n = Number(m[1]);
  return { ms: Math.ceil(n / 1000), s: n, m: n * 60, h: n * 3600 }[m[2] || 's'];
}

function capArgv(v) {
  const t = v.trim();
  let list;
  if (t.startsWith('[')) {
    try { list = JSON.parse(t); } catch (e) { throw new Error(`not a JSON list of strings: ${e.message}`); }
    if (!Array.isArray(list) || !list.every((x) => typeof x === 'string')) throw new Error('not a JSON list of strings');
  } else list = t.split(/\s+/).filter(Boolean);
  if (!list.length || list.some((x) => !x)) throw new Error('a command is an argument list, the program first');
  return list;
}

/// A `when:` condition: `contains <text>`, `missing <text>`, `<key> <op> <value>`.
export function parseWhen(text) {
  const t = text.trim();
  const unq = (s) => s.trim().replace(/^"|"$/g, '');
  if (t.startsWith('contains ')) return { kind: 'contains', text: unq(t.slice(9)) };
  if (t.startsWith('missing ')) return { kind: 'missing', text: unq(t.slice(8)) };
  const m = /^(\S+)\s+(<=|>=|!=|==|<|>|=)\s+(.+)$/.exec(t);
  if (!m) throw new Error(`\`${t}\`: a condition is \`contains <text>\`, \`missing <text>\` or \`<key> <op> <value>\` (op: < <= > >= = !=)`);
  const op = m[2] === '==' ? '=' : m[2];
  if (['<', '<=', '>', '>='].includes(op) && Number.isNaN(Number(m[3].trim()))) throw new Error(`\`${t}\`: ${op} compares numbers`);
  return { kind: 'compare', key: m[1], op, value: unq(m[3]) };
}

export function whenText(w) {
  return w.kind === 'compare' ? `${w.key} ${w.op} ${w.value}` : `${w.kind} ${w.text}`;
}

/// The value of `key` in a snapshot: `key: v` or `key=v` (dry_core::capability::value_of).
export function snapshotValue(snap, key) {
  for (const line of snap.split('\n')) {
    let from = 0;
    for (;;) {
      const at = line.indexOf(key, from);
      if (at < 0) break;
      const before = at === 0 || /[ \t;,]$/.test(line.slice(0, at));
      const rest = line.slice(at + key.length);
      if (before && (rest[0] === ':' || rest[0] === '=')) {
        const v = rest.slice(1).trimStart();
        const end = v.search(/[;,\t]/);
        return (end < 0 ? v : v.slice(0, end)).trim();
      }
      from = at + key.length;
    }
  }
  return null;
}

export function whenHolds(w, snap) {
  if (w.kind === 'contains') return snap.includes(w.text);
  if (w.kind === 'missing') return !snap.includes(w.text);
  const v = snapshotValue(snap, w.key);
  if (v == null) return false;
  if (w.op === '=') return v === w.value;
  if (w.op === '!=') return v !== w.value;
  const a = parseFloat(v), b = Number(w.value);
  if (Number.isNaN(a)) return false;
  return { '<': a < b, '<=': a <= b, '>': a > b, '>=': a >= b }[w.op];
}

/// One `capabilities/<id>.md` (dry_core::capability::declared::parse, the
/// same rules): errors say `file:line: why`. Inputs only ever as whole
/// arguments (`{name}`), never inside one, never the program, never after
/// a code flag.
export function parseCapability(file, text) {
  const err = (n, why) => new Error(`${file}:${n}: ${why}`);
  const d = { id: '', doc: [], probe: null, snapshot: null, interval: 60, tools: [], events: [] };
  let part = 'head', sec = null, inInputs = false, intervalSet = false;
  const finish = () => {
    if (!sec) return;
    if (part === 'tool') {
      if (!sec.cmd) throw err(sec.start, `tool ${sec.name} has no \`command:\``);
      const names = new Set(sec.inputs.map((i) => i.name));
      sec.cmd.list.forEach((arg, n) => {
        const whole = /^\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(arg);
        if (whole) {
          if (!names.has(whole[1])) throw err(sec.cmd.at, `{${whole[1]}} is not one of its inputs`);
          if (n === 0) throw err(sec.cmd.at, 'the program itself cannot be an input');
          if (CODE_FLAGS.includes(sec.cmd.list[n - 1])) throw err(sec.cmd.at, `an input cannot follow ${sec.cmd.list[n - 1]}: it would run as code`);
        } else if ([...arg.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)].some((m) => names.has(m[1]))) {
          throw err(sec.cmd.at, `"${arg}": an input goes in as a whole argument ({name}), never inside one`);
        }
      });
      if (d.tools.some((t) => t.name === sec.name)) throw err(sec.start, `a second tool named ${sec.name}`);
      d.tools.push({ name: sec.name, doc: sec.doc.join(' '), inputs: sec.inputs, command: sec.cmd.list, approval: sec.approval, timeout: sec.timeout ?? 30 });
    } else if (part === 'event') {
      if (!sec.when) throw err(sec.start, `event ${sec.name} has no \`when:\``);
      if (d.events.some((e) => e.name === sec.name)) throw err(sec.start, `a second event named ${sec.name}`);
      d.events.push({ name: sec.name, when: sec.when, text: sec.doc.length ? sec.doc.join(' ') : `${sec.name}: ${whenText(sec.when)}` });
    }
  };
  const lines = text.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const n = i + 1, line = lines[i].replace(/\s+$/, ''), t = line.trim();
    if (!d.id) {
      if (!t) continue;
      if (!t.startsWith('# ')) throw err(n, 'the file starts with `# <id>`, the capability\'s id');
      const id = t.slice(2).trim();
      if (!CAP_ID.test(id)) throw err(n, `\`${id}\` is not a capability id (lower case letters, digits, - and _)`);
      d.id = id;
      continue;
    }
    if (t.startsWith('## ')) {
      finish();
      inInputs = false;
      const head = t.slice(3);
      const at = head.indexOf(':');
      const what = (at < 0 ? head : head.slice(0, at)).trim(), name = at < 0 ? '' : head.slice(at + 1).trim();
      if (what !== 'tool' && what !== 'event') throw err(n, `\`## ${head}\`: a section is \`## tool: <name>\` or \`## event: <name>\``);
      if (!INPUT_NAME.test(name)) throw err(n, `\`${name}\` is not a ${what} name (letters, digits, _)`);
      part = what;
      sec = { start: n, name, doc: [], cmd: null, inputs: [], approval: false, timeout: null, when: null };
      continue;
    }
    if (!t) continue;
    const input = /^\s*-\s+([A-Za-z0-9_]+)\s*(?:\(([^)]*)\))?\s*:?\s*(.*)$/.exec(line);
    if (inInputs && input) {
      let type = 'string', required = false;
      for (const f of (input[2] || '').split(',').map((x) => x.trim()).filter(Boolean)) {
        if (f === 'required') required = true;
        else if (TYPES.includes(f)) type = f;
        else throw err(n, `\`${f}\`: an input's flags are its type (string, integer, number, boolean) and \`required\``);
      }
      if (!INPUT_NAME.test(input[1])) throw err(n, `\`${input[1]}\` is not an input name (letters, digits, _)`);
      if (sec.inputs.some((x) => x.name === input[1])) throw err(n, `a second input named ${input[1]}`);
      sec.inputs.push({ name: input[1], type, required, description: input[3].trim() });
      continue;
    }
    inInputs = false;
    const kv = /^([a-z]+):\s*(.*)$/.exec(t);
    const k = kv?.[1], v = kv?.[2]?.trim();
    const wrap = (f) => { try { return f(); } catch (e) { throw err(n, e.message); } };
    if (part === 'head' && k === 'probe') d.probe = wrap(() => capArgv(v));
    else if (part === 'head' && k === 'snapshot') {
      d.snapshot = wrap(() => capArgv(v));
      if (d.snapshot.some((a) => /\{[A-Za-z_][A-Za-z0-9_]*\}/.test(a))) throw err(n, 'a snapshot command takes no inputs');
    } else if (part === 'head' && k === 'interval') {
      const s = capDuration(v);
      if (!s) throw err(n, `\`${v}\` is not a duration (60s, 5m)`);
      d.interval = s; intervalSet = true;
    } else if (part === 'tool' && k === 'command') sec.cmd = { at: n, list: wrap(() => capArgv(v)) };
    else if (part === 'tool' && k === 'timeout') { const s = capDuration(v); if (!s) throw err(n, `\`${v}\` is not a duration`); sec.timeout = s; }
    else if (part === 'tool' && k === 'approval') {
      if (!['yes', 'no', 'true', 'false'].includes(v)) throw err(n, `approval is yes or no, not \`${v}\``);
      sec.approval = v === 'yes' || v === 'true';
    } else if (part === 'tool' && k === 'inputs' && v === '') inInputs = true;
    else if (part === 'event' && k === 'when') sec.when = wrap(() => parseWhen(v));
    else if (part === 'head') d.doc.push(t);
    else sec.doc.push(t);
  }
  if (!d.id) throw err(1, 'empty: the file starts with `# <id>`');
  finish();
  if (d.probe && d.probe.some((a) => /\{[A-Za-z_][A-Za-z0-9_]*\}/.test(a))) throw err(1, 'a probe takes no inputs');
  if (intervalSet && !d.snapshot) throw err(1, '`interval:` without a `snapshot:` command');
  if (d.events.length && !d.snapshot) throw err(1, 'events are read from the snapshot: add a `snapshot:` command');
  d.doc = d.doc.join('\n');
  if (!d.doc) throw err(1, 'say what it is and how to use it, under the `# id` line');
  return d;
}

/// The argv a declared tool's call becomes: inputs checked, each a whole
/// argument (left out when optional and not given) and DRY_INPUT_<NAME>.
export function capabilityArgv(d, tool, args) {
  const t = d.tools.find((x) => x.name === tool);
  if (!t) throw Object.assign(new Error(`${d.id} has no tool ${tool}`), { status: 404 });
  args = args ?? {};
  if (typeof args !== 'object' || Array.isArray(args)) throw Object.assign(new Error('arguments is an object'), { status: 400 });
  const bad = (m) => Object.assign(new Error(m), { status: 400 });
  for (const k of Object.keys(args)) if (!t.inputs.some((i) => i.name === k)) throw bad(`${d.id}.${tool} has no input ${k}`);
  const values = {};
  for (const i of t.inputs) {
    const v = args[i.name];
    if (v == null || v === '') { if (i.required) throw bad(`${d.id}.${tool} needs ${i.name}`); continue; }
    const s = typeof v === 'object' ? null : String(v);
    if (s == null) throw bad(`${i.name} must be text`);
    if (i.type === 'integer' && !/^-?\d{1,15}$/.test(s)) throw bad(`${i.name} must be a whole number`);
    if (i.type === 'number' && !/^-?\d+(\.\d+)?([eE][-+]?\d+)?$/.test(s)) throw bad(`${i.name} must be a number`);
    if (i.type === 'boolean' && s !== 'true' && s !== 'false') throw bad(`${i.name} must be true or false`);
    if (s.includes('\0')) throw bad(`${i.name} holds a zero byte`);
    if (s.length > 65536) throw bad(`${i.name} is longer than 64 KB`);
    values[i.name] = s;
  }
  const argv = t.command.flatMap((a) => { const m = /^\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(a); return m ? (m[1] in values ? [values[m[1]]] : []) : [a]; });
  const env = { DRY_TOOL: `${d.id}.${tool}` };
  for (const [k, v] of Object.entries(values)) env[`DRY_INPUT_${k.toUpperCase()}`] = v;
  return { argv, env, timeoutMs: t.timeout * 1000 };
}

function lastLines(text, n) {
  const lines = text.split('\n');
  if (lines.length && lines[lines.length - 1] === '') lines.pop();
  return lines.slice(Math.max(0, lines.length - n)).join('\n');
}

function shortTime(d = new Date()) {
  return d.toTimeString().slice(0, 8);
}

// ---- the server ----

export function createCompanion(opts = {}) {
  const port = opts.port ?? DEFAULT_PORT;
  const quiet = !!opts.quiet;
  const extraOrigins = (opts.origins ?? []).map((o) => o.replace(/\/$/, ''));
  const lmstudio = opts.lmstudio ? opts.lmstudio.replace(/\/$/, '') : null;
  const ollama = opts.ollama ? opts.ollama.replace(/\/$/, '') : null;
  const home = (p) => p.replace(/^~(?=$|\/)/, os.homedir());
  // The workspace: one folder the page gets (--folder, default the current one).
  const folder = (() => {
    const abs = path.resolve(home(opts.folder ?? process.cwd()));
    if (!fs.existsSync(abs) || !fs.statSync(abs).isDirectory()) throw new Error(`the folder ${abs} is not a folder`);
    const real = fs.realpathSync(abs);
    if (real === path.parse(real).root) throw new Error('the whole disk cannot be the folder: give one folder with --folder');
    return real;
  })();
  const dryDir = path.join(folder, DRY_DIR);
  const storeDir = opts.store ?? path.join(dryDir, 'state');
  const snapshotFile = path.join(dryDir, 'snapshot.json');
  const configFile = opts.config ? path.resolve(home(opts.config)) : path.join(dryDir, 'companion.json');
  const providersFile = opts.providers ? path.resolve(home(opts.providers)) : path.join(dryDir, 'providers.json');
  const heartbeatMs = opts.heartbeatMs ?? HEARTBEAT_MS;
  const verbose = !!opts.verbose;
  const roots = (opts.roots?.length ? opts.roots : [folder]).map((r) => {
    const abs = path.resolve(r.replace(/^~(?=$|\/)/, os.homedir()));
    if (!fs.existsSync(abs) || !fs.statSync(abs).isDirectory()) throw new Error(`the root ${abs} is not a folder`);
    return fs.realpathSync(abs);
  });

  const toolsDir = opts.tools ? path.resolve(opts.tools.replace(/^~(?=$|\/)/, os.homedir())) : null;
  const allowUrls = (opts.allowUrls ?? []).map((u) => u.replace(/\/$/, ''));
  const ttlMs = opts.ttlMs ?? (opts.ttlHours ?? DEFAULT_TTL_HOURS) * 3600 * 1000;
  // The built app: --app DIR, else ../dist next to this script if it is there; --no-app: none.
  const appDir = (() => {
    if (opts.app === false) return null;
    const dir = opts.app ? path.resolve(opts.app) : path.join(HERE, '..', 'dist');
    if (fs.existsSync(path.join(dir, 'app', 'index.html')) || fs.existsSync(path.join(dir, 'index.html'))) return fs.realpathSync(dir);
    if (opts.app) throw new Error(`--app ${dir}: no index.html there (build the app first)`);
    return null;
  })();
  const shotDir = path.join(os.tmpdir(), 'dry-companion-shots');

  let code = newCode();
  let failed = 0;
  const pairings = new Map(); // token -> { id, origin, created, expires, seen }; memory only
  let nextPairing = 1;
  const sessions = new Map(); // id -> session
  const listeners = new Set(); // SSE responses
  const events = []; // the event log: { id, event, data, all, at }
  let nextEvent = 1;
  let nextId = 1;
  let runtimes = null;
  const browser = { ws: null, child: null, profile: null, bin: null, starting: null, nextId: 1, pending: new Map(), pages: new Map() };

  function log(line) {
    if (!quiet) console.log(line);
  }
  function newCode() {
    const abc = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
    let c = '';
    for (const b of crypto.randomBytes(6)) c += abc[b % abc.length];
    return c;
  }
  function announceCode() {
    if (opts.pairCodeFile) try { fs.writeFileSync(opts.pairCodeFile, code + '\n'); } catch (e) { console.error(`could not write ${opts.pairCodeFile}: ${e.message}`); }
    if (!quiet) console.log(`\n  pairing code: ${code}   (type it in dry → Settings → Companion; it works once)\n`);
  }

  // ---- paths ----

  /// `p` as a real path inside a root, or an error. `opts.within`: it must
  /// also be inside that folder. `forWrite`: it may not exist yet.
  function confine(p, { forWrite = false, inside = null } = {}) {
    if (typeof p !== 'string' || !p || p.includes('\0')) throw httpError(400, 'a path is needed');
    const abs = path.resolve(path.isAbsolute(p) ? p : path.join(roots[0], p));
    let real;
    if (fs.existsSync(abs)) {
      real = fs.realpathSync(abs);
    } else {
      if (!forWrite) throw httpError(404, `no such file: ${p}`);
      let lst = null;
      try { lst = fs.lstatSync(abs); } catch { /* not there */ }
      if (lst?.isSymbolicLink()) throw httpError(403, `${p} is a link that leads nowhere`);
      let up = path.dirname(abs);
      const rest = [path.basename(abs)];
      while (!fs.existsSync(up)) { rest.unshift(path.basename(up)); up = path.dirname(up); }
      real = path.join(fs.realpathSync(up), ...rest);
    }
    const root = roots.find((r) => within(real, r));
    if (!root) throw httpError(403, `${p} is outside the companion's roots`, 'path_outside');
    for (const part of path.relative(root, real).split(path.sep)) {
      if (part && secretName(part)) throw httpError(403, `${part} may hold secrets, so it is never opened or shown`, 'secret_path');
    }
    if (within(real, dryDir)) throw httpError(403, `${DRY_DIR}/ is dry's own state`, 'dry_state');
    if (inside) {
      const base = confine(inside);
      if (!within(real, base)) throw httpError(403, `${p} leads outside ${inside}`);
    }
    return real;
  }

  // ---- the workspace folder: the page's files and terminal (docs/companion-protocol.md §6) ----

  /// A path in the folder as `{real, rel}`, or an error with its code:
  /// absolute paths, `..` and links that lead out are `path_outside`,
  /// secret-looking names `secret_path`, `.dry/` (and writes into `.git`)
  /// `dry_state`. `forWrite`: it may not exist yet.
  function wsPath(p, { forWrite = false, dir = false } = {}) {
    const r = folderParts(p ?? '');
    if (r.error) throw httpError(r.code === 'bad_request' ? 400 : 403, r.error, r.code);
    const abs = path.join(folder, ...r.parts);
    let real, exists = true;
    try { real = fs.realpathSync(abs); } catch {
      exists = false;
      let lst = null;
      try { lst = fs.lstatSync(abs); } catch { /* not there */ }
      if (lst?.isSymbolicLink()) throw httpError(403, `${p} is a link that leads nowhere`, 'path_outside');
      let up = path.dirname(abs);
      const rest = [path.basename(abs)];
      while (!fs.existsSync(up)) { rest.unshift(path.basename(up)); up = path.dirname(up); }
      real = path.join(fs.realpathSync(up), ...rest);
    }
    if (!within(real, folder)) throw httpError(403, `${p} leads outside the folder (a link?)`, 'path_outside');
    const parts = path.relative(folder, real).split(path.sep).filter(Boolean);
    for (const part of parts) if (secretName(part)) throw httpError(403, `${part} may hold secrets, so it is never read, written or listed`, 'secret_path');
    if (parts[0] === DRY_DIR) throw httpError(403, `${DRY_DIR}/ is dry's own state`, 'dry_state');
    if (forWrite && parts.includes('.git')) throw httpError(403, 'files inside .git are written only by git', 'dry_state');
    if (!exists && !forWrite) throw httpError(404, `no such file: ${p}`, 'not_found');
    if (dir && exists && !fs.statSync(real).isDirectory()) throw httpError(400, `${p} is not a folder`, 'not_a_folder');
    return { real, rel: parts.join('/'), exists };
  }

  /// The files under a folder of the workspace, paths relative to it: a
  /// folder's own files first, then its folders' (breadth first), at most
  /// `max`. Never `.git`, `.dry`, build folders, secrets, or links that lead out.
  function wsList(dir, max) {
    const base = wsPath(dir, { dir: true });
    const out = [];
    const queue = [''];
    let truncated = false;
    while (queue.length) {
      const rel = queue.shift();
      let entries = [];
      try { entries = fs.readdirSync(path.join(base.real, rel), { withFileTypes: true }); } catch { continue; }
      for (const e of entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))) {
        if (HIDDEN.has(e.name) || e.name === DRY_DIR || secretName(e.name)) continue;
        const r = rel ? `${rel}/${e.name}` : e.name;
        let isDir = e.isDirectory();
        if (e.isSymbolicLink()) {
          try { const real = fs.realpathSync(path.join(base.real, r)); if (!within(real, folder)) continue; isDir = fs.statSync(real).isDirectory(); } catch { continue; }
        }
        if (isDir) queue.push(r);
        else if (out.length >= max) { truncated = true; break; } else out.push(r);
      }
      if (truncated) break;
    }
    return { path: base.rel, files: out, truncated };
  }

  function wsRead(p) {
    const { real, rel } = wsPath(p);
    const st = fs.statSync(real);
    if (st.isDirectory()) throw httpError(400, `${p} is a folder`, 'not_a_file');
    if (st.size > MAX_READ) return { path: rel, kind: 'too_large', bytes: st.size };
    const bytes = fs.readFileSync(real);
    let text = null;
    if (!bytes.includes(0)) try { text = new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { /* binary */ }
    if (text == null) return { path: rel, kind: 'binary', bytes: st.size, image: /\.(png|jpe?g|gif|webp|svg|bmp|ico|avif)$/i.test(real) };
    return { path: rel, kind: 'text', text, bytes: st.size };
  }

  function wsWrite(p, text) {
    if (typeof text !== 'string') throw httpError(400, 'write takes text');
    const { real, rel } = wsPath(p, { forWrite: true });
    if (fs.existsSync(real) && fs.statSync(real).isDirectory()) throw httpError(400, `${p} is a folder`, 'not_a_file');
    fs.mkdirSync(path.dirname(real), { recursive: true });
    const tmp = `${real}.dry-${crypto.randomBytes(4).toString('hex')}.tmp`;
    fs.writeFileSync(tmp, text);
    fs.renameSync(tmp, real);
    return { ok: true, path: rel, bytes: Buffer.byteLength(text) };
  }

  function wsStat(p) {
    const { real, rel, exists } = wsPath(p, { forWrite: true });
    if (!exists) return { path: rel, kind: 'missing' };
    const st = fs.statSync(real);
    return { path: rel, kind: st.isDirectory() ? 'dir' : 'file', size: st.isDirectory() ? 0 : st.size, modified: Math.floor(st.mtimeMs / 1000) };
  }

  function termKey(k) {
    if (typeof k !== 'string' || !KEY_RE.test(k)) throw httpError(400, 'key names a session: letters, digits and ._:/- (at most 200)', 'bad_key');
    return k;
  }

  function termSession(s, lines) {
    const out = { key: s.key, id: s.id, mode: s.mode, status: s.status, exit: s.exit, signal: s.signal, timed_out: s.timedOut, started: s.started, ended: s.ended, command: s.command };
    if (lines != null) out.tail = lastLines(s.output, lines);
    return out;
  }

  /// A terminal call's checks: the switch, the command, its folder.
  function termPrep(b) {
    need('terminal');
    const key = termKey(b.key);
    const command = typeof b.command === 'string' ? b.command : '';
    if (!command.trim()) throw httpError(400, 'a command is needed');
    if (command.includes('\0')) throw httpError(400, 'the command holds a zero byte');
    if (needsConfirm(command)) throw httpError(428, 'push, clone and publishing commands go through the git endpoints with a person\'s click (docs/companion.md)');
    const cwd = wsPath(b.cwd ?? '', { dir: true }).real;
    return { key, command, cwd };
  }

  // ---- switches: each capability on or off, the person's choice in this terminal ----

  function readConfig() {
    try {
      const v = JSON.parse(fs.readFileSync(configFile, 'utf8'));
      return { version: 1, switches: v && typeof v.switches === 'object' && !Array.isArray(v.switches) ? v.switches : {} };
    } catch { return { version: 1, switches: {} }; }
  }
  const config = readConfig();
  for (const id of opts.on ?? []) config.switches[id] = true;
  for (const id of opts.off ?? []) config.switches[id] = false;

  function saveConfig() {
    fs.mkdirSync(path.dirname(configFile), { recursive: true });
    fs.writeFileSync(configFile + '.tmp', JSON.stringify(config, null, 2) + '\n');
    fs.renameSync(configFile + '.tmp', configFile);
  }

  /// An actuator acts on the world (runs commands, flips pins): off until a person switches it on.
  function actuator(id) {
    if (id === 'terminal') return true;
    const d = loadDeclared().declared.find((x) => x.id === id);
    return !!d?.tools.some((t) => t.approval);
  }

  function isOn(id) {
    const v = config.switches[id];
    return typeof v === 'boolean' ? v : !actuator(id);
  }

  function need(id) {
    if (!isOn(id)) throw httpError(403, `${id} is switched off in the companion's terminal (type: on ${id})`, 'capability_off');
  }

  function setSwitch(id, on) {
    config.switches[id] = !!on;
    saveConfig();
    log(`${shortTime()}  ${id} switched ${on ? 'on' : 'off'} (saved in ${configFile})`);
    notify('notifications/tools/list_changed', {});
  }

  // ---- the workspace's own state: <folder>/.dry/state (the store) and .dry/snapshot.json ----

  function snapshotRead() {
    try { return JSON.parse(fs.readFileSync(snapshotFile, 'utf8')); } catch { return null; }
  }

  function snapshotWrite(v) {
    if (!v || typeof v !== 'object' || Array.isArray(v) || v.version !== 1) throw httpError(400, 'a snapshot is {"version": 1, ...} (docs/companion-protocol.md §7)', 'bad_snapshot');
    fs.mkdirSync(dryDir, { recursive: true });
    fs.writeFileSync(snapshotFile + '.tmp', JSON.stringify(v, null, 1) + '\n');
    fs.renameSync(snapshotFile + '.tmp', snapshotFile);
    return { ok: true, path: `${DRY_DIR}/snapshot.json` };
  }

  function workspaceInfo() {
    return { protocol: PROTOCOL, folder, name: path.basename(folder), state: path.relative(folder, storeDir) || storeDir, snapshot: snapshotRead(),
      switches: Object.fromEntries(['file', 'terminal'].map((id) => [id, isOn(id)])) };
  }

  function markersOf(dir) {
    try {
      const names = new Set(fs.readdirSync(dir));
      return MARKERS.filter((m) => names.has(m));
    } catch { return []; }
  }

  /// The project folder `real` is in (itself or a parent, up to its root), or null.
  function projectOf(real) {
    const root = roots.find((r) => within(real, r));
    if (!root) return null;
    let d = real;
    for (;;) {
      if (fs.existsSync(d) && fs.statSync(d).isDirectory() && markersOf(d).length) return d;
      if (d === root) return null;
      d = path.dirname(d);
    }
  }

  function gitBranch(dir) {
    try {
      const head = fs.readFileSync(path.join(dir, '.git', 'HEAD'), 'utf8').trim();
      return head.startsWith('ref: refs/heads/') ? head.slice(16) : head.slice(0, 12);
    } catch { return null; }
  }

  function projects() {
    const out = [];
    const look = (dir, depth, root) => {
      const markers = markersOf(dir);
      if (markers.length && dir !== root) {
        out.push({ name: path.basename(dir), path: dir, root, markers, git: markers.includes('.git'), branch: gitBranch(dir) });
        return;
      }
      if (markers.length && dir === root) {
        out.push({ name: path.basename(dir), path: dir, root, markers, git: markers.includes('.git'), branch: gitBranch(dir) });
      }
      if (depth >= 2) return;
      let entries = [];
      try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
      for (const e of entries.sort((a, b) => a.name.localeCompare(b.name))) {
        if (!e.isDirectory() || e.name.startsWith('.') || HIDDEN.has(e.name)) continue;
        look(path.join(dir, e.name), depth + 1, root);
      }
    };
    for (const r of roots) look(r, 0, r);
    return out;
  }

  // ---- runtimes ----

  function onPath(bin) {
    for (const dir of (process.env.PATH || '').split(path.delimiter)) {
      if (!dir) continue;
      const full = path.join(dir, bin);
      try { fs.accessSync(full, fs.constants.X_OK); if (fs.statSync(full).isFile()) return full; } catch { /* next */ }
    }
    return null;
  }

  function versionOf(bin, args) {
    return new Promise((resolve) => {
      execFile(bin, args, { timeout: 8000, env: { ...process.env, NO_COLOR: '1' } }, (err, stdout, stderr) => {
        const text = `${stdout || ''}\n${stderr || ''}`.split('\n').map((l) => l.trim()).find((l) => l) || '';
        resolve(err && !text ? null : text.slice(0, 120));
      });
    });
  }

  async function detectRuntimes() {
    const found = {};
    await Promise.all(Object.entries(RUNTIMES).map(async ([name, args]) => {
      const where = onPath(name);
      found[name] = where ? { path: where, version: await versionOf(where, args) } : null;
    }));
    runtimes = found;
    return found;
  }

  // ---- exec sessions ----

  function cleanEnv() {
    const env = {};
    for (const [k, v] of Object.entries(process.env)) if (!secretEnv(k)) env[k] = v;
    return env;
  }

  function publicSession(s, tail) {
    const out = { id: s.id, key: s.key, cwd: s.cwd, command: s.command, mode: s.mode, status: s.status, exit: s.exit, signal: s.signal, timed_out: s.timedOut, started: s.started, ended: s.ended };
    if (tail != null) out.tail = lastLines(s.output, tail);
    return out;
  }

  function broadcast(event, data, filter) {
    const msg = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
    for (const l of listeners) if (!filter || filter(l)) l.res.write(msg);
  }

  function launch({ cwd, command, mode, key, timeoutMs }) {
    const id = String(nextId++);
    const s = { id, key: key || null, cwd, command, mode, status: 'running', exit: null, signal: null, timedOut: false, started: Math.floor(Date.now() / 1000), ended: null, output: '', child: null, done: null };
    if (s.key) for (const [oid, o] of sessions) if (o.key === s.key && o.status !== 'running') sessions.delete(oid);
    sessions.set(id, s);
    // Keep at most 200 sessions: the oldest finished go first.
    if (sessions.size > 200) for (const [oid, o] of sessions) { if (o.status !== 'running') { sessions.delete(oid); if (sessions.size <= 200) break; } }
    const win = process.platform === 'win32';
    const child = spawn(win ? (process.env.ComSpec || 'cmd.exe') : '/bin/sh', win ? ['/d', '/s', '/c', command] : ['-c', command], { cwd, env: cleanEnv(), detached: !win, windowsVerbatimArguments: win, stdio: ['ignore', 'pipe', 'pipe'] });
    s.child = child;
    const take = (buf) => {
      const text = buf.toString('utf8');
      s.output += text;
      if (s.output.length > MAX_OUTPUT) s.output = s.output.slice(s.output.length - MAX_OUTPUT);
      broadcast('output', { id, key: s.key, text }, (l) => l.id === id);
    };
    child.stdout.on('data', take);
    child.stderr.on('data', take);
    let timer = null;
    if (timeoutMs) {
      timer = setTimeout(() => { s.timedOut = true; kill(s); }, timeoutMs);
    }
    s.done = new Promise((resolve) => {
      const finish = (code, signal, why) => {
        if (s.status !== 'running') return;
        if (timer) clearTimeout(timer);
        if (why) s.output += `\n${why}\n`;
        s.status = 'exited';
        s.exit = code;
        s.signal = signal;
        s.ended = Math.floor(Date.now() / 1000);
        s.child = null;
        log(`${shortTime()}  exec ${id} ${mode} ended ${code ?? signal}${s.timedOut ? ' (timed out)' : ''}: ${command.slice(0, 60)}`);
        // A start's exit is a callback: every stream hears it. A run's
        // (its caller is waiting) and a background's only by id or ?all=1.
        emit('exit', publicSession(s), mode === 'start');
        resolve(s);
      };
      child.on('error', (e) => finish(null, null, `could not start /bin/sh: ${e.message}`));
      child.on('close', (code, signal) => finish(code, signal));
    });
    return s;
  }

  function kill(s) {
    if (!s.child) return;
    const c = s.child;
    const sig = (signal) => { try { if (process.platform === 'win32') c.kill(signal); else process.kill(-c.pid, signal); } catch { /* gone */ } };
    sig('SIGTERM');
    setTimeout(() => { if (s.child === c) sig('SIGKILL'); }, 2000);
  }

  function findSession(q) {
    if (q.id && sessions.has(String(q.id))) return sessions.get(String(q.id));
    if (q.key) { let found = null; for (const s of sessions.values()) if (s.key === q.key) found = s; return found; }
    return null;
  }

  function execCwd(cwd) {
    const real = confine(cwd);
    if (!fs.statSync(real).isDirectory()) throw httpError(400, `${cwd} is not a folder`);
    if (!projectOf(real)) throw httpError(403, `${cwd} is not inside a project (a folder with .git or a manifest)`);
    return real;
  }

  // ---- git ----

  function git(args, cwd, timeout = 120000) {
    return new Promise((resolve) => {
      execFile('git', args, { cwd, timeout, maxBuffer: 16 * 1024 * 1024, env: { ...process.env, GIT_TERMINAL_PROMPT: '0', LC_ALL: 'C' } }, (err, stdout, stderr) => {
        resolve({ ok: !err, code: err ? (typeof err.code === 'number' ? err.code : null) : 0, stdout: stdout || '', stderr: stderr || '', args });
      });
    });
  }

  function gitProject(p) {
    const real = confine(p);
    const proj = projectOf(real);
    if (!proj || !fs.existsSync(path.join(proj, '.git'))) throw httpError(400, `${p} is not inside a git project`);
    return proj;
  }

  function parseStatus(text) {
    const lines = text.split('\n').filter(Boolean);
    const head = lines[0]?.startsWith('## ') ? lines.shift().slice(3) : '';
    const m = head.match(/^(.+?)(?:\.\.\.(\S+))?(?: \[(.+)\])?$/) || [];
    const ahead = /ahead (\d+)/.exec(m[3] || '')?.[1];
    const behind = /behind (\d+)/.exec(m[3] || '')?.[1];
    return {
      branch: m[1] || null, upstream: m[2] || null, ahead: Number(ahead || 0), behind: Number(behind || 0),
      files: lines.map((l) => ({ xy: l.slice(0, 2), path: l.slice(3) })),
    };
  }

  async function gitCall(op, b) {
    const needConfirm = op === 'push' || op === 'clone';
    if (needConfirm && b.confirmed !== true) {
      throw httpError(428, `${op} needs a person's confirmation: the page sends confirmed=true only from a click (docs/companion.md)`);
    }
    if (op === 'clone') {
      if (!cloneUrlOk(b.url)) throw httpError(400, 'clone takes an https://, ssh://, git@host:path or file:/// URL');
      const into = confine(b.root || roots[0]);
      if (!fs.statSync(into).isDirectory()) throw httpError(400, 'clone goes into a folder under a root');
      const name = b.name || path.basename(b.url.replace(/\/$/, '')).replace(/\.git$/, '');
      if (!/^[A-Za-z0-9_][A-Za-z0-9._-]{0,99}$/.test(name)) throw httpError(400, `not a folder name: ${name}`);
      const dest = path.join(into, name);
      if (fs.existsSync(dest)) throw httpError(409, `${dest} is already there`);
      const r = await git(['clone', '--', b.url, dest], into, 600000);
      return { ...r, path: dest };
    }
    const cwd = gitProject(b.path);
    switch (op) {
      case 'status': {
        const r = await git(['status', '--porcelain=v1', '-b'], cwd);
        return { ...r, project: cwd, ...parseStatus(r.stdout) };
      }
      case 'diff': {
        const args = ['diff', '--no-color', '--no-ext-diff'];
        if (b.staged) args.push('--cached');
        if (b.stat) args.push('--stat');
        if (b.file) args.push('--', path.relative(cwd, confine(path.join(cwd, b.file), { inside: cwd })));
        const r = await git(args, cwd);
        if (r.stdout.length > 512 * 1024) r.stdout = r.stdout.slice(0, 512 * 1024) + '\n… (cut at 512 KB)';
        return r;
      }
      case 'branch': {
        if (!b.name) {
          const r = await git(['branch', '--format=%(refname:short)'], cwd);
          return { ...r, branches: r.stdout.split('\n').filter(Boolean), current: gitBranch(cwd) };
        }
        if (!refOk(b.name)) throw httpError(400, `not a branch name: ${b.name}`);
        return git(b.create ? ['switch', '-c', b.name] : ['switch', b.name], cwd);
      }
      case 'commit': {
        if (typeof b.message !== 'string' || !b.message.trim()) throw httpError(400, 'a commit needs a message');
        if (b.all !== false) {
          const add = await git(['add', '-A'], cwd);
          if (!add.ok) return add;
        }
        const r = await git(['commit', '-m', b.message], cwd);
        const head = await git(['rev-parse', '--short', 'HEAD'], cwd);
        return { ...r, head: head.stdout.trim() };
      }
      case 'push': {
        const remote = b.remote || 'origin';
        const branch = b.branch || gitBranch(cwd);
        if (!refOk(remote) || !refOk(branch)) throw httpError(400, 'push takes a remote and a branch name');
        const url = (await git(['remote', 'get-url', remote], cwd)).stdout.trim();
        const r = await git(['push', ...(b.setUpstream === false ? [] : ['-u']), remote, branch], cwd, 600000);
        return { ...r, remote, url, branch };
      }
      case 'pull': {
        const args = ['pull', '--ff-only'];
        if (b.remote) { if (!refOk(b.remote)) throw httpError(400, 'not a remote name'); args.push(b.remote); }
        if (b.branch) { if (!refOk(b.branch)) throw httpError(400, 'not a branch name'); args.push(b.branch); }
        return git(args, cwd, 600000);
      }
      default:
        throw httpError(404, `no git operation ${op}`);
    }
  }

  // ---- providers: declared in providers.json, never scanned (docs/companion-protocol.md §8) ----

  async function getJson(url, ms = 3000) {
    const r = await fetch(url, { signal: AbortSignal.timeout(ms) });
    if (!r.ok) throw new Error(`${url} answered ${r.status}`);
    return r.json();
  }

  const providerStates = new Map(); // name -> { entry, state, why, hint, models, url, bin, version, child, at }
  let nvidia = null;

  /// The template a new providers.json starts from: the repo's (beside
  /// models.json, next to this script's folder), else the built-in one.
  function providersTemplate() {
    try { return JSON.parse(fs.readFileSync(path.join(HERE, '..', 'providers.json'), 'utf8')); } catch { return PROVIDERS_TEMPLATE; }
  }

  /// What providers.json declares (and --lmstudio / --ollama, as ports).
  function declaredProviders() {
    let text = null;
    try { text = fs.readFileSync(providersFile, 'utf8'); } catch { /* none yet */ }
    const { providers, errors } = text == null ? { providers: [], errors: [] } : parseProviders(text);
    for (const [name, url] of [['lmstudio', lmstudio], ['ollama', ollama]]) {
      if (url && !providers.some((p) => p.name === name)) providers.push(checkProvider({ name, kind: 'port', url }));
    }
    return { providers, errors, exists: text != null };
  }

  function writeProviders(v) {
    fs.mkdirSync(path.dirname(providersFile), { recursive: true });
    fs.writeFileSync(providersFile + '.tmp', JSON.stringify(v, null, 2) + '\n');
    fs.renameSync(providersFile + '.tmp', providersFile);
  }

  /// A new entry into providers.json (from the page's form): checked, a name not taken.
  async function addProvider(entry) {
    let p;
    try { p = checkProvider(entry); } catch (e) { throw httpError(400, e.message, 'bad_provider'); }
    let v;
    try { v = JSON.parse(fs.readFileSync(providersFile, 'utf8')); } catch { v = providersTemplate(); }
    if (!Array.isArray(v.providers)) v.providers = [];
    if (v.providers.some((x) => x?.name === p.name) || declaredProviders().providers.some((x) => x.name === p.name)) throw httpError(409, `a provider named ${p.name} is already declared`, 'exists');
    v.providers.push(p);
    writeProviders(v);
    log(`${shortTime()}  provider ${p.name} (${p.kind}) added to ${providersFile}`);
    const r = await probeProvider(p);
    notify('notifications/tools/list_changed', {});
    return publicProvider(r);
  }

  function hfCached(id) {
    const hub = process.env.HF_HUB_CACHE || path.join(process.env.HF_HOME || path.join(os.homedir(), '.cache', 'huggingface'), 'hub');
    return fs.existsSync(path.join(hub, `models--${id.replace(/\//g, '--')}`));
  }

  function freePort() {
    return new Promise((resolve, reject) => {
      const srv = http.createServer();
      srv.once('error', reject);
      srv.listen(0, '127.0.0.1', () => { const p = srv.address().port; srv.close(() => resolve(p)); });
    });
  }

  async function modelsAt(url, ms = 3000) {
    const v = await getJson(`${url}/v1/models`, ms);
    return (v.data || []).map((m) => m.id).filter((x) => typeof x === 'string');
  }

  /// One provider probed: ok (its models), failed (why), or absent (the
  /// program is not on this machine: the hint says how to get it).
  async function probeProvider(entry) {
    const had = providerStates.get(entry.name);
    const r = { entry, state: 'failed', why: '', hint: '', models: [], url: null, bin: null, version: null, child: had?.child ?? null, at: Date.now() };
    if (entry.kind === 'cli') {
      const bin = entry.path ? home(entry.path) : onPath(entry.cli);
      if (!bin || !fs.existsSync(bin)) {
        Object.assign(r, { state: 'absent', why: entry.path ? `${entry.path}: no such program` : `${entry.cli} is not on PATH`, hint: entry.path ? `fix "path", or ${INSTALL[entry.cli]}` : INSTALL[entry.cli] });
      } else {
        const version = await versionOf(bin, ['--version']);
        if (version == null) Object.assign(r, { state: 'failed', why: `${bin} --version did not answer`, bin });
        else Object.assign(r, { state: 'ok', bin, version, models: entry.model ? [entry.model] : { claude: ['sonnet', 'opus', 'haiku'], codex: ['default'], copilot: ['auto'] }[entry.cli] });
      }
    } else if (entry.kind === 'port') {
      try { Object.assign(r, { state: 'ok', url: entry.url, models: await modelsAt(entry.url) }); } catch (e) {
        Object.assign(r, { state: 'failed', url: entry.url, why: `GET ${entry.url}/v1/models: ${e.cause?.code || e.message}`, hint: 'is its server running, on that port?' });
      }
    } else {
      if (nvidia === null) nvidia = !!onPath('nvidia-smi');
      const { runner, accel } = chooseRunner(entry.runner, { nvidia });
      r.runner = runner;
      r.accel = accel;
      const bin = entry.path ? home(entry.path) : onPath(RUNNER_BIN[runner]);
      const local = fs.existsSync(home(entry.model));
      if (!bin || !fs.existsSync(bin)) {
        Object.assign(r, { state: 'absent', why: `${entry.path || RUNNER_BIN[runner]} (${runner}, ${accel}) is not on this machine`, hint: INSTALL[runner] });
      } else if (!local && !(runner === 'mlx' && hfCached(entry.model))) {
        Object.assign(r, { state: 'failed', bin, why: `the model ${entry.model} is not on this machine (the companion never downloads)`,
          hint: runner === 'mlx' ? `download it yourself: huggingface-cli download ${entry.model}` : 'give "model" the path of a .gguf file on this machine' });
      } else if (had?.child && had.url) {
        try { Object.assign(r, { state: 'ok', bin, url: had.url, models: await modelsAt(had.url) }); } catch (e) { Object.assign(r, { state: 'starting', bin, url: had.url, why: 'the runner is starting' }); }
      } else {
        Object.assign(r, await startRunner(entry, runner, bin));
      }
    }
    providerStates.set(entry.name, r);
    return r;
  }

  /// A native runner started on a local port, then registered as a port
  /// (its /v1/models answering is what makes it ok).
  async function startRunner(entry, runner, bin) {
    const port = entry.port ?? await freePort();
    const argv = runnerArgv(runner, bin, home(entry.model), port);
    const url = `http://127.0.0.1:${port}`;
    const child = spawn(argv[0], argv.slice(1), { env: cleanEnv(), stdio: ['ignore', 'ignore', 'pipe'], detached: process.platform !== 'win32' });
    let err = '';
    child.stderr.on('data', (d) => { err = (err + d).slice(-2000); });
    log(`${shortTime()}  provider ${entry.name}: started ${path.basename(bin)} (${runner}) on ${url}`);
    const exited = new Promise((resolve) => child.on('exit', (c) => resolve(c)));
    child.on('exit', () => {
      const st = providerStates.get(entry.name);
      if (st?.child === child) { st.child = null; st.state = 'failed'; st.why = `the runner exited: ${err.trim().split('\n').slice(-1)[0] || 'no output'}`; }
    });
    const t0 = Date.now();
    while (Date.now() - t0 < (opts.runnerWaitMs ?? 120000)) {
      const gone = await Promise.race([exited, new Promise((r) => setTimeout(() => r(undefined), 300))]);
      if (gone !== undefined) return { state: 'failed', bin, url, why: `the runner exited (${gone}): ${err.trim().split('\n').slice(-1)[0] || 'no output'}`, child: null };
      try { return { state: 'ok', bin, url, models: await modelsAt(url, 1000), child }; } catch { /* not yet */ }
    }
    return { state: 'starting', bin, url, why: 'the runner is still loading its model', child };
  }

  async function probeProviders() {
    const { providers } = declaredProviders();
    const r = await Promise.all(providers.map((p) => probeProvider(p).catch((e) => ({ entry: p, state: 'failed', why: e.message, models: [] }))));
    for (const name of [...providerStates.keys()]) if (!providers.some((p) => p.name === name)) { stopRunner(providerStates.get(name)); providerStates.delete(name); }
    return r;
  }

  function stopRunner(st) {
    if (!st?.child) return;
    try { if (process.platform === 'win32') st.child.kill(); else process.kill(-st.child.pid, 'SIGTERM'); } catch { /* gone */ }
    st.child = null;
  }

  function publicProvider(r) {
    return { name: r.entry.name, kind: r.entry.kind, entry: r.entry, state: r.state, why: r.why || '', hint: r.hint || '', models: r.models.map((m) => `${r.entry.name}/${m}`),
      ...(r.url ? { url: r.url } : {}), ...(r.runner ? { runner: r.runner, accel: r.accel } : {}), ...(r.version ? { version: r.version } : {}) };
  }

  async function providersNow(refresh) {
    const { providers, errors, exists } = declaredProviders();
    if (refresh || providers.some((p) => !providerStates.has(p.name))) await probeProviders();
    return { file: providersFile, exists, template: providersTemplate(), errors, providers: providers.map((p) => publicProvider(providerStates.get(p.name) ?? { entry: p, state: 'failed', why: 'not probed', models: [] })) };
  }

  /// Every model the declared providers serve, as `<provider>/<model>`.
  async function allModels() {
    const { providers } = await providersNow(false);
    return providers.filter((p) => p.state === 'ok').flatMap((p) => p.models.map((id) => ({ id, object: 'model', owned_by: p.name, provider: p.name, kind: p.kind })));
  }

  function textOf(content) {
    if (typeof content === 'string') return content;
    if (Array.isArray(content)) return content.map((p) => (typeof p === 'string' ? p : p?.text ?? '')).join('');
    return '';
  }

  /// A chat as one prompt for a CLI that takes one: the system text apart,
  /// the turns as a transcript (one user message stays as it is).
  function flatten(messages) {
    const system = messages.filter((m) => m.role === 'system' || m.role === 'developer').map((m) => textOf(m.content)).join('\n\n');
    const turns = messages.filter((m) => m.role !== 'system' && m.role !== 'developer');
    if (turns.length === 1 && turns[0].role === 'user') return { system, prompt: textOf(turns[0].content) };
    const prompt = turns.map((m) => `${m.role === 'assistant' ? 'Assistant' : m.role === 'tool' ? 'Tool' : 'User'}: ${textOf(m.content)}`).join('\n\n')
      + '\n\nReply as the Assistant to the last message.';
    return { system, prompt };
  }

  function runCli(bin, args, input, ms) {
    return new Promise((resolve, reject) => {
      const child = spawn(bin, args, { cwd: os.tmpdir(), env: { ...process.env, NO_COLOR: '1' }, stdio: ['pipe', 'pipe', 'pipe'] });
      let out = '', err = '';
      const t = setTimeout(() => { child.kill('SIGKILL'); reject(new Error(`${path.basename(bin)} took longer than ${ms / 1000} s`)); }, ms);
      child.stdout.on('data', (d) => { out += d; });
      child.stderr.on('data', (d) => { err += d; });
      child.on('error', (e) => { clearTimeout(t); reject(e); });
      child.on('close', (code) => { clearTimeout(t); resolve({ code, out, err }); });
      if (input != null) child.stdin.end(input); else child.stdin.end();
    });
  }

  function completion(model, content, usage) {
    return {
      id: `chatcmpl-${crypto.randomBytes(8).toString('hex')}`, object: 'chat.completion', created: Math.floor(Date.now() / 1000), model,
      choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }],
      usage: usage || { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
    };
  }

  /// A chat for a cli provider: the CLI with argument arrays, no tools, in a temporary folder.
  async function cliChat(st, id, model, body) {
    const { system, prompt } = flatten(body.messages || []);
    const bin = st.bin;
    if (st.entry.cli === 'claude') {
      const args = ['-p', '--output-format', 'json', '--no-session-persistence', '--tools', '', '--strict-mcp-config'];
      if (model && model !== 'default') args.push('--model', model);
      if (system) args.push('--system-prompt', system);
      const r = await runCli(bin, args, prompt, 10 * 60 * 1000);
      let v = null;
      try { v = JSON.parse(r.out); } catch { /* below */ }
      if (!v || v.is_error || r.code !== 0) throw httpError(502, `claude -p failed (exit ${r.code}): ${(v?.result || r.err || r.out).slice(0, 400)}`);
      const u = v.usage || {};
      const pin = (u.input_tokens || 0) + (u.cache_read_input_tokens || 0) + (u.cache_creation_input_tokens || 0);
      return completion(id, v.result ?? '', { prompt_tokens: pin, completion_tokens: u.output_tokens || 0, total_tokens: pin + (u.output_tokens || 0) });
    }
    if (st.entry.cli === 'codex') {
      const outFile = path.join(os.tmpdir(), `dry-codex-${crypto.randomBytes(6).toString('hex')}.txt`);
      const args = ['exec', '--color', 'never', '--sandbox', 'read-only', '--skip-git-repo-check', '--ephemeral', '-o', outFile];
      if (model && model !== 'default') args.push('--model', model);
      args.push('-');
      const r = await runCli(bin, args, system ? `${system}\n\n${prompt}` : prompt, 10 * 60 * 1000);
      let text = '';
      try { text = fs.readFileSync(outFile, 'utf8'); fs.rmSync(outFile, { force: true }); } catch { /* none */ }
      if (r.code !== 0 && !text) throw httpError(502, `codex exec failed (exit ${r.code}): ${(r.err || r.out).slice(-400)}`);
      return completion(id, text.trim());
    }
    const args = ['-p', system ? `${system}\n\n${prompt}` : prompt, '-s', '--no-color', '--available-tools', ''];
    if (model && model !== 'auto') args.push('--model', model);
    const r = await runCli(bin, args, null, 10 * 60 * 1000);
    if (r.code !== 0) throw httpError(502, `copilot -p failed (exit ${r.code}): ${(r.err || r.out).slice(0, 400)}`);
    return completion(id, r.out.trim());
  }

  async function passthrough(base, model, body, res, origin) {
    let up;
    try {
      up = await fetch(`${base}/v1/chat/completions`, {
        method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ...body, model }),
      });
    } catch (e) {
      throw httpError(502, `could not reach ${base} (${e.cause?.code || e.message}): is the model server running?`);
    }
    res.writeHead(up.status, { ...cors(origin), 'content-type': up.headers.get('content-type') || 'application/json' });
    if (up.body) for await (const chunk of up.body) res.write(chunk);
    res.end();
    return up.status;
  }

  // ---- the store ----

  async function storeRead() {
    const texts = {};
    const walk = async (dir, rel) => {
      let entries = [];
      try { entries = await fsp.readdir(dir, { withFileTypes: true }); } catch { return; }
      for (const e of entries) {
        if (e.name.endsWith('.tmp')) continue;
        const r = rel ? `${rel}/${e.name}` : e.name;
        if (e.isDirectory()) await walk(path.join(dir, e.name), r);
        else if (e.isFile()) texts[r] = await fsp.readFile(path.join(dir, e.name), 'utf8');
      }
    };
    await walk(storeDir, '');
    return texts;
  }

  async function storeWrite(put = {}, remove = []) {
    for (const p of [...Object.keys(put), ...remove]) if (!storePathOk(p)) throw httpError(400, `not a store path: ${p}`);
    for (const [p, text] of Object.entries(put)) {
      const full = path.join(storeDir, ...p.split('/'));
      await fsp.mkdir(path.dirname(full), { recursive: true });
      await fsp.writeFile(full + '.tmp', String(text));
      await fsp.rename(full + '.tmp', full);
    }
    for (const p of remove) await fsp.rm(path.join(storeDir, ...p.split('/')), { force: true });
  }

  // ---- pairings ----

  function sec(ms) {
    return Math.floor(ms / 1000);
  }

  /// The pairing a token is, while it has not expired (and when it was last used).
  function pairingOf(tok) {
    const p = tok ? pairings.get(tok) : null;
    if (!p) return null;
    if (Date.now() >= p.expires) { pairings.delete(tok); log(`${shortTime()}  pairing ${p.id} expired`); return null; }
    p.seen = Date.now();
    return p;
  }

  function listPairings(current) {
    return [...pairings.entries()].filter(([, p]) => p.expires > Date.now()).map(([t, p]) => ({
      id: p.id, origin: p.origin, created: sec(p.created), expires: sec(p.expires), seen: sec(p.seen), current: t === current,
      ...(p.hello ? { hello: p.hello } : {}), streams: p.streams || 0,
    }));
  }

  function revoke(id) {
    for (const [t, p] of pairings) if (p.id === id) { pairings.delete(t); closeStreams(p); log(`${shortTime()}  pairing ${id} revoked`); return true; }
    return false;
  }

  /// A pairing that ended: its open streams end too (the peer sees it leave).
  function closeStreams(pairing) {
    for (const l of [...listeners]) if (l.pairing === pairing) { listeners.delete(l); try { l.res.end(); } catch { /* gone */ } }
    for (const r of [...mcpStreams]) if (r.pairing === pairing) { mcpStreams.delete(r); try { r.end(); } catch { /* gone */ } }
  }

  function pairingsText() {
    const list = listPairings(null);
    if (!list.length) return '  no pairings';
    const hello = (id) => { for (const p of pairings.values()) if (p.id === id && p.hello) return ` [${p.hello.runtime || '?'} ${p.hello.platform || ''}${p.streams ? ', listening' : ''}]`; return ''; };
    return list.map((p) => `  ${p.id}${hello(p.id)}  ${p.origin || '(no origin)'}  since ${new Date(p.created * 1000).toTimeString().slice(0, 8)}  until ${new Date(p.expires * 1000).toTimeString().slice(0, 8)}`).join('\n')
      + '\n  (type: revoke <id>)';
  }

  // ---- the event log: exits and callbacks, with ids a page resumes from ----

  /// Record an event and send it to the streams that want it. `all`: every
  /// stream hears it (a start's exit, the callback); otherwise only a
  /// stream for that session or one opened with ?all=1.
  function emit(event, data, all) {
    const e = { id: nextEvent++, event, data, all, at: sec(Date.now()) };
    events.push(e);
    if (events.length > MAX_EVENTS) events.shift();
    for (const l of listeners) if (wants(l, e)) sse(l.res, e);
    return e;
  }

  function wants(l, e) {
    return e.all || l.all || (l.id && l.id === e.data?.id);
  }

  function sse(res, e) {
    res.write(`id: ${e.id}\nevent: ${e.event}\ndata: ${JSON.stringify(e.data)}\n\n`);
  }

  // ---- custom tools: tools/*.json and tools/*.md, from each project and --tools ----

  /// One tool file as its definition: name, description, inputs and the
  /// command (an argv array; an input goes in only as a whole argument,
  /// `{name}`, and always as the environment variable DRY_INPUT_<NAME>).
  function parseTool(file, text) {
    let t;
    if (file.endsWith('.json')) {
      try { t = JSON.parse(text); } catch (e) { throw new Error(`not JSON: ${e.message}`); }
      if (Array.isArray(t.inputs)) {
        t.inputs = t.inputs.map((i) => ({ name: i.name, type: i.type || 'string', required: !!i.required, description: i.description || '' }));
      } else if (t.inputs && typeof t.inputs === 'object') {
        t.inputs = Object.entries(t.inputs).map(([name, i]) => ({ name, type: i.type || 'string', required: !!i.required, description: i.description || '' }));
      } else t.inputs = [];
    } else {
      t = { inputs: [], description: '' };
      const lines = text.split('\n');
      let inInputs = false;
      const desc = [];
      for (const raw of lines) {
        const line = raw.trimEnd();
        const head = /^#\s+(\S+)\s*$/.exec(line);
        if (head && !t.name) { t.name = head[1]; continue; }
        const kv = /^(command|timeout|name|description):\s*(.*)$/.exec(line);
        if (kv) {
          inInputs = false;
          const [, k, v] = kv;
          if (k === 'command') t.command = v.trim().startsWith('[') ? JSON.parse(v) : v.trim().split(/\s+/).filter(Boolean);
          else if (k === 'timeout') t.timeout = v.trim();
          else t[k] = v.trim();
          continue;
        }
        if (/^inputs:\s*$/.test(line)) { inInputs = true; continue; }
        const input = /^\s*-\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:\(([^)]*)\))?\s*:?\s*(.*)$/.exec(line);
        if (inInputs && input) {
          const flags = (input[2] || '').split(',').map((x) => x.trim());
          t.inputs.push({ name: input[1], type: flags.find((f) => TYPES.includes(f)) || 'string', required: flags.includes('required'), description: input[3].trim() });
          continue;
        }
        if (!inInputs && !t.command && line.trim()) desc.push(line.trim());
      }
      if (!t.description) t.description = desc.join(' ');
    }
    const name = t.name || path.basename(file).replace(/\.(json|md)$/, '');
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(name)) throw new Error(`not a tool name: ${name} (letters, digits, - and _)`);
    if (!Array.isArray(t.command) || !t.command.length || !t.command.every((x) => typeof x === 'string' && x)) throw new Error('command must be a list of arguments, the program first');
    const names = new Set();
    for (const i of t.inputs) {
      if (!/^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(i.name || '')) throw new Error(`not an input name: ${i.name}`);
      if (!TYPES.includes(i.type)) throw new Error(`input ${i.name}: type is one of ${TYPES.join(', ')}`);
      names.add(i.name);
    }
    t.command.forEach((arg, n) => {
      const whole = /^\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(arg);
      if (whole) {
        if (!names.has(whole[1])) throw new Error(`{${whole[1]}} is not one of its inputs`);
        if (n === 0) throw new Error('the program itself cannot be an input');
        if (CODE_FLAGS.includes(t.command[n - 1])) throw new Error(`an input cannot follow ${t.command[n - 1]}: it would run as code`);
      } else if (/\{[A-Za-z_][A-Za-z0-9_]*\}/.test(arg) && [...arg.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)].some((m) => names.has(m[1]))) {
        throw new Error(`"${arg}": an input goes in as a whole argument ({name}), never inside one`);
      }
    });
    const timeoutMs = Math.min(parseDuration(t.timeout) ?? 60000, MAX_TIMEOUT);
    return { name, description: String(t.description || ''), inputs: t.inputs, command: t.command, timeoutMs };
  }

  function parseDuration(v) {
    if (v == null || v === '') return null;
    if (typeof v === 'number') return v;
    const m = /^(\d+)\s*(ms|s|m)?$/.exec(String(v).trim());
    if (!m) return null;
    return Number(m[1]) * ({ ms: 1, s: 1000, m: 60000 }[m[2] || 's']);
  }

  /// Every tool: the --tools folder's by their names, a project's as `<project>.<name>`.
  function loadTools() {
    const tools = [], errors = [];
    const from = (dir, prefix, cwd) => {
      let files = [];
      try { files = fs.readdirSync(dir).filter((f) => /\.(json|md)$/.test(f)).sort(); } catch { return; }
      for (const f of files) {
        const full = path.join(dir, f);
        try {
          const t = parseTool(f, fs.readFileSync(full, 'utf8'));
          t.name = prefix + t.name;
          t.cwd = cwd;
          t.source = full;
          if (tools.some((x) => x.name === t.name)) throw new Error(`a second tool named ${t.name}`);
          tools.push(t);
        } catch (e) { errors.push({ file: full, error: e.message }); }
      }
    };
    if (toolsDir) from(toolsDir, '', toolsDir);
    for (const p of projects()) from(path.join(p.path, 'tools'), `${p.name}.`, p.path);
    return { tools, errors };
  }

  function schemaOf(t) {
    const properties = {};
    for (const i of t.inputs) properties[i.name] = { type: i.type, ...(i.description ? { description: i.description } : {}) };
    return { type: 'object', properties, required: t.inputs.filter((i) => i.required).map((i) => i.name) };
  }

  function publicTool(t) {
    return { name: t.name, description: t.description, inputSchema: schemaOf(t), command: t.command, source: t.source };
  }

  /// An input's value as the one argument it becomes, or why not.
  function inputValue(i, v) {
    let s;
    switch (i.type) {
      case 'integer': s = String(v); if (!/^-?\d{1,15}$/.test(s)) throw httpError(400, `${i.name} must be a whole number`); break;
      case 'number': s = String(v); if (!/^-?\d+(\.\d+)?([eE][-+]?\d+)?$/.test(s)) throw httpError(400, `${i.name} must be a number`); break;
      case 'boolean': s = String(v); if (s !== 'true' && s !== 'false') throw httpError(400, `${i.name} must be true or false`); break;
      default:
        if (typeof v === 'object' && v !== null) throw httpError(400, `${i.name} must be text`);
        s = String(v);
    }
    if (s.includes('\0')) throw httpError(400, `${i.name} holds a zero byte`);
    if (s.length > 65536) throw httpError(400, `${i.name} is longer than 64 KB`);
    return s;
  }

  /// Run a tool: its command with the inputs as whole arguments and as
  /// environment variables; never through a shell.
  async function callTool(name, args) {
    const t = loadTools().tools.find((x) => x.name === name);
    if (!t) throw httpError(404, `no tool ${name}`);
    if (args == null) args = {};
    if (typeof args !== 'object' || Array.isArray(args)) throw httpError(400, 'arguments is an object');
    for (const k of Object.keys(args)) if (!t.inputs.some((i) => i.name === k)) throw httpError(400, `${name} has no input ${k}`);
    const values = {};
    for (const i of t.inputs) {
      const v = args[i.name];
      if (v == null || v === '') { if (i.required) throw httpError(400, `${name} needs ${i.name}`); continue; }
      values[i.name] = inputValue(i, v);
    }
    const argv = t.command.flatMap((arg) => {
      const m = /^\{([A-Za-z_][A-Za-z0-9_]*)\}$/.exec(arg);
      return m ? (m[1] in values ? [values[m[1]]] : []) : [arg];
    });
    const env = cleanEnv();
    env.DRY_TOOL = t.name;
    for (const [k, v] of Object.entries(values)) env[`DRY_INPUT_${k.toUpperCase()}`] = v;
    let bin = argv[0];
    if (bin.includes('/') && !path.isAbsolute(bin)) bin = path.resolve(t.cwd, bin);
    const t0 = Date.now();
    return new Promise((resolve) => {
      let out = '', timedOut = false;
      const child = spawn(bin, argv.slice(1), { cwd: t.cwd, env, shell: false, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
      const take = (d) => { out += d; if (out.length > MAX_OUTPUT) out = out.slice(out.length - MAX_OUTPUT); };
      child.stdout.on('data', take);
      child.stderr.on('data', take);
      const timer = setTimeout(() => { timedOut = true; try { process.kill(-child.pid, 'SIGKILL'); } catch { /* gone */ } }, t.timeoutMs);
      child.on('error', (e) => { clearTimeout(timer); resolve({ ok: false, exit: null, output: `could not start ${argv[0]}: ${e.message}`, ms: Date.now() - t0 }); });
      child.on('close', (code, signal) => {
        clearTimeout(timer);
        if (timedOut) out += `\n(stopped: it ran past its ${t.timeoutMs / 1000} s)`;
        resolve({ ok: code === 0 && !timedOut, exit: code, signal, timed_out: timedOut, output: out.slice(-64 * 1024), ms: Date.now() - t0 });
      });
    });
  }

  // ---- the browser: this machine's Chrome, headless, over the DevTools protocol ----

  /// Only this machine's pages, and the prefixes given with --allow-url.
  function urlAllowed(u) {
    let url;
    try { url = new URL(u); } catch { return false; }
    if (u === 'about:blank') return true;
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
    const h = url.hostname.toLowerCase();
    if (h === 'localhost' || h === '127.0.0.1' || h === '[::1]' || h === '::1' || h.endsWith('.localhost')) return true;
    return allowUrls.some((a) => u.startsWith(a));
  }

  function findChrome() {
    const c = [opts.chrome, process.env.CHROME_PATH,
      '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Chromium.app/Contents/MacOS/Chromium',
      '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge', '/Applications/Brave Browser.app/Contents/MacOS/Brave Browser',
    ].filter(Boolean);
    for (const p of c) if (fs.existsSync(p)) return p;
    for (const b of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge']) { const p = onPath(b); if (p) return p; }
    return null;
  }

  async function ensureBrowser() {
    if (browser.ws) return browser;
    if (browser.starting) return browser.starting;
    browser.starting = (async () => {
      if (typeof WebSocket !== 'function') throw httpError(503, `the browser capability needs Node 22 or newer (a global WebSocket); this is ${process.version}`);
      const bin = findChrome();
      if (!bin) throw httpError(503, 'no Chrome, Chromium, Edge or Brave found (give one with --chrome PATH)');
      const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'dry-companion-chrome-'));
      const child = spawn(bin, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check',
        '--disable-gpu', '--disable-extensions', '--disable-background-networking', '--disable-sync', '--hide-scrollbars', '--mute-audio', 'about:blank'],
      { stdio: ['ignore', 'ignore', 'pipe'] });
      const wsUrl = await new Promise((resolve, reject) => {
        let err = '';
        const t = setTimeout(() => reject(httpError(503, `Chrome did not start: ${err.slice(-300)}`)), 15000);
        child.stderr.on('data', (d) => {
          err += d;
          const m = /DevTools listening on (ws:\/\/\S+)/.exec(err);
          if (m) { clearTimeout(t); resolve(m[1]); }
        });
        child.on('exit', (code) => { clearTimeout(t); reject(httpError(503, `Chrome exited (${code}): ${err.slice(-300)}`)); });
      });
      const ws = new WebSocket(wsUrl);
      await new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = () => reject(httpError(503, 'could not reach Chrome\'s DevTools socket')); });
      ws.onmessage = (m) => {
        const msg = JSON.parse(typeof m.data === 'string' ? m.data : Buffer.from(m.data).toString());
        if (msg.id && browser.pending.has(msg.id)) {
          const { resolve, reject, timer } = browser.pending.get(msg.id);
          browser.pending.delete(msg.id);
          clearTimeout(timer);
          if (msg.error) reject(httpError(502, `${msg.error.message}`)); else resolve(msg.result);
          return;
        }
        const page = [...browser.pages.values()].find((pg) => pg.sessionId === msg.sessionId);
        if (page) onPageEvent(page, msg.method, msg.params || {});
      };
      ws.onclose = () => { browser.ws = null; browser.pages.clear(); };
      Object.assign(browser, { child, ws, profile, bin });
      log(`${shortTime()}  browser started: ${path.basename(bin)} (headless, profile ${profile})`);
      return browser;
    })();
    try { return await browser.starting; } finally { browser.starting = null; }
  }

  function cdp(method, params = {}, sessionId, ms = 20000) {
    return new Promise((resolve, reject) => {
      if (!browser.ws) return reject(httpError(503, 'the browser is not running'));
      const id = browser.nextId++;
      const timer = setTimeout(() => { browser.pending.delete(id); reject(httpError(504, `${method} took longer than ${ms / 1000} s`)); }, ms);
      browser.pending.set(id, { resolve, reject, timer });
      browser.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }

  function onPageEvent(page, method, p) {
    const err = (text) => { page.errors.push(String(text).slice(0, 500)); if (page.errors.length > 50) page.errors.shift(); };
    if (method === 'Runtime.exceptionThrown') err(p.exceptionDetails?.exception?.description || p.exceptionDetails?.text || 'an exception');
    else if (method === 'Runtime.consoleAPICalled' && (p.type === 'error' || p.type === 'assert')) err((p.args || []).map((a) => a.value ?? a.description ?? '').join(' '));
    else if (method === 'Log.entryAdded' && p.entry?.level === 'error') err(`${p.entry.text}${p.entry.url ? ` (${p.entry.url})` : ''}`);
    else if (method === 'Page.loadEventFired') { const w = page.waiters.splice(0); for (const r of w) r(); }
  }

  async function pageFor(key) {
    await ensureBrowser();
    const k = key || 'default';
    if (browser.pages.has(k)) return browser.pages.get(k);
    const { targetId } = await cdp('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp('Target.attachToTarget', { targetId, flatten: true });
    const page = { key: k, targetId, sessionId, errors: [], waiters: [], shots: 0, lastShot: null };
    browser.pages.set(k, page);
    for (const d of ['Page.enable', 'Runtime.enable', 'Log.enable']) await cdp(d, {}, sessionId);
    await cdp('Emulation.setDeviceMetricsOverride', { width: 1280, height: 800, deviceScaleFactor: 1, mobile: false }, sessionId);
    return page;
  }

  function loaded(page, ms) {
    return new Promise((resolve) => { const t = setTimeout(resolve, ms); page.waiters.push(() => { clearTimeout(t); resolve(); }); });
  }

  async function evaluate(page, expression) {
    const r = await cdp('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true }, page.sessionId);
    if (r.exceptionDetails) throw httpError(400, r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result?.value;
  }

  async function guardUrl(page) {
    const href = await evaluate(page, 'location.href');
    if (!urlAllowed(href)) {
      await cdp('Page.navigate', { url: 'about:blank' }, page.sessionId);
      throw httpError(403, `the page went to ${href}, which is not this machine's (allow it with --allow-url)`);
    }
    return href;
  }

  /// The page as text: its accessibility tree (roles, names, values).
  async function pageText(page) {
    try {
      const { nodes } = await cdp('Accessibility.getFullAXTree', {}, page.sessionId);
      const by = new Map(nodes.map((n) => [n.nodeId, n]));
      const lines = [];
      const skip = new Set(['generic', 'none', 'InlineTextBox', 'LineBreak', 'RootWebArea', 'presentation']);
      const walk = (n, depth, parentName) => {
        if (!n || lines.length > 600) return;
        const role = n.role?.value || '';
        const name = String(n.name?.value ?? '').trim();
        const value = n.value?.value;
        let next = depth;
        const show = !n.ignored && !skip.has(role) && !(role === 'StaticText' && (!name || name === parentName));
        if (show) {
          const r = role === 'StaticText' ? 'text' : role;
          lines.push(`${'  '.repeat(depth)}${r}${name ? ` "${name}"` : ''}${value !== undefined && value !== '' ? ` = "${value}"` : ''}`);
          next = depth + 1;
        }
        for (const c of n.childIds || []) walk(by.get(c), next, name || parentName);
      };
      walk(nodes[0], 0, '');
      return lines.join('\n');
    } catch {
      return String(await evaluate(page, 'document.body ? document.body.innerText : ""'));
    }
  }

  async function shoot(page) {
    const png = await cdp('Page.captureScreenshot', { format: 'png' }, page.sessionId);
    if (!fs.existsSync(shotDir)) fs.mkdirSync(shotDir, { recursive: true });
    page.shots += 1;
    const file = path.join(shotDir, `${page.key.replace(/[^A-Za-z0-9_-]/g, '_')}-${page.shots}.png`);
    fs.writeFileSync(file, Buffer.from(png.data, 'base64'));
    page.lastShot = file;
    const jpeg = await cdp('Page.captureScreenshot', { format: 'jpeg', quality: 60, clip: { x: 0, y: 0, width: 1280, height: 800, scale: 0.4 } }, page.sessionId);
    return { file, jpeg: jpeg.data };
  }

  async function describePage(page, { shot = false, did = null } = {}) {
    const href = await guardUrl(page);
    const out = { url: href, title: await evaluate(page, 'document.title'), text: (await pageText(page)).slice(0, 20000), errors: [...page.errors], screenshot: page.lastShot };
    if (shot) { const s = await shoot(page); out.screenshot = s.file; out.jpeg = s.jpeg; }
    if (did) out.did = did;
    return out;
  }

  /// Find an element by a CSS selector or the text it shows; scroll to it; its centre.
  const FIND = `(target, forTyping) => {
    let el = null;
    try { el = document.querySelector(target); } catch {}
    const norm = (s) => (s || '').replace(/\\s+/g, ' ').trim().toLowerCase();
    const want = norm(target);
    if (!el) {
      const sel = forTyping ? 'input,textarea,select,[contenteditable=""],[contenteditable=true]' : 'a,button,input,select,textarea,summary,label,[role=button],[role=link],[role=tab],[role=menuitem],[role=checkbox],[onclick]';
      const all = [...document.querySelectorAll(sel)].filter((e) => e.getClientRects().length);
      const names = (e) => [e.innerText, e.value, e.getAttribute('aria-label'), e.getAttribute('placeholder'), e.getAttribute('title'), e.getAttribute('name'),
        e.id && document.querySelector('label[for="' + e.id + '"]')?.innerText, e.closest('label')?.innerText].map(norm);
      el = all.find((e) => names(e).includes(want)) || all.find((e) => names(e).some((n) => n && n.includes(want)));
    }
    if (!el) return null;
    el.scrollIntoView({ block: 'center', inline: 'center' });
    const r = el.getBoundingClientRect();
    const label = (el.innerText || el.value || el.getAttribute('aria-label') || el.getAttribute('placeholder') || '').trim().slice(0, 60);
    return { x: r.left + r.width / 2, y: r.top + r.height / 2, what: (el.getAttribute('role') || el.tagName.toLowerCase()) + (label ? ' "' + label + '"' : '') };
  }`;

  async function locate(page, target, forTyping) {
    const r = await evaluate(page, `(${FIND})(${JSON.stringify(String(target))}, ${forTyping ? 'true' : 'false'})`);
    if (!r) throw httpError(404, `nothing on the page matches ${JSON.stringify(target)} (a CSS selector, or the text it shows)`);
    return r;
  }

  async function mouseClick(page, x, y) {
    for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) {
      await cdp('Input.dispatchMouseEvent', { type, x, y, button: 'left', clickCount: type === 'mouseMoved' ? 0 : 1 }, page.sessionId);
    }
  }

  async function settleAfter(page, before) {
    await new Promise((r) => setTimeout(r, 400));
    const now = await evaluate(page, 'location.href').catch(() => before);
    if (now !== before) await loaded(page, 5000);
  }

  async function browserOp(op, b) {
    const page = await pageFor(b.page);
    switch (op) {
      case 'open': {
        const url = String(b.url || '');
        if (!urlAllowed(url)) throw httpError(403, `${url} is not this machine's (localhost, 127.0.0.1); allow more with --allow-url`);
        page.errors = [];
        const done = loaded(page, 15000);
        const r = await cdp('Page.navigate', { url }, page.sessionId, 20000);
        if (r.errorText) throw httpError(502, `could not open ${url}: ${r.errorText}`);
        await done;
        await new Promise((res) => setTimeout(res, 300));
        return describePage(page, { shot: true });
      }
      case 'look': return describePage(page);
      case 'screenshot': { await guardUrl(page); const s = await shoot(page); return { url: await evaluate(page, 'location.href'), screenshot: s.file, jpeg: s.jpeg }; }
      case 'click': {
        const before = await evaluate(page, 'location.href');
        const at = await locate(page, b.target, false);
        await mouseClick(page, at.x, at.y);
        await settleAfter(page, before);
        return describePage(page, { shot: true, did: `clicked ${at.what}` });
      }
      case 'type': {
        const before = await evaluate(page, 'location.href');
        const at = await locate(page, b.target, true);
        await mouseClick(page, at.x, at.y);
        await cdp('Input.insertText', { text: String(b.text ?? '') }, page.sessionId);
        if (b.press) {
          const k = String(b.press);
          const code = { Enter: 13, Tab: 9, Escape: 27 }[k];
          await cdp('Input.dispatchKeyEvent', { type: 'keyDown', key: k, code: k, windowsVirtualKeyCode: code, ...(k === 'Enter' ? { text: '\r' } : {}) }, page.sessionId);
          await cdp('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code: k, windowsVirtualKeyCode: code }, page.sessionId);
        }
        await settleAfter(page, before);
        return describePage(page, { shot: true, did: `typed into ${at.what}${b.press ? ` and pressed ${b.press}` : ''}` });
      }
      case 'close': {
        await cdp('Target.closeTarget', { targetId: page.targetId }).catch(() => {});
        browser.pages.delete(page.key);
        return { ok: true };
      }
      default: throw httpError(404, `no browser operation ${op}`);
    }
  }

  function closeBrowser() {
    const had = !!browser.child;
    try { browser.ws?.close(); } catch { /* gone */ }
    try { browser.child?.kill('SIGKILL'); } catch { /* gone */ }
    if (browser.profile) fs.rm(browser.profile, { recursive: true, force: true }, () => {});
    Object.assign(browser, { ws: null, child: null, profile: null });
    browser.pages.clear();
    if (had) log(`${shortTime()}  browser closed`);
    return had;
  }

  // ---- evidence: a project's `dry evidence` run (docs/evidence.md) ----

  const evidenceRuns = new Set();

  /// The `dry` binary: --dry, DRY_BIN, this checkout's target/, then PATH.
  function findDry() {
    const c = [opts.dry, process.env.DRY_BIN, path.join(HERE, '..', 'target', 'release', 'dry'), path.join(HERE, '..', 'target', 'debug', 'dry')].filter(Boolean);
    for (const p of c) if (fs.existsSync(p)) return p;
    return onPath('dry');
  }

  /// The project's evidence as it is now: the manifest, the summary, one line.
  function evidenceRead(project) {
    const dir = path.join(project, 'evidence');
    let manifest = null, summary = '';
    try { manifest = JSON.parse(fs.readFileSync(path.join(dir, 'manifest.json'), 'utf8')); } catch { /* none yet */ }
    try { summary = fs.readFileSync(path.join(dir, 'summary.md'), 'utf8'); } catch { /* none yet */ }
    const line = summary.split('\n').find((l) => / · (pass|fail)$/.test(l)) || (manifest ? `${manifest.shots.length} shots · ${manifest.status}` : 'no evidence yet');
    return { project, line, summary, manifest };
  }

  /// Run `dry evidence` in a project (inside a root) and wait for it.
  async function evidenceRun(b) {
    const project = confine(String(b.project || ''));
    if (!fs.existsSync(path.join(project, 'evidence.md'))) throw httpError(404, `${project} has no evidence.md (docs/evidence.md)`);
    const bin = findDry();
    if (!bin) throw httpError(503, 'no dry binary: build one (cargo build -p dry-tools) or give --dry PATH');
    const args = ['evidence', '--project', project, '--quiet'];
    if (b.update) args.push('--update');
    if (b.expect) {
      const e = String(b.expect).replace(/\s+/g, '');
      if (!/^[A-Za-z0-9_@,-]+$/.test(e)) throw httpError(400, 'expect is scenario names (or name@WxH), comma separated');
      args.push('--expect', e);
    }
    const t0 = Date.now();
    log(`${shortTime()}  evidence ${project}${b.update ? ' --update' : ''}${b.expect ? ` --expect ${b.expect}` : ''}`);
    const { code, out } = await new Promise((resolve) => {
      const child = spawn(bin, args, { cwd: project, stdio: ['ignore', 'pipe', 'pipe'] });
      evidenceRuns.add(child);
      let out = '';
      const add = (d) => { out += d; if (out.length > MAX_OUTPUT) out = out.slice(-MAX_OUTPUT); };
      child.stdout.on('data', add);
      child.stderr.on('data', add);
      child.on('close', (c) => { evidenceRuns.delete(child); resolve({ code: c, out }); });
      child.on('error', (e) => { evidenceRuns.delete(child); resolve({ code: -1, out: e.message }); });
    });
    const r = { ...evidenceRead(project), code, output: lastLines(out, 20), ms: Date.now() - t0 };
    log(`${shortTime()}  evidence: ${r.line} (exit ${code})`);
    return r;
  }

  /// One evidence image (a shot, its .new or .diff), only from the folder.
  function evidenceFile(projectPath, name) {
    const project = confine(String(projectPath || ''));
    const n = String(name || '');
    if (!/^[A-Za-z0-9_@.-]+\.png$/.test(n) || n.startsWith('.')) throw httpError(400, 'not an evidence image');
    return fs.readFileSync(confine(path.join(project, 'evidence', n), { inside: path.join(project, 'evidence') }));
  }

  /// The kill switch: every running command and the browser.
  function stopAll() {
    let n = 0;
    for (const s of sessions.values()) if (s.status === 'running') { kill(s); n += 1; }
    for (const c of evidenceRuns) { try { c.kill('SIGTERM'); } catch { /* gone */ } n += 1; }
    const b = closeBrowser();
    log(`${shortTime()}  stop all: ${n} session${n === 1 ? '' : 's'} stopped${b ? ', browser closed' : ''}`);
    return { stopped: n, browser: b };
  }

  // ---- capabilities (docs/capabilities.md): what this machine provides, over MCP ----

  // capabilities/<id>.md: --capabilities DIR, else ../capabilities next to this script; --no-capabilities: none.
  const capDir = (() => {
    if (opts.capabilities === false) return null;
    const dir = opts.capabilities ? path.resolve(opts.capabilities.replace(/^~(?=$|\/)/, os.homedir())) : path.join(HERE, '..', 'capabilities');
    if (fs.existsSync(dir) && fs.statSync(dir).isDirectory()) return fs.realpathSync(dir);
    if (opts.capabilities) throw new Error(`--capabilities ${dir}: not a folder`);
    return null;
  })();
  const PROBE_MS = 30000;
  const probes = new Map(); // id -> { state, why, at }
  const snaps = new Map(); // id -> { text, at, holds: { event: bool } }
  const mcpStreams = new Set(); // GET /mcp responses
  let capSignature = null;
  let capTimer = null;

  /// Every declared file read, and the files that did not read.
  function loadDeclared() {
    const out = [], errors = [];
    if (!capDir) return { declared: out, errors };
    let files = [];
    try { files = fs.readdirSync(capDir).filter((f) => f.endsWith('.md')).sort(); } catch { return { declared: out, errors }; }
    for (const f of files) {
      try {
        const d = parseCapability(f, fs.readFileSync(path.join(capDir, f), 'utf8'));
        if (`${d.id}.md` !== f) throw new Error(`${f}: its id is \`${d.id}\`: name the file ${d.id}.md`);
        out.push(d);
      } catch (e) { errors.push({ file: path.join(capDir, f), error: e.message }); }
    }
    return { declared: out, errors };
  }

  /// A program and its arguments, no shell, no input, secrets left out; killed at the timeout.
  function runArgv(argv, env = {}, timeoutMs = 30000, cwd = capDir || os.homedir()) {
    return new Promise((resolve) => {
      let out = '', done = false;
      let child;
      try {
        child = spawn(argv[0], argv.slice(1), { cwd, env: { ...cleanEnv(), ...env }, shell: false, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
      } catch (e) { resolve({ code: null, out: '', error: e.message }); return; }
      const take = (d) => { out += d; if (out.length > 65536) out = out.slice(out.length - 65536); };
      child.stdout.on('data', take);
      child.stderr.on('data', take);
      const timer = setTimeout(() => { try { process.kill(-child.pid, 'SIGKILL'); } catch { /* gone */ } finish(null, `timed out after ${Math.round(timeoutMs / 1000)}s`); }, timeoutMs);
      const finish = (code, error) => { if (done) return; done = true; clearTimeout(timer); resolve({ code, out, error }); };
      child.on('error', (e) => finish(null, e.code === 'ENOENT' ? `${argv[0]}: no such program` : e.message));
      child.on('close', (code) => finish(code, null));
    });
  }

  async function probeOf(d, refresh) {
    const had = probes.get(d.id);
    if (had && !refresh && Date.now() - had.at < PROBE_MS) return had;
    let r = { state: 'present', why: '' };
    if (d.probe) {
      const p = await runArgv(d.probe, {}, 30000);
      if (p.code !== 0) r = { state: 'absent', why: p.error ? `probe \`${d.probe.join(' ')}\` did not run: ${p.error}` : `probe \`${d.probe.join(' ')}\` exited ${p.code}` };
    }
    const out = { ...r, at: Date.now() };
    probes.set(d.id, out);
    return out;
  }

  /// The capabilities this companion offers besides the declared ones:
  /// what it already did, now described as capabilities.
  async function builtinCaps() {
    const rt = runtimes || await detectRuntimes();
    const has = (n) => !!rt[n];
    const chrome = findChrome();
    const browserOk = !!chrome && typeof WebSocket === 'function';
    const containers = ['docker', 'podman'].filter(has);
    const tool = (name, description, properties, required, approval = false) => ({ name, description, inputSchema: { type: 'object', properties, required }, approval });
    const out = [
      { id: 'file', state: 'present', doc: `The workspace folder's files: list, read, write, stat (over /v1/fs; list and read here). The folder is ${folder}. Secrets, .git and .dry are never shown.`,
        tools: [tool('file.list', 'The files under a folder of the workspace (at most 200), paths relative to it.', { path: { type: 'string', description: 'a folder in the workspace; empty: all of it' } }, []), tool('file.read', 'One file\'s text.', { path: { type: 'string', description: 'relative to the workspace folder' } }, ['path'])] },
      { id: 'terminal', state: 'present', doc: `Commands in the workspace folder, in this machine's shell (${os.platform()}): run, start, tail, stop (over /v1/term). A dry engine's terminal artifact uses it through the companion.`,
        tools: [tool('terminal.run', 'Run a command in the workspace folder (a shell) and wait: its exit code and last lines.', { command: { type: 'string' }, cwd: { type: 'string', description: 'a folder in the workspace (default: the folder)' } }, ['command'], true)],
        snapshot: async () => { const run = [...sessions.values()].filter((x) => x.status === 'running'); return run.length ? run.map((x) => `running ${x.key || x.id}: ${x.command}`).join('\n') : 'nothing running'; } },
      { id: 'browser', state: browserOk ? 'present' : 'absent', why: browserOk ? '' : (chrome ? 'Node 22 is needed for the browser (global WebSocket)' : 'no Chrome, Chromium, Edge or Brave found'),
        doc: 'A headless browser on this machine for its own pages (localhost): open, look, click, type, screenshot.', tools: BROWSER_TOOLS.map((t) => ({ ...t, approval: false })) },
      { id: 'evidence', state: findDry() ? 'present' : 'absent', why: findDry() ? '' : 'no dry binary to run evidence with (--dry PATH)',
        doc: 'A project\'s evidence (its evidence.md): run it, read the last one.', tools: EVIDENCE_TOOLS.map((t) => ({ ...t, approval: false })) },
      { id: 'git', state: has('git') ? 'present' : 'absent', why: has('git') ? '' : 'no git on PATH', doc: 'git in this machine\'s projects, with its own credentials: status here; push and clone need a person\'s click.',
        tools: [tool('git.status', 'A project\'s branch and changed files.', { path: { type: 'string' } }, ['path'])] },
      { id: 'container', state: containers.length ? 'present' : 'absent', why: containers.length ? '' : 'no docker or podman on PATH',
        doc: `Containers on this machine (${containers.join(', ') || 'none'}): run them with the terminal.`, tools: [],
        snapshot: containers.length ? async () => { const r = await runArgv([rt[containers[0]].path, 'ps', '--format', '{{.Names}} {{.Status}}'], {}, 10000); return r.code === 0 ? (r.out.trim() || 'no containers running') : `${containers[0]} ps failed: ${(r.error || r.out).trim()}`; } : null },
    ];
    // Each declared provider is a `model.<name>` capability: its models, through /v1/chat/completions.
    const { providers } = await providersNow(false);
    for (const pr of providers) {
      out.push({ id: `model.${pr.name}`, state: pr.state === 'ok' ? 'present' : 'absent', why: pr.state === 'ok' ? '' : `${pr.state}: ${pr.why}${pr.hint ? ` (${pr.hint})` : ''}`,
        doc: `Models from ${pr.name} (${pr.kind}${pr.url ? ` at ${pr.url}` : ''}), through this companion's /v1/chat/completions: ${pr.models.join(', ') || 'none'}.`, tools: [], models: pr.models,
        snapshot: async () => pr.models.join('\n') || 'no models' });
    }
    return out;
  }

  /// A capability a person switched off: absent, saying how to switch it on.
  function switched(c) {
    if (isOn(c.id)) return c;
    return { ...c, state: 'absent', off: true, why: `switched off in the companion's terminal (type: on ${c.id})` };
  }

  /// Every capability: the companion's own, then the declared ones, probed.
  async function capabilities(refresh = false) {
    const out = await builtinCaps();
    const { declared, errors } = loadDeclared();
    for (const d of declared) {
      const p = await probeOf(d, refresh);
      out.push({
        id: d.id, state: p.state, why: p.why, doc: d.doc, declared: d, interval: d.interval, events: d.events,
        tools: d.tools.map((t) => {
          const properties = {};
          for (const i of t.inputs) properties[i.name] = { type: i.type, ...(i.description ? { description: i.description } : {}) };
          return { name: `${d.id}.${t.name}`, description: t.doc, inputSchema: { type: 'object', properties, required: t.inputs.filter((i) => i.required).map((i) => i.name) }, approval: t.approval };
        }),
      });
    }
    return { caps: out.map(switched), errors };
  }

  function capMeta(c, extra = {}) {
    return { capability: c.id, doc: c.doc, state: c.state, ...(c.why ? { why: c.why } : {}), ...extra };
  }

  async function capTools() {
    const { caps } = await capabilities();
    return caps.flatMap((c) => c.tools.map(({ approval, ...t }) => ({ ...t, _meta: capMeta(c, { approval: !!approval }) })));
  }

  async function capResources() {
    const { caps } = await capabilities();
    return caps.map((c) => ({
      uri: `dry://capability/${c.id}`, name: c.id, description: c.doc.split('\n')[0], mimeType: 'text/plain',
      _meta: capMeta(c, { snapshot: !!(c.declared?.snapshot || c.snapshot), interval: c.declared ? c.interval : null, events: (c.events || []).map((e) => ({ name: e.name, when: whenText(e.when), text: e.text })) }),
    }));
  }

  /// A capability's snapshot now.
  async function capRead(uri) {
    const id = String(uri || '').replace(/^dry:\/\/capability\//, '');
    const { caps } = await capabilities();
    const c = caps.find((x) => x.id === id);
    if (!c) throw httpError(404, `no capability ${id}`);
    if (c.state !== 'present') return `(${c.state}: ${c.why})`;
    if (c.declared?.snapshot) {
      const r = await runArgv(c.declared.snapshot, {}, 30000);
      return r.code === 0 ? r.out.trimEnd() : `snapshot failed: ${(r.error || r.out).trim()}`;
    }
    if (c.snapshot) return await c.snapshot();
    return '';
  }

  /// A call to a capability's tool (declared ones and the companion's
  /// terminal, file and git); null: not one of them.
  async function capCall(name, args) {
    const text = (t, isError = false) => ({ content: [{ type: 'text', text: t }], isError });
    args = args || {};
    const capId = name.includes('.') ? name.slice(0, name.lastIndexOf('.')) : name.startsWith('browser_') ? 'browser' : name.startsWith('evidence_') ? 'evidence' : null;
    if (capId && !isOn(capId) && (BUILTIN_IDS.includes(capId) || loadDeclared().declared.some((d) => d.id === capId))) need(capId);
    if (name === 'terminal.run') {
      const command = String(args.command || '');
      if (!command) throw httpError(400, 'terminal.run needs command');
      if (needsConfirm(command)) throw httpError(428, 'push, clone and publishing go through the git endpoints with a person\'s click');
      const cwd = args.cwd && path.isAbsolute(String(args.cwd)) ? execCwd(String(args.cwd)) : wsPath(String(args.cwd || ''), { dir: true }).real;
      const s = launch({ cwd, command, mode: 'run', key: null, timeoutMs: 30 * 60 * 1000 });
      await s.done;
      return text(`exit ${s.exit ?? s.signal}\n${lastLines(s.output, 15)}`, s.exit !== 0);
    }
    if (name === 'file.read') {
      const r = wsRead(String(args.path || ''));
      return r.kind === 'text' ? text(r.text) : text(`${r.path}: ${r.kind} (${r.bytes} bytes)`, true);
    }
    if (name === 'file.list') {
      const r = wsList(String(args.path || args.root || ''), 200);
      return text(r.files.join('\n') + (r.truncated ? '\n…' : ''));
    }
    if (name === 'git.status') {
      const proj = gitProject(String(args.path || ''));
      const r = await git(['status', '--porcelain=v1', '-b'], proj);
      return text(r.ok ? r.stdout : r.stderr, !r.ok);
    }
    const dot = name.indexOf('.');
    if (dot < 0) return null;
    const { declared } = loadDeclared();
    const d = declared.find((x) => x.id === name.slice(0, dot));
    if (!d) return null;
    const p = await probeOf(d);
    if (p.state !== 'present') return text(`${d.id} is ${p.state}: ${p.why}`, true);
    const { argv, env, timeoutMs } = capabilityArgv(d, name.slice(dot + 1), args);
    const r = await runArgv(argv, env, timeoutMs);
    if (r.error) return text(r.error, true);
    return text(r.out.trimEnd() || '(done, no output)', r.code !== 0);
  }

  /// For /v1/capabilities and the terminal's `capabilities`: each one's id, state, tools.
  async function capabilitiesSummary(refresh) {
    const { caps, errors } = await capabilities(refresh);
    return { dir: capDir, list: caps.map((c) => ({ id: c.id, state: c.state, why: c.why || '', tools: c.tools.map((t) => t.name), declared: !!c.declared })), errors };
  }

  function notify(method, params) {
    const msg = `data: ${JSON.stringify({ jsonrpc: '2.0', method, params })}\n\n`;
    for (const r of mcpStreams) r.write(msg);
  }

  /// While a peer listens: the declared snapshots read at their intervals
  /// (an event that became true rides on its update), and the folder
  /// watched (a change is `tools/list_changed`: the peer re-registers).
  async function capTick() {
    const { declared } = loadDeclared();
    for (const d of declared) await probeOf(d);
    const sig = JSON.stringify(declared.map((d) => [d.id, d.doc, d.tools, d.events, d.interval, probes.get(d.id)?.state]));
    if (capSignature !== null && sig !== capSignature) notify('notifications/tools/list_changed', {});
    capSignature = sig;
    for (const d of declared) {
      if (!d.snapshot) continue;
      const p = await probeOf(d);
      if (p.state !== 'present') continue;
      const had = snaps.get(d.id);
      if (had && Date.now() - had.at < d.interval * 1000) continue;
      const r = await runArgv(d.snapshot, {}, 30000);
      const text = r.code === 0 ? r.out.trimEnd() : `snapshot failed: ${(r.error || r.out).trim()}`;
      const holds = Object.fromEntries(d.events.map((e) => [e.name, whenHolds(e.when, text)]));
      snaps.set(d.id, { text, at: Date.now(), holds });
      if (!had) continue; // the first reading never fires
      const uri = `dry://capability/${d.id}`;
      const fired = d.events.filter((e) => holds[e.name] && !had.holds[e.name]);
      for (const e of fired) {
        log(`${shortTime()}  capability ${d.id}: event ${e.name}`);
        notify('notifications/resources/updated', { uri, _meta: { event: { name: e.name, text: e.text } } });
      }
      if (!fired.length && text !== had.text) notify('notifications/resources/updated', { uri });
    }
  }

  function openMcpStream(req, res, origin, pairing) {
    res.writeHead(200, { ...cors(origin), 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
    res.write(': dry-companion capabilities stream\n\n');
    res.pairing = pairing;
    mcpStreams.add(res);
    pairing.streams = (pairing.streams || 0) + 1;
    log(`${shortTime()}  peer ${pairing.id}${pairing.hello ? ` (${pairing.hello.runtime})` : ''} listening`);
    const beat = setInterval(() => res.write(': ping\n\n'), heartbeatMs);
    if (!capTimer) capTimer = setInterval(() => { capTick().catch((e) => log(`${shortTime()}  capabilities: ${e.message}`)); }, 1000);
    req.on('close', () => {
      clearInterval(beat);
      mcpStreams.delete(res);
      pairing.streams -= 1;
      log(`${shortTime()}  peer ${pairing.id} stopped listening`);
      if (!mcpStreams.size && capTimer) { clearInterval(capTimer); capTimer = null; }
    });
  }

  // ---- MCP (streamable HTTP at /mcp): the same tools for Claude Code or any MCP client ----

  const QUIET_GETS = new Set(['/v1/fs/read', '/v1/fs/list', '/v1/fs/stat', '/v1/term/tail', '/v1/health', '/v1/exec/session', '/v1/exec/sessions', '/v1/files/read', '/v1/files/list', '/v1/files/tree', '/v1/events/log', '/v1/workspace', '/v1/pairings', '/v1/capabilities', '/v1/providers', '/v1/models', '/v1/store', '/v1/switches', '/v1/tools', '/v1/projects']);
  // The page's saves: every step writes them; a line each would hide the calls.
  const QUIET_POSTS = new Set(['/v1/store', '/v1/workspace/snapshot']);
  const BUILTIN_IDS = ['file', 'terminal', 'browser', 'evidence', 'git', 'container'];
  const MCP_MODERN = ['2026-07-28'];
  const MCP_LEGACY = ['2025-11-25', '2025-06-18', '2025-03-26'];
  const BROWSER_TOOLS = [
    { name: 'browser_open', description: 'Open a page of this machine (localhost) in a headless browser: its text, console errors and a screenshot.', inputSchema: { type: 'object', properties: { url: { type: 'string' } }, required: ['url'] } },
    { name: 'browser_look', description: 'The open page as text (its accessibility tree) and its console errors.', inputSchema: { type: 'object', properties: {} } },
    { name: 'browser_click', description: 'Click an element of the open page: a CSS selector or the text it shows.', inputSchema: { type: 'object', properties: { target: { type: 'string' } }, required: ['target'] } },
    { name: 'browser_type', description: 'Type into a field of the open page; press: a key after (Enter).', inputSchema: { type: 'object', properties: { target: { type: 'string' }, text: { type: 'string' }, press: { type: 'string' } }, required: ['target', 'text'] } },
    { name: 'browser_screenshot', description: 'A screenshot of the open page (a PNG file, and a small JPEG).', inputSchema: { type: 'object', properties: {} } },
  ];
  const EVIDENCE_TOOLS = [
    { name: 'evidence_run', description: 'Run a project\'s evidence (its evidence.md): build, start the app, screenshot every scenario and compare with the committed evidence/ folder. One line back, then the summary.', inputSchema: { type: 'object', properties: { project: { type: 'string' }, expect: { type: 'string', description: 'scenarios expected to change, comma separated' }, update: { type: 'boolean' } }, required: ['project'] } },
    { name: 'evidence_read', description: 'A project\'s last evidence: one line, then the summary (which shots changed, by how much, the page-text lines).', inputSchema: { type: 'object', properties: { project: { type: 'string' } }, required: ['project'] } },
  ];

  /// Every tool: the custom ones, then each capability's, grouped by
  /// `_meta.capability` (the browser and evidence tools are in their groups).
  async function mcpTools() {
    return [...loadTools().tools.map((t) => ({ name: t.name, description: t.description, inputSchema: schemaOf(t) })), ...await capTools()];
  }

  async function mcpCall(name, args) {
    const cap = await capCall(name, args).catch((e) => ({ content: [{ type: 'text', text: e.message }], isError: true, _meta: { code: e.code || 'internal' } }));
    if (cap) return cap;
    const b = (name.startsWith('browser_') && BROWSER_TOOLS.some((t) => t.name === name)) || EVIDENCE_TOOLS.some((t) => t.name === name);
    try {
      if (name === 'evidence_run' || name === 'evidence_read') {
        const r = name === 'evidence_run' ? await evidenceRun(args || {}) : evidenceRead(confine(String(args?.project || '')));
        const exit = r.code === undefined ? '' : ` (exit ${r.code})`;
        return { content: [{ type: 'text', text: `evidence: ${r.line}${exit}\n\n${r.summary || r.output || ''}` }], isError: r.code !== undefined && r.code > 1 };
      }
      if (b) {
        const op = name.slice('browser_'.length);
        const r = await browserOp(op === 'look' ? 'look' : op, { ...(args || {}), page: 'mcp' });
        const text = r.text != null ? `${r.did ? r.did + '\n' : ''}${r.url} "${r.title}"\n${r.text}\nconsole errors: ${r.errors.length ? r.errors.join('\n') : 'none'}\nscreenshot: ${r.screenshot || 'none'}` : `screenshot: ${r.screenshot}`;
        const content = [{ type: 'text', text }];
        if (r.jpeg) content.push({ type: 'image', data: r.jpeg, mimeType: 'image/jpeg' });
        return { content, isError: false };
      }
      const r = await callTool(name, args || {});
      return { content: [{ type: 'text', text: r.output || `(exit ${r.exit})` }], isError: !r.ok };
    } catch (e) {
      if (e.status === 404 && !b) throw e;
      return { content: [{ type: 'text', text: e.message }], isError: true };
    }
  }

  function decodeHeader(v) {
    const m = /^=\?base64\?(.*)\?=$/.exec(v || '');
    return m ? Buffer.from(m[1], 'base64').toString('utf8') : v;
  }

  async function handleMcp(req, res, origin, pairing) {
    const reply = (status, body, extra = {}) => {
      res.writeHead(status, { ...cors(origin), 'content-type': 'application/json', ...extra });
      res.end(body == null ? undefined : JSON.stringify(body));
    };
    const err = (status, id, code, message, data) => reply(status, { jsonrpc: '2.0', ...(id !== undefined ? { id } : {}), error: { code, message, ...(data ? { data } : {}) } });
    // GET with Accept: text/event-stream is a peer's stream of notifications (capabilities).
    if (req.method === 'GET' && /text\/event-stream/.test(req.headers.accept || '')) { openMcpStream(req, res, origin, pairing); return 200; }
    if (req.method !== 'POST') { res.writeHead(405, { ...cors(origin), allow: 'POST' }); res.end(); return 405; }
    let msg;
    try { msg = await readBody(req); } catch { err(400, null, -32700, 'Parse error'); return 400; }
    if (Array.isArray(msg) || typeof msg !== 'object' || msg.jsonrpc !== '2.0' || typeof msg.method !== 'string') { err(400, null, -32600, 'Invalid Request'); return 400; }
    if (msg.id === undefined) { res.writeHead(202, cors(origin)); res.end(); return 202; }
    const { id, method } = msg;
    const params = msg.params || {};
    const meta = params._meta?.['io.modelcontextprotocol/protocolVersion'];
    const header = req.headers['mcp-protocol-version'];
    if (meta !== undefined) {
      // A modern request: stateless, its version and method mirrored in headers.
      if (!MCP_MODERN.includes(meta)) { err(400, id, -32022, 'Unsupported protocol version', { supported: [...MCP_MODERN, ...MCP_LEGACY], requested: meta }); return 400; }
      if (header !== meta) { err(400, id, -32020, `Header mismatch: MCP-Protocol-Version ${header ?? '(missing)'} does not match ${meta}`); return 400; }
      if (req.headers['mcp-method'] !== method) { err(400, id, -32020, `Header mismatch: Mcp-Method ${req.headers['mcp-method'] ?? '(missing)'} does not match ${method}`); return 400; }
      if (method === 'tools/call' && decodeHeader(req.headers['mcp-name']) !== params.name) { err(400, id, -32020, `Header mismatch: Mcp-Name does not match ${params.name}`); return 400; }
    } else if (header && !MCP_LEGACY.includes(header)) {
      err(400, id, -32022, 'Unsupported protocol version', { supported: [...MCP_MODERN, ...MCP_LEGACY], requested: header }); return 400;
    }
    const info = { name: NAME, version: VERSION };
    let result;
    switch (method) {
      case 'initialize': {
        const asked = params.protocolVersion;
        // The handshake's hello (docs/capabilities.md §4): theirs kept on the pairing, ours back.
        if (params._meta?.hello && pairing) {
          pairing.hello = params._meta.hello;
          log(`${shortTime()}  ${pairing.id} is ${pairing.hello.runtime || '?'} (${pairing.hello.platform || '?'}${pairing.hello.version ? ` ${pairing.hello.version}` : ''})`);
        }
        result = { protocolVersion: MCP_LEGACY.includes(asked) ? asked : MCP_LEGACY[0], capabilities: { tools: { listChanged: true }, resources: { listChanged: true, subscribe: false } }, serverInfo: info,
          instructions: 'This machine\'s capabilities (terminal, file, browser, git, container, models, and capabilities/*.md), grouped by _meta.capability, and its custom tools (tools/*.md, tools/*.json in its projects).',
          _meta: { hello: { runtime: NAME, platform: `${os.platform()} ${os.arch()}`, version: VERSION, protocol: 'dry-capabilities/1' } } };
        break;
      }
      case 'server/discover': result = { supportedVersions: [...MCP_MODERN, ...MCP_LEGACY], capabilities: { tools: {} }, serverInfo: info }; break;
      case 'ping': result = {}; break;
      case 'tools/list': result = { tools: await mcpTools() }; break;
      case 'resources/list': result = { resources: await capResources() }; break;
      case 'resources/read':
        try { result = { contents: [{ uri: params.uri, mimeType: 'text/plain', text: await capRead(params.uri) }] }; } catch (e) { err(e.status === 404 ? 200 : 400, id, -32002, e.message); return 400; }
        break;
      case 'tools/call': {
        const t1 = Date.now();
        const who = pairing?.hello?.runtime ? `${pairing.id} (${pairing.hello.runtime})` : pairing?.id;
        log(`${shortTime()}  call ${String(params.name || '')} from ${who} …`);
        try { result = await mcpCall(String(params.name || ''), params.arguments); } catch (e) { log(`${shortTime()}  call ${params.name}: ${e.message}`); err(e.status === 404 ? 200 : 400, id, -32602, e.message); return 400; }
        log(`${shortTime()}  call ${params.name} ${result.isError ? 'failed' : 'done'} ${Date.now() - t1}ms`);
        break;
      }
      default:
        if (meta !== undefined) { err(404, id, -32601, 'Method not found'); return 404; }
        reply(200, { jsonrpc: '2.0', id, error: { code: -32601, message: 'Method not found' } });
        return 200;
    }
    reply(200, { jsonrpc: '2.0', id, result });
    return 200;
  }

  // ---- the app, served from here (same origin: no CORS, no local network prompt) ----

  function serveApp(req, res, p) {
    if (!appDir) return false;
    if (req.method !== 'GET' && req.method !== 'HEAD') return false;
    let rel;
    try { rel = decodeURIComponent(p); } catch { return false; }
    if (rel === '/app') { res.writeHead(301, { location: '/app/' }); res.end(); return true; }
    let full = path.join(appDir, rel);
    try {
      if (fs.statSync(full).isDirectory()) full = path.join(full, 'index.html');
      const real = fs.realpathSync(full);
      if (!within(real, appDir)) return false;
      const ext = path.extname(real).toLowerCase();
      const body = fs.readFileSync(real);
      res.writeHead(200, {
        'content-type': MIME[ext] || 'application/octet-stream', 'cache-control': 'no-cache', 'x-content-type-options': 'nosniff',
        'cross-origin-opener-policy': 'same-origin', 'cross-origin-embedder-policy': 'require-corp', 'cross-origin-resource-policy': 'same-origin',
      });
      res.end(req.method === 'HEAD' ? undefined : body);
      return true;
    } catch {
      return false;
    }
  }

  // ---- HTTP ----

  function httpError(status, message, code) {
    const e = new Error(message);
    e.status = status;
    e.code = code || CODES[status] || 'internal';
    return e;
  }

  function cors(origin) {
    if (!originAllowed(origin, extraOrigins)) return {};
    return { 'access-control-allow-origin': origin, vary: 'Origin' };
  }

  function send(res, status, obj, origin) {
    const body = JSON.stringify(obj);
    res.writeHead(status, { ...cors(origin), 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
    res.end(body);
  }

  async function readBody(req) {
    const chunks = [];
    let n = 0;
    for await (const c of req) {
      n += c.length;
      if (n > MAX_BODY) throw httpError(413, 'the body is too large');
      chunks.push(c);
    }
    if (!n) return {};
    try { return JSON.parse(Buffer.concat(chunks).toString('utf8')); } catch { throw httpError(400, 'the body is not JSON'); }
  }

  function tokenOf(req) {
    const h = req.headers['x-dry-token'];
    if (typeof h === 'string' && h) return h;
    const a = req.headers.authorization;
    if (typeof a === 'string' && a.startsWith('Bearer ')) return a.slice(7).trim();
    return null;
  }

  function sameText(a, b) {
    const x = Buffer.from(String(a)), y = Buffer.from(String(b));
    return x.length === y.length && crypto.timingSafeEqual(x, y);
  }

  async function handle(req, res) {
    const t0 = Date.now();
    const origin = req.headers.origin;
    const url = new URL(req.url, `http://${req.headers.host || '127.0.0.1'}`);
    const p = url.pathname;
    const q = Object.fromEntries(url.searchParams);
    let status = 200, note = '';
    // Reads the page makes at every render are not lines (--verbose shows them); writes, runs and failures are.
    const done = () => {
      if (!verbose && p === '/favicon.ico') return;
      if (!verbose && status < 400 && (note === 'app' || (req.method === 'GET' && QUIET_GETS.has(p)) || (req.method === 'POST' && QUIET_POSTS.has(p)) || (p === '/mcp' && note === ''))) return;
      log(`${shortTime()}  ${req.method.padEnd(4)} ${p} ${status} ${Date.now() - t0}ms${origin ? ` ${origin.replace(/^https?:\/\//, '')}` : ''}${note ? ` ${note}` : ''}`);
    };
    try {
      if (!hostAllowed(req.headers.host, server.address()?.port ?? port)) {
        status = 403; send(res, 403, { error: 'this server answers only to 127.0.0.1 / localhost', code: 'host_refused' }); return;
      }
      if (origin && !originAllowed(origin, extraOrigins)) {
        status = 403; send(res, 403, { error: `origin ${origin} is not allowed (add it with --origin)`, code: 'origin_refused' }); return;
      }
      if (req.method === 'OPTIONS') {
        status = 204;
        res.writeHead(204, {
          ...cors(origin),
          'access-control-allow-methods': 'GET, POST, OPTIONS',
          'access-control-allow-headers': 'content-type, authorization, x-dry-token, last-event-id, mcp-protocol-version, mcp-method, mcp-name, accept',
          'access-control-allow-private-network': 'true',
          'access-control-max-age': '600',
        });
        res.end();
        return;
      }
      if (!p.startsWith('/v1/') && !p.startsWith('/api/') && p !== '/mcp') {
        if (serveApp(req, res, p)) { note = 'app'; return; }
        if (appDir && (req.method === 'GET' || req.method === 'HEAD')) { status = 404; send(res, 404, { error: `no file ${p} in the app`, code: 'not_found' }, origin); return; }
      }
      if (p === '/mcp') {
        if (!pairingOf(tokenOf(req))) {
          status = 401;
          res.writeHead(401, { ...cors(origin), 'content-type': 'application/json', 'www-authenticate': 'Bearer realm="dry-companion"' });
          res.end(JSON.stringify({ error: 'not paired: use the pairing token as a Bearer token', code: 'not_paired' }));
          return;
        }
        status = await handleMcp(req, res, origin, pairingOf(tokenOf(req)));
        return;
      }
      if (p === '/v1/health' && req.method === 'GET') {
        const tok = tokenOf(req);
        send(res, 200, { ok: true, name: NAME, version: VERSION, protocol: PROTOCOL, paired: !!pairingOf(tok), app: !!appDir }, origin);
        return;
      }
      if (p === '/v1/pair' && req.method === 'POST') {
        const b = await readBody(req);
        if (typeof b.code !== 'string' || !sameText(b.code.trim().toUpperCase(), code)) {
          failed += 1;
          if (failed >= FAILED_PAIRS) { code = newCode(); failed = 0; log(`${shortTime()}  ${FAILED_PAIRS} wrong codes: the code changed`); announceCode(); }
          status = 401; send(res, 401, { error: 'wrong pairing code: use the one this companion printed', code: 'wrong_code' }, origin); return;
        }
        const token = crypto.randomBytes(32).toString('hex');
        const now = Date.now();
        const pairing = { id: `p${nextPairing++}`, origin: origin || '', created: now, expires: now + ttlMs, seen: now };
        pairings.set(token, pairing);
        code = newCode();
        failed = 0;
        note = `paired ${pairing.id}`;
        send(res, 200, { token, id: pairing.id, expires: sec(pairing.expires), ttl: sec(ttlMs), name: NAME, version: VERSION, protocol: PROTOCOL }, origin);
        log(`${shortTime()}  connected: ${pairing.id} from ${origin || '(no origin: a program)'} until ${new Date(pairing.expires).toTimeString().slice(0, 5)}`);
        announceCode();
        return;
      }
      const tok = tokenOf(req);
      if (!pairingOf(tok)) { status = 401; send(res, 401, { error: 'not paired (or the pairing expired or was revoked): send a pairing code to /v1/pair', code: 'not_paired' }, origin); return; }

      const get = req.method === 'GET', post = req.method === 'POST';
      if (p === '/v1/unpair' && post) { const pr = pairings.get(tok); pairings.delete(tok); if (pr) { closeStreams(pr); log(`${shortTime()}  pairing ${pr.id} left`); } send(res, 200, { ok: true }, origin); return; }
      if (p === '/v1/pairings' && get) { send(res, 200, { pairings: listPairings(tok) }, origin); return; }
      if (p === '/v1/pairings/revoke' && post) {
        const b = await readBody(req);
        if (!revoke(String(b.id || ''))) { status = 404; send(res, 404, { error: `no pairing ${b.id}`, code: 'not_found' }, origin); return; }
        note = `revoked ${b.id}`;
        send(res, 200, { ok: true }, origin);
        return;
      }
      if (p === '/v1/stop-all' && post) { const r = stopAll(); note = `stopped ${r.stopped}`; send(res, 200, r, origin); return; }
      if (p === '/v1/tools' && get) { const t = loadTools(); send(res, 200, { tools: t.tools.map(publicTool), errors: t.errors }, origin); return; }
      if (p === '/v1/tools/call' && post) {
        const b = await readBody(req);
        const r = await callTool(String(b.name || ''), b.arguments);
        note = `tool ${b.name} ${r.ok ? 'ok' : `exit ${r.exit}`}`;
        send(res, 200, r, origin);
        return;
      }
      if (p === '/v1/evidence/run' && post) {
        need('evidence');
        const b = await readBody(req);
        const r = await evidenceRun(b);
        note = `evidence ${r.line}`;
        send(res, 200, r, origin);
        return;
      }
      if (p === '/v1/evidence' && get) { send(res, 200, evidenceRead(confine(String(q.project || ''))), origin); return; }
      if (p === '/v1/evidence/file' && get) {
        const bytes = evidenceFile(q.project, q.name);
        res.writeHead(200, { ...cors(origin), 'content-type': 'image/png', 'cache-control': 'no-store' });
        res.end(bytes);
        return;
      }
      if (p.startsWith('/v1/browser/') && post) {
        need('browser');
        const op = p.slice('/v1/browser/'.length);
        const b = await readBody(req);
        const r = await browserOp(op, b);
        note = `browser ${op}${b.url ? ' ' + b.url : ''}${b.target ? ' ' + b.target : ''}`;
        send(res, 200, r, origin);
        return;
      }
      if (p === '/v1/events/log' && get) {
        const since = Number(q.since) || 0;
        send(res, 200, { events: events.filter((e) => e.id > since), last: nextEvent - 1 }, origin);
        return;
      }
      if (p === '/v1/capabilities' && get) {
        const rt = q.refresh || !runtimes ? await detectRuntimes() : runtimes;
        send(res, 200, {
          name: NAME, version: VERSION, protocol: PROTOCOL, platform: `${os.platform()} ${os.arch()}`, hostname: os.hostname(), node: process.version,
          roots, folder, runtimes: rt, providers: (await providersNow(false)).providers, store: storeDir,
          tools: loadTools().tools.map(publicTool), tool_errors: loadTools().errors,
          browser: { chrome: findChrome(), node_websocket: typeof WebSocket === 'function', allow: ['localhost', '127.0.0.1', ...allowUrls] },
          evidence: { dry: findDry() },
          capabilities: await capabilitiesSummary(!!q.refresh),
          app: appDir, mcp: '/mcp', events: nextEvent - 1,
        }, origin);
        return;
      }
      // The workspace folder's primitives (docs/companion-protocol.md §6).
      if (p === '/v1/workspace' && get) { send(res, 200, workspaceInfo(), origin); return; }
      if (p === '/v1/workspace/snapshot' && post) {
        const b = await readBody(req);
        const r = snapshotWrite(b.snapshot);
        note = 'snapshot saved';
        send(res, 200, r, origin);
        return;
      }
      if (p.startsWith('/v1/fs/')) {
        need('file');
        const op = p.slice('/v1/fs/'.length);
        if (op === 'list' && get) { send(res, 200, wsList(q.path ?? '', Math.max(1, Math.min(Number(q.max) || 1000, 10000))), origin); return; }
        if (op === 'read' && get) { send(res, 200, wsRead(q.path ?? ''), origin); return; }
        if (op === 'stat' && get) { send(res, 200, wsStat(q.path ?? ''), origin); return; }
        if (op === 'write' && post) {
          const b = await readBody(req);
          const r = wsWrite(b.path, b.text);
          note = `wrote ${r.path} (${r.bytes} bytes)`;
          send(res, 200, r, origin);
          return;
        }
      }
      if (p.startsWith('/v1/term/')) {
        const op = p.slice('/v1/term/'.length);
        if (op === 'run' && post) {
          const b = await readBody(req);
          const { key, command, cwd } = termPrep(b);
          const timeoutMs = Math.min(Math.max(Number(b.timeout_ms) || 30 * 60 * 1000, 100), MAX_TIMEOUT);
          log(`${shortTime()}  run ${key}: ${command.slice(0, 80)}`);
          const s = launch({ cwd, command, mode: 'run', key, timeoutMs });
          await s.done;
          note = `${key} exit ${s.exit ?? s.signal}`;
          send(res, 200, { ...termSession(s), output: s.output.slice(-64 * 1024), ms: Date.now() - t0 }, origin);
          return;
        }
        if (op === 'start' && post) {
          const b = await readBody(req);
          const { key, command, cwd } = termPrep(b);
          const mode = b.callback === false ? 'background' : 'start';
          const timeoutMs = b.timeout_ms ? Math.min(Math.max(Number(b.timeout_ms), 100), MAX_TIMEOUT) : 0;
          log(`${shortTime()}  ${mode} ${key}: ${command.slice(0, 80)}`);
          const s = launch({ cwd, command, mode, key, timeoutMs });
          send(res, 200, termSession(s), origin);
          return;
        }
        if (op === 'tail' && get) {
          need('terminal');
          const key = termKey(q.key);
          const s = findSession({ key });
          const lines = Math.max(0, Math.min(Number(q.lines) || 30, 2000));
          send(res, 200, s ? termSession(s, lines) : { key, status: 'unknown', exit: null, tail: '' }, origin);
          return;
        }
        if (op === 'stop' && post) {
          need('terminal');
          const b = await readBody(req);
          const s = findSession({ key: termKey(b.key) });
          if (!s) throw httpError(404, `no session ${b.key}`, 'not_found');
          kill(s);
          note = `stop ${b.key}`;
          send(res, 200, termSession(s), origin);
          return;
        }
      }
      if (p === '/v1/providers' && get) { send(res, 200, await providersNow(!!q.refresh), origin); return; }
      if (p === '/v1/providers' && post) {
        const b = await readBody(req);
        const r = await addProvider(b.provider ?? b);
        note = `provider ${r.name} ${r.state}`;
        send(res, 200, { provider: r }, origin);
        return;
      }
      if (p === '/v1/switches' && get) {
        const { caps } = await capabilities();
        send(res, 200, { file: configFile, switches: caps.map((c) => ({ id: c.id, on: isOn(c.id), actuator: actuator(c.id) })) }, origin);
        return;
      }
      if (p === '/v1/projects' && get) { send(res, 200, { roots, projects: projects() }, origin); return; }
      if (p === '/v1/files/tree' && get) {
        const dir = confine(q.path || roots[0]);
        if (!fs.statSync(dir).isDirectory()) throw httpError(400, `${q.path} is not a folder`);
        const entries = [];
        for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
          if (HIDDEN.has(e.name) || secretName(e.name)) continue;
          const full = path.join(dir, e.name);
          let isDir = e.isDirectory(), size = 0, link = false;
          if (e.isSymbolicLink()) {
            link = true;
            try { const real = fs.realpathSync(full); if (!roots.some((r) => within(real, r))) continue; isDir = fs.statSync(real).isDirectory(); } catch { continue; }
          }
          if (!isDir) try { size = fs.statSync(full).size; } catch { /* gone */ }
          entries.push({ name: e.name, path: full, dir: isDir, size, link });
        }
        entries.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
        send(res, 200, { path: dir, project: projectOf(dir), entries }, origin);
        return;
      }
      if (p === '/v1/files/list' && get) {
        // dry_core::live::Host::list: files under the root, a folder's own files first, then its folders' (breadth first).
        const base = confine(q.root || roots[0]);
        const max = Math.min(Number(q.max) || 200, 5000);
        const out = [];
        const queue = [''];
        while (queue.length && out.length < max) {
          const rel = queue.shift();
          let entries = [];
          try { entries = fs.readdirSync(path.join(base, rel), { withFileTypes: true }); } catch { continue; }
          for (const e of entries.map((x) => x).sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))) {
            if (HIDDEN.has(e.name) || secretName(e.name)) continue;
            const r = rel ? `${rel}/${e.name}` : e.name;
            let isDir = e.isDirectory();
            if (e.isSymbolicLink()) {
              try { const real = fs.realpathSync(path.join(base, r)); if (!within(real, base)) continue; isDir = fs.statSync(real).isDirectory(); } catch { continue; }
            }
            if (isDir) queue.push(r);
            else { out.push(r); if (out.length >= max) break; }
          }
        }
        send(res, 200, { root: base, files: out }, origin);
        return;
      }
      if (p === '/v1/files/read' && get) {
        let real;
        try { real = confine(q.path, { inside: q.within || null }); } catch (e) {
          send(res, 200, { kind: 'missing', why: e.message }, origin); return;
        }
        const st = fs.statSync(real);
        if (st.isDirectory()) { send(res, 200, { kind: 'missing', why: `${q.path} is a folder` }, origin); return; }
        if (st.size > MAX_READ) { send(res, 200, { kind: 'too_large', bytes: st.size }, origin); return; }
        const bytes = fs.readFileSync(real);
        let text = null;
        if (!bytes.includes(0)) try { text = new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { /* binary */ }
        if (text == null) {
          send(res, 200, { kind: 'binary', bytes: st.size, image: /\.(png|jpe?g|gif|webp|svg|bmp|ico|avif)$/i.test(real) }, origin);
        } else {
          send(res, 200, { kind: 'text', text, bytes: st.size, path: real }, origin);
        }
        return;
      }
      if (p === '/v1/files/write' && post) {
        need('file');
        const b = await readBody(req);
        if (typeof b.text !== 'string') throw httpError(400, 'write takes text');
        const real = confine(b.path, { forWrite: true, inside: b.within || null });
        if (real.split(path.sep).includes('.git')) throw httpError(403, 'files inside .git are written only by git');
        fs.mkdirSync(path.dirname(real), { recursive: true });
        fs.writeFileSync(real, b.text);
        note = real;
        send(res, 200, { ok: true, path: real, bytes: Buffer.byteLength(b.text) }, origin);
        return;
      }
      if (p === '/v1/exec/run' && post) {
        need('terminal');
        const b = await readBody(req);
        const command = String(b.command || '').trim();
        if (!command) throw httpError(400, 'run takes a command');
        if (needsConfirm(command)) throw httpError(428, 'push, clone and publishing commands go through the git endpoints with a person\'s click (docs/companion.md)');
        const cwd = execCwd(b.cwd);
        const timeoutMs = Math.min(Math.max(Number(b.timeout_ms) || 120000, 100), MAX_TIMEOUT);
        const s = launch({ cwd, command, mode: 'run', key: b.key, timeoutMs });
        note = `exec ${s.id}: ${command.slice(0, 60)}`;
        await s.done;
        send(res, 200, { ...publicSession(s), output: s.output.slice(-64 * 1024), ms: Date.now() - t0 }, origin);
        return;
      }
      if (p === '/v1/exec/start' && post) {
        need('terminal');
        const b = await readBody(req);
        const command = String(b.command || '').trim();
        if (!command) throw httpError(400, 'start takes a command');
        if (needsConfirm(command)) throw httpError(428, 'push, clone and publishing commands go through the git endpoints with a person\'s click (docs/companion.md)');
        const mode = b.mode === 'background' ? 'background' : 'start';
        const cwd = execCwd(b.cwd);
        const timeoutMs = b.timeout_ms ? Math.min(Math.max(Number(b.timeout_ms), 100), MAX_TIMEOUT) : 0;
        const s = launch({ cwd, command, mode, key: b.key, timeoutMs });
        note = `exec ${s.id} ${mode}: ${command.slice(0, 60)}`;
        send(res, 200, publicSession(s), origin);
        return;
      }
      if (p === '/v1/exec/sessions' && get) { send(res, 200, { sessions: [...sessions.values()].map((s) => publicSession(s)) }, origin); return; }
      if (p === '/v1/exec/session' && get) {
        const s = findSession(q);
        if (!s) { status = 404; send(res, 404, { error: 'no such session (it may have been lost when the companion restarted)', code: 'not_found' }, origin); return; }
        send(res, 200, publicSession(s, Math.min(Number(q.tail) || 30, 2000)), origin);
        return;
      }
      if (p === '/v1/exec/stop' && post) {
        const b = await readBody(req);
        const s = findSession(b);
        if (!s) { status = 404; send(res, 404, { error: 'no such session', code: 'not_found' }, origin); return; }
        kill(s);
        send(res, 200, publicSession(s), origin);
        return;
      }
      if (p === '/v1/events' && get) {
        // Server-sent events over a fetch (EventSource cannot send the token header).
        res.writeHead(200, { ...cors(origin), 'content-type': 'text/event-stream', 'cache-control': 'no-store', connection: 'keep-alive' });
        const l = { res, id: q.id || null, all: q.all === '1', pairing: pairingOf(tok) };
        // Resume: what was logged after the last id the page had (?since=, or Last-Event-ID).
        const since = Number(q.since ?? req.headers['last-event-id'] ?? NaN);
        res.write(`event: hello\ndata: ${JSON.stringify({ sessions: [...sessions.values()].filter((s) => s.status === 'running').length, last: nextEvent - 1 })}\n\n`);
        if (Number.isFinite(since)) for (const e of events) if (e.id > since && wants(l, e)) sse(res, e);
        listeners.add(l);
        const beat = setInterval(() => res.write(': ping\n\n'), heartbeatMs);
        req.on('close', () => { clearInterval(beat); listeners.delete(l); });
        note = 'stream open';
        return;
      }
      if (p.startsWith('/v1/git/') && post) {
        const op = p.slice('/v1/git/'.length);
        const b = await readBody(req);
        const r = await gitCall(op, b);
        note = `git ${op}${r.ok === false ? ' failed' : ''}`;
        send(res, 200, r, origin);
        return;
      }
      if (p === '/v1/models' && get) { send(res, 200, { object: 'list', data: await allModels() }, origin); return; }
      if (p === '/api/v0/models' && get) { send(res, 200, { object: 'list', data: await allModels() }, origin); return; }
      if (p === '/v1/chat/completions' && post) {
        const b = await readBody(req);
        const model = String(b.model || '');
        note = model;
        const slash = model.indexOf('/');
        const name = slash < 0 ? model : model.slice(0, slash);
        if (!providerStates.has(name)) await providersNow(false);
        const st = providerStates.get(name);
        if (!st) throw httpError(404, `no provider ${name}: declare it in ${providersFile} (docs/companion-protocol.md §8)`, 'no_provider');
        if (st.state !== 'ok') throw httpError(503, `the provider ${name} is ${st.state}: ${st.why}`, 'provider_down');
        const id = slash < 0 ? '' : model.slice(slash + 1);
        if (st.entry.kind !== 'cli') { status = await passthrough(st.url, id, b, res, origin); return; }
        const reply = await cliChat(st, model, id, b);
        if (b.stream) {
          res.writeHead(200, { ...cors(origin), 'content-type': 'text/event-stream', 'cache-control': 'no-store' });
          const chunk = { ...reply, object: 'chat.completion.chunk', choices: [{ index: 0, delta: { role: 'assistant', content: reply.choices[0].message.content }, finish_reason: 'stop' }] };
          res.end(`data: ${JSON.stringify(chunk)}\n\ndata: [DONE]\n\n`);
        } else {
          send(res, 200, reply, origin);
        }
        return;
      }
      if (p === '/v1/store' && get) { send(res, 200, { texts: await storeRead() }, origin); return; }
      if (p === '/v1/store' && post) {
        const b = await readBody(req);
        await storeWrite(b.put || {}, b.remove || []);
        note = `${Object.keys(b.put || {}).length} put, ${(b.remove || []).length} removed`;
        send(res, 200, { ok: true }, origin);
        return;
      }
      status = 404;
      send(res, 404, { error: `no ${req.method} ${p}`, code: 'not_found' }, origin);
    } catch (e) {
      status = e.status || 500;
      if (!res.headersSent) send(res, status, { error: e.message, code: typeof e.code === 'string' && /^[a-z_]+$/.test(e.code) ? e.code : (CODES[status] || 'internal') }, origin);
      else res.end();
      note = e.message.slice(0, 80);
    } finally {
      done();
    }
  }

  const server = http.createServer((req, res) => { handle(req, res); });

  /// What this machine has, each found or missing with how to fix it: the
  /// companion's terminal prints these at start (docs/companion.md).
  async function checks() {
    const rt = await detectRuntimes();
    const out = [{ what: 'folder', name: 'folder', ok: true, line: `${folder} (state in ${DRY_DIR}/)` }];
    const { providers, errors: perr, exists } = declaredProviders();
    if (!exists && !providers.length) out.push({ what: 'providers', name: providersFile, ok: false, line: 'none declared', hint: `paste entries into ${providersFile} (the examples are in it), or add one in dry → Settings → Companion` });
    for (const e of perr) out.push({ what: 'providers', name: e.entry ?? 'providers.json', ok: false, line: e.error });
    const probed = await probeProviders();
    for (const r of probed) {
      const pr = publicProvider(r);
      out.push({ what: 'provider', name: pr.name, ok: pr.state === 'ok', state: pr.state, line: pr.state === 'ok' ? `${pr.kind}${pr.url ? ` ${pr.url}` : ''}: ${pr.models.length} model${pr.models.length === 1 ? '' : 's'}${pr.runner ? ` (${pr.runner}, ${pr.accel})` : ''}` : `${pr.kind}: ${pr.why}`, hint: pr.hint });
    }
    const { declared, errors } = loadDeclared();
    for (const d of declared) {
      const pb = await probeOf(d, true);
      out.push({ what: 'capability', name: d.id, ok: pb.state === 'present', state: pb.state === 'present' ? null : 'absent', line: pb.state === 'present' ? d.doc.split('\n')[0] : pb.why, hint: pb.state === 'present' ? '' : 'its probe says this machine lacks it' });
    }
    for (const e of errors) out.push({ what: 'capability', name: path.basename(e.file), ok: false, line: e.error, hint: 'fix the line it names' });
    out.push(rt.git ? { what: 'git', name: 'git', ok: true, line: rt.git.version || rt.git.path } : { what: 'git', name: 'git', ok: false, line: 'not on PATH', hint: 'install git (https://git-scm.com/downloads)' });
    const chrome = findChrome();
    out.push(chrome ? { what: 'chrome', name: 'browser', ok: typeof WebSocket === 'function', line: typeof WebSocket === 'function' ? chrome : `${chrome}, but Node ${process.version} has no WebSocket`, hint: typeof WebSocket === 'function' ? '' : 'use Node 22 or newer' }
      : { what: 'chrome', name: 'browser', ok: false, line: 'no Chrome, Chromium, Edge or Brave', hint: 'install Google Chrome or Chromium, or give --chrome PATH' });
    const ct = ['docker', 'podman'].find((n) => rt[n]);
    out.push(ct ? { what: 'container', name: ct, ok: true, line: rt[ct].version || rt[ct].path } : { what: 'container', name: 'container', ok: false, line: 'no docker or podman', hint: 'install Docker Desktop or Podman (optional: containers)' });
    return out;
  }

  /// Every capability with its switch, for the terminal's list.
  async function switchList() {
    const { caps } = await capabilities();
    return caps.map((c) => ({ id: c.id, on: isOn(c.id), actuator: actuator(c.id), saved: typeof config.switches[c.id] === 'boolean', doc: c.doc.split('\n')[0], state: c.off ? 'present' : c.state, why: c.off ? '' : c.why || '' }));
  }

  return {
    server,
    roots,
    folder,
    configFile,
    providersFile,
    pairCode: () => code,
    detectRuntimes,
    checks,
    switchList,
    setSwitch,
    saveSwitches: saveConfig,
    providers: providersNow,
    listen() {
      return new Promise((resolve, reject) => {
        server.once('error', reject);
        server.listen(port, '127.0.0.1', () => {
          server.off('error', reject);
          resolve(server.address().port);
        });
      });
    },
    announce() {
      const a = server.address();
      console.log(`${NAME} ${VERSION} on http://127.0.0.1:${a.port}  (protocol ${PROTOCOL}, node ${process.version})`);
      console.log(`listening http://127.0.0.1:${a.port}`);
      console.log(`folder: ${folder}`);
      if (opts.roots?.length) console.log(`roots: ${roots.join(', ')}`);
      console.log(`pages allowed: https://kaush4l.github.io, http://localhost:*, http://127.0.0.1:*${extraOrigins.length ? ', ' + extraOrigins.join(', ') : ''}`);
      if (appDir) console.log(`the app: http://localhost:${a.port}/app/  (from ${appDir})`);
      console.log(`MCP: http://localhost:${a.port}/mcp  (Bearer: a pairing token) · tools: ${loadTools().tools.length}`);
      console.log('type: pairings · revoke <id> · on|off <id> · switches · providers · code · tools · capabilities · stop-all   (Esc: stop all)');
      announceCode();
    },
    stopAll,
    pairingsText,
    capabilities,
    capTick,
    revoke,
    appDir,
    /// A line typed in the companion's terminal.
    command(line) {
      const [cmd, arg] = line.trim().split(/\s+/);
      switch (cmd) {
        case '': return '';
        case 'pairings': return pairingsText();
        case 'revoke': return revoke(arg || '') ? `  revoked ${arg}` : `  no pairing ${arg || '(give its id)'}`;
        case 'stop-all': { const r = stopAll(); return `  stopped ${r.stopped} session(s)${r.browser ? ', closed the browser' : ''}`; }
        case 'code': return `  pairing code: ${code}`;
        case 'on': case 'off': {
          if (!arg) return `  ${cmd} <id>: which capability (type: switches)`;
          return switchList().then((l) => { if (!l.some((x) => x.id === arg)) return `  no capability ${arg} (type: switches)`; setSwitch(arg, cmd === 'on'); return `  ${arg} ${cmd}`; });
        }
        case 'switches': return switchList().then((l) => l.map((x) => `  ${x.on ? 'on ' : 'off'}  ${x.id.padEnd(16)}${x.actuator ? ' actuator' : ''}${x.state !== 'present' ? ` (${x.state}: ${x.why})` : ''}`).join('\n'));
        case 'providers': return providersNow(true).then((v) => [`  ${v.file}${v.exists ? '' : ' (not there yet)'}`, ...v.providers.map((x) => `  ${x.state.padEnd(8)} ${x.name.padEnd(12)} ${x.kind}${x.state === 'ok' ? `: ${x.models.join(', ')}` : `: ${x.why}${x.hint ? ` → ${x.hint}` : ''}`}`), ...v.errors.map((e) => `  ! ${e.error}`)].join('\n'));
        case 'capabilities': return capabilitiesSummary(true).then((c) => [...c.list.map((x) => `  ${x.id.padEnd(10)} ${x.state.padEnd(12)}${x.declared ? ' declared' : ''}${x.why ? ' — ' + x.why : ''}${x.tools.length ? '\n             tools: ' + x.tools.join(', ') : ''}`), ...c.errors.map((e) => `  ! ${e.error}`)].join('\n'));
        case 'tools': { const t = loadTools(); return [...t.tools.map((x) => `  ${x.name}: ${x.description}`), ...t.errors.map((e) => `  ! ${e.file}: ${e.error}`)].join('\n') || '  no tools'; }
        default: return '  commands: pairings · revoke <id> · on <id> · off <id> · switches · providers · stop-all · code · tools · capabilities';
      }
    },
    close() {
      for (const s of sessions.values()) kill(s);
      closeBrowser();
      for (const st of providerStates.values()) stopRunner(st);
      for (const l of listeners) l.res.end();
      for (const r of mcpStreams) r.end();
      if (capTimer) clearInterval(capTimer);
      return new Promise((resolve) => { server.close(() => resolve()); server.closeAllConnections?.(); });
    },
  };
}

// ---- the command line ----

export function parseArgs(argv) {
  const o = { port: DEFAULT_PORT, roots: [], origins: [], on: [], off: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i], next = () => { const v = argv[++i]; if (v == null) throw new Error(`${a} needs a value`); return v; };
    if (a === '--port') o.port = Number(next());
    else if (a === '--folder') o.folder = next();
    else if (a === '--yes' || a === '-y') o.yes = true;
    else if (a === '--on') o.on.push(...next().split(',').filter(Boolean));
    else if (a === '--off') o.off.push(...next().split(',').filter(Boolean));
    else if (a === '--pair-code-file') o.pairCodeFile = next();
    else if (a === '--providers') o.providers = next();
    else if (a === '--config') o.config = next();
    else if (a === '--heartbeat') o.heartbeatMs = Math.max(100, Number(next()) * 1000);
    else if (a === '--verbose') o.verbose = true;
    else if (a === '--root') o.roots.push(next());
    else if (a === '--origin') o.origins.push(next());
    else if (a === '--lmstudio') o.lmstudio = next();
    else if (a === '--ollama') o.ollama = next();
    else if (a === '--store') o.store = next();
    else if (a === '--quiet') o.quiet = true;
    else if (a === '--app') o.app = next();
    else if (a === '--no-app') o.app = false;
    else if (a === '--tools') o.tools = next();
    else if (a === '--allow-url') (o.allowUrls ??= []).push(next());
    else if (a === '--chrome') o.chrome = next();
    else if (a === '--dry') o.dry = next();
    else if (a === '--ttl') o.ttlHours = Number(next());
    else if (a === '--capabilities') o.capabilities = next();
    else if (a === '--no-capabilities') o.capabilities = false;
    else if (a === '--help' || a === '-h') o.help = true;
    else throw new Error(`unknown option ${a} (see --help)`);
  }
  if (!Number.isInteger(o.port) || o.port < 0 || o.port > 65535) throw new Error('--port takes a port number');
  if (o.heartbeatMs != null && !Number.isFinite(o.heartbeatMs)) throw new Error('--heartbeat takes seconds');
  return o;
}

const HELP = `dry companion ${VERSION}: one folder of this machine (its files and a terminal), its models and tools, for the dry web app.

  node dry-companion.mjs [options]

  --folder DIR     the folder the page gets (default: the current one); dry keeps its state in DIR/.dry/
  --port N         listen on 127.0.0.1:N (default ${DEFAULT_PORT})
  --yes            take the saved switches (and the folder) without asking
  --on ID[,ID]     switch capabilities on (saved); --off ID[,ID] switches them off
  --providers FILE the declared model providers (default: DIR/.dry/providers.json, made from the template)
  --config FILE    where the switches are saved (default: DIR/.dry/companion.json)
  --pair-code-file FILE   write each new pairing code there (for scripts and the conformance suite)
  --heartbeat S    a ping on every open stream each S seconds (default 15)
  --capabilities DIR   capability files (<id>.md) this machine declares (default: ../capabilities next to this script)
  --no-capabilities    declare none
  --app DIR        serve the built app at / and /app/ (default: ../dist next to this script); --no-app: none
  --origin URL     one more page origin to answer (repeatable)
  --tools DIR      custom tools (*.md, *.json)
  --allow-url URL  a URL prefix the browser may open besides localhost (repeatable)
  --chrome PATH    the Chrome / Chromium to drive (default: found on this machine)
  --dry PATH       the dry binary that runs a project's evidence
  --ttl HOURS      how long a pairing lasts (default ${DEFAULT_TTL_HOURS})
  --root DIR       projects mode: a folder whose projects the page may use (repeatable)
  --store DIR      where the page's state goes (default: DIR/.dry/state)
  --verbose        a line for every read too; --quiet: no lines

While it runs: Esc stops everything it runs. Type pairings · revoke <id> · on <id> · off <id> · switches · providers · code · capabilities · stop-all

Then in dry: Settings → Companion, type the code printed below, Connect.`;

const mark = (ok, state) => (state && state !== 'ok' ? state : ok ? 'ok' : 'missing').padEnd(8);

/// Ask one line on the terminal (stdin is a terminal).
function ask(rl, question) {
  return new Promise((resolve) => rl.question(question, (a) => resolve(a.trim())));
}

async function main() {
  let o;
  try { o = parseArgs(process.argv.slice(2)); } catch (e) { console.error(e.message); process.exit(2); }
  if (o.help) { console.log(HELP); process.exit(0); }
  const major = Number(process.versions.node.split('.')[0]);
  if (major < 20) { console.error(`dry companion needs Node 20 or newer (this is ${process.version})`); process.exit(2); }
  const tty = !!process.stdin.isTTY;
  const rl = tty ? readline.createInterface({ input: process.stdin, output: process.stdout, terminal: true }) : null;
  const interactive = tty && !o.yes;
  // 1. The folder, confirmed.
  const folder = path.resolve((o.folder ?? process.cwd()).replace(/^~(?=$|\/)/, os.homedir()));
  console.log(`dry companion ${VERSION}`);
  if (interactive) {
    const a = await ask(rl, `the page gets this folder: ${folder}\n  its files, and a terminal in it (state in ${DRY_DIR}/). Use it? [Y/n] `);
    if (/^n/i.test(a)) { console.log('give the folder with --folder DIR'); process.exit(0); }
  } else console.log(`folder: ${folder}`);
  let c;
  try { c = createCompanion({ ...o, folder }); } catch (e) { console.error(e.message); process.exit(2); }
  if (o.on.length || o.off.length) c.saveSwitches();
  // A providers.json to paste into, made from the template on the first run.
  if (!fs.existsSync(c.providersFile)) {
    const v = (await c.providers(false)).template;
    try { fs.mkdirSync(path.dirname(c.providersFile), { recursive: true }); fs.writeFileSync(c.providersFile, JSON.stringify({ ...v, providers: v.providers ?? [] }, null, 2) + '\n'); console.log(`providers: made ${c.providersFile} from the template: paste entries into "providers"`); } catch (e) { console.log(`providers: could not make ${c.providersFile}: ${e.message}`); }
  }
  // 2. The checks: each found or missing, with the fix.
  console.log('\nchecks');
  for (const k of await c.checks()) console.log(`  ${mark(k.ok, k.state)} ${k.what.padEnd(10)} ${k.name === k.what ? '' : `${k.name}: `}${k.line}${!k.ok && k.hint ? `\n             → ${k.hint}` : ''}`);
  // 3. The switches: what the page may use (actuators start off).
  for (;;) {
    const list = await c.switchList();
    console.log(`\nswitches (saved in ${c.configFile}; actuators start off)`);
    list.forEach((x, n) => console.log(`  ${String(n + 1).padStart(2)}  ${x.on ? 'on ' : 'off'}  ${x.id.padEnd(16)} ${x.actuator ? '(actuator) ' : ''}${x.state !== 'present' ? `[${x.state}] ` : ''}${x.doc.slice(0, 70)}`));
    if (!interactive) break;
    const a = await ask(rl, 'flip some (their numbers, e.g. "2 5"), or Enter to go on: ');
    if (!a) { c.saveSwitches(); break; }
    for (const w of a.split(/[\s,]+/)) { const x = list[Number(w) - 1]; if (x) c.setSwitch(x.id, !x.on); }
  }
  // 4. Listening, the pairing code, then live lines.
  try { await c.listen(); } catch (e) { console.error(`could not listen on 127.0.0.1:${o.port}: ${e.message}`); process.exit(1); }
  console.log('');
  c.announce();
  let stopping = false;
  const stop = () => { if (stopping) return; stopping = true; console.log('\nstopping'); c.close().then(() => process.exit(0)); };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  if (rl) {
    rl.on('line', async (line) => { const out = await c.command(line); if (out) console.log(out); });
    rl.on('close', stop);
    // One key stops everything it runs: Esc.
    process.stdin.on('keypress', (_, key) => { if (key?.name === 'escape') console.log(c.command('stop-all')); });
  } else {
    process.stdin.on('end', () => {});
    process.stdin.resume();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href) main();
