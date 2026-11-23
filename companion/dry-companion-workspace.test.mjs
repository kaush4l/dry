// The companion's own pieces for the folder workspace and providers.json
// (the wire contract itself is checked from outside by conformance/).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { checkProvider, parseProviders, chooseRunner, runnerArgv, folderParts, createCompanion, parseArgs, PROVIDERS_TEMPLATE } from './dry-companion.mjs';

test('folderParts: relative parts, or why not (path_outside, secret_path, dry_state)', () => {
  assert.deepEqual(folderParts('').parts, []);
  assert.deepEqual(folderParts('./src//a/../b.rs').parts, ['src', 'b.rs']);
  assert.deepEqual(folderParts('src\\win\\x.txt').parts, ['src', 'win', 'x.txt']);
  for (const p of ['..', '../x', 'a/../../x', '/etc/passwd', 'C:/x', 'c:\\x']) assert.equal(folderParts(p).code, 'path_outside', p);
  for (const p of ['.env', 'a/.env.local', 'id_ed25519', 'k.pem', '.ssh/config']) assert.equal(folderParts(p).code, 'secret_path', p);
  assert.equal(folderParts('.dry/state/x').code, 'dry_state');
  assert.equal(folderParts('a\0b').code, 'bad_request');
});

test('checkProvider: each kind checked and normalised; parseProviders keeps the good ones and says why for the rest', () => {
  assert.deepEqual(checkProvider({ name: 'lm', kind: 'port', url: 'http://127.0.0.1:1234/v1/' }), { name: 'lm', kind: 'port', url: 'http://127.0.0.1:1234' });
  assert.deepEqual(checkProvider({ name: 'claude', kind: 'cli' }), { name: 'claude', kind: 'cli', cli: 'claude' });
  assert.deepEqual(checkProvider({ name: 'local', kind: 'native', model: 'org/m' }), { name: 'local', kind: 'native', model: 'org/m', runner: 'auto' });
  for (const bad of [{ name: 'A', kind: 'cli' }, { name: 'x', kind: 'cloud' }, { name: 'x', kind: 'cli', cli: 'gemini' }, { name: 'x', kind: 'port', url: 'file:///x' },
    { name: 'x', kind: 'native' }, { name: 'x', kind: 'native', model: 'm', runner: 'vllm' }, { name: 'x', kind: 'native', model: 'm', port: 80 }, { name: 'x', kind: 'cli', cli: 'claude', path: 'relative/claude' }]) {
    assert.throws(() => checkProvider(bad), undefined, JSON.stringify(bad));
  }
  const r = parseProviders(JSON.stringify({ providers: [{ name: 'a', kind: 'cli', cli: 'codex' }, { name: 'a', kind: 'cli', cli: 'codex' }, { name: 'b' }] }));
  assert.deepEqual(r.providers.map((p) => p.name), ['a']);
  assert.equal(r.errors.length, 2);
  assert.match(parseProviders('{').errors[0].error, /not JSON/);
  assert.equal(parseProviders(JSON.stringify(PROVIDERS_TEMPLATE)).providers.length, 0, 'the template declares nothing');
  const shipped = JSON.parse(fs.readFileSync(new URL('../providers.json', import.meta.url), 'utf8'));
  for (const e of shipped.examples) checkProvider(e);
  assert.deepEqual(shipped.providers, []);
});

test('chooseRunner by hardware; runnerArgv never downloads', () => {
  assert.equal(chooseRunner('auto', { platform: 'darwin', arch: 'arm64' }).runner, 'mlx');
  assert.deepEqual(chooseRunner('auto', { platform: 'linux', arch: 'x64', nvidia: true }), { runner: 'llama.cpp', accel: 'cuda' });
  assert.deepEqual(chooseRunner('auto', { platform: 'win32', arch: 'x64', nvidia: false }), { runner: 'llama.cpp', accel: 'cpu' });
  assert.equal(chooseRunner('llama.cpp', { platform: 'darwin', arch: 'arm64' }).runner, 'llama.cpp');
  assert.deepEqual(runnerArgv('mlx', 'mlx_lm.server', 'org/m', 9000), ['mlx_lm.server', '--model', 'org/m', '--host', '127.0.0.1', '--port', '9000']);
  const ll = runnerArgv('llama.cpp', 'llama-server', '/m.gguf', 9001);
  assert.deepEqual(ll, ['llama-server', '-m', '/m.gguf', '--host', '127.0.0.1', '--port', '9001']);
  assert.ok(!ll.includes('-hf') && !ll.includes('--hf-repo'));
});

test('the command line: folder, switches, the code file', () => {
  const o = parseArgs(['--folder', '/tmp/x', '--yes', '--on', 'terminal,gpio', '--off', 'browser', '--pair-code-file', '/tmp/c', '--heartbeat', '2']);
  assert.deepEqual([o.folder, o.yes, o.on, o.off, o.pairCodeFile, o.heartbeatMs], ['/tmp/x', true, ['terminal', 'gpio'], ['browser'], '/tmp/c', 2000]);
  assert.throws(() => parseArgs(['--port', 'x']));
});

test('switches: actuators start off, a choice is saved and read back; the whole disk is refused', async () => {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dry-cw-')));
  const config = path.join(tmp, 'companion.json');
  const caps = path.join(tmp, 'caps');
  fs.mkdirSync(caps);
  fs.writeFileSync(path.join(caps, 'pin.md'), '# pin\nA pin.\n\n## tool: set\nSet it.\ncommand: true\napproval: yes\n');
  const a = createCompanion({ port: 0, folder: tmp, config, capabilities: caps, quiet: true, app: false });
  const l = await a.switchList();
  const by = (id) => l.find((x) => x.id === id);
  assert.deepEqual([by('terminal').on, by('terminal').actuator], [false, true]);
  assert.deepEqual([by('pin').on, by('pin').actuator], [false, true]);
  assert.deepEqual([by('file').on, by('file').actuator], [true, false]);
  a.setSwitch('terminal', true);
  assert.deepEqual(JSON.parse(fs.readFileSync(config, 'utf8')).switches, { terminal: true });
  const b = createCompanion({ port: 0, folder: tmp, config, capabilities: caps, quiet: true, app: false, off: ['file'] });
  const l2 = await b.switchList();
  assert.equal(l2.find((x) => x.id === 'terminal').on, true, 'the saved choice');
  assert.equal(l2.find((x) => x.id === 'file').on, false, '--off for this run');
  assert.throws(() => createCompanion({ port: 0, folder: '/', quiet: true, app: false }), /whole disk/);
  await a.close();
  await b.close();
  fs.rmSync(tmp, { recursive: true, force: true });
});
