// node --test companion/   Capabilities (docs/capabilities.md): the declared
// files read the way dry_core reads them, the handshake over MCP (hello,
// tools grouped by _meta.capability, resources), a declared tool called
// with its inputs as whole arguments, and the stream: a snapshot's event
// and a changed folder (list_changed).
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createCompanion, parseCapability, capabilityArgv, parseWhen, whenHolds, snapshotValue } from './dry-companion.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
let c, base, token, tmp, caps;

async function rpc(method, params = {}, id = 1) {
  const r = await fetch(`${base}/mcp`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', authorization: `Bearer ${token}` },
    body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
  });
  return r.json();
}

before(async () => {
  tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dry-companion-caps-')));
  const root = path.join(tmp, 'root');
  fs.mkdirSync(path.join(root, 'proj'), { recursive: true });
  fs.writeFileSync(path.join(root, 'proj', 'package.json'), '{}');
  caps = path.join(tmp, 'capabilities');
  fs.mkdirSync(caps);
  fs.writeFileSync(path.join(caps, 'flag.txt'), 'off\n');
  // A capability whose snapshot is a file the test flips: its event fires when it says ON.
  fs.writeFileSync(path.join(caps, 'flag.md'), '# flag\nA flag in a file: on or off.\n\nsnapshot: cat flag.txt\ninterval: 1s\n\n## tool: say\nSay it back.\ncommand: printf %s {text}\ninputs:\n- text (string, required): what to say\n\n## tool: flip\nTurn it on.\napproval: yes\ncommand: cp on.txt flag.txt\n\n## event: on\nwhen: contains ON\nThe flag went on.\n');
  fs.writeFileSync(path.join(caps, 'gone.md'), '# gone\nSomething this machine has not got.\n\nprobe: dry-no-such-program-here\n');
  fs.writeFileSync(path.join(caps, 'broken.md'), '# broken\ndoc\n## tool: x\ncommand: sh -c {code}\ninputs:\n- code: it\n');
  c = createCompanion({ port: 0, roots: [root], folder: root, on: ['flag', 'terminal'], config: path.join(tmp, 'companion.json'), quiet: true, app: false, capabilities: caps, store: path.join(tmp, 'store') });
  base = `http://127.0.0.1:${await c.listen()}`;
  const r = await fetch(`${base}/v1/pair`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ code: c.pairCode() }) });
  token = (await r.json()).token;
});

after(async () => {
  await c.close();
  fs.rmSync(tmp, { recursive: true, force: true });
});

test('the shipped capability files read here as they read in dry_core', () => {
  for (const f of ['battery', 'gpio', 'sensors']) {
    const d = parseCapability(`${f}.md`, fs.readFileSync(path.join(HERE, '..', 'capabilities', `${f}.md`), 'utf8'));
    assert.equal(d.id, f);
  }
  const gpio = parseCapability('gpio.md', fs.readFileSync(path.join(HERE, '..', 'capabilities', 'gpio.md'), 'utf8'));
  assert.deepEqual(gpio.tools.map((t) => [t.name, t.approval]), [['read', false], ['set', true]]);
  const err = (t) => { try { parseCapability('x.md', t); return null; } catch (e) { return e.message; } };
  assert.equal(err('battery\n'), 'x.md:1: the file starts with `# <id>`, the capability\'s id');
  assert.equal(err('# a\ndoc\n## tool: t\nno command\n'), 'x.md:3: tool t has no `command:`');
  assert.equal(err('# a\ndoc\n## tool: x\ncommand: echo hi={v}\ninputs:\n- v: it\n'), 'x.md:4: "hi={v}": an input goes in as a whole argument ({name}), never inside one');
  assert.equal(err('# a\ndoc\n## tool: x\ncommand: sh -c {v}\ninputs:\n- v: it\n'), 'x.md:4: an input cannot follow -c: it would run as code');
  assert.equal(err('# a\ndoc\n## tool: x\ncommand: {v}\ninputs:\n- v: it\n'), 'x.md:4: the program itself cannot be an input');
});

test('a call becomes an argv: each input one whole argument and DRY_INPUT_<NAME>', () => {
  const d = parseCapability('t.md', '# t\ndoc\n## tool: greet\ncommand: echo hello {who}\ninputs:\n- who (string, required): who\n- n (integer): times\n');
  const attack = 'Ada; touch /tmp/x $(rm -rf ~)';
  const a = capabilityArgv(d, 'greet', { who: attack });
  assert.deepEqual(a.argv, ['echo', 'hello', attack]);
  assert.equal(a.env.DRY_INPUT_WHO, attack);
  assert.throws(() => capabilityArgv(d, 'greet', {}), /needs who/);
  assert.throws(() => capabilityArgv(d, 'greet', { who: 'a', n: '2; ls' }), /whole number/);
  assert.throws(() => capabilityArgv(d, 'greet', { who: 'a', x: 1 }), /has no input x/);
  assert.equal(whenHolds(parseWhen('temp > 70'), "temp=71.2'C"), true);
  assert.equal(whenHolds(parseWhen('temp > 70'), "temp=48.3'C"), false);
  assert.equal(snapshotValue('a: 1; state: charged', 'state'), 'charged');
});

test('the handshake: hello both ways, tools grouped by _meta.capability, one resource per capability', async () => {
  const hello = { runtime: 'test', platform: 'node', version: '1', protocol: 'dry-capabilities/1' };
  const init = await rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'test', version: '1' }, _meta: { hello } });
  assert.equal(init.result._meta.hello.runtime, 'dry-companion');
  assert.equal(init.result._meta.hello.protocol, 'dry-capabilities/1');
  assert.equal(init.result.capabilities.tools.listChanged, true);
  assert.match(c.pairingsText(), /\[test node\]/, 'the peer\'s hello is kept on its pairing');
  const tools = (await rpc('tools/list')).result.tools;
  const by = (n) => tools.find((t) => t.name === n);
  assert.equal(by('flag.say')._meta.capability, 'flag');
  assert.equal(by('flag.say')._meta.approval, false);
  assert.equal(by('flag.flip')._meta.approval, true, 'an actuator says so');
  assert.equal(by('terminal.run')._meta.capability, 'terminal');
  assert.equal(by('browser_open')._meta.capability, 'browser');
  assert.ok(!tools.some((t) => t.name.startsWith('broken.')), 'a file that does not read brings nothing');
  const res = (await rpc('resources/list')).result.resources;
  const r = (id) => res.find((x) => x._meta.capability === id);
  assert.equal(r('flag').uri, 'dry://capability/flag');
  assert.deepEqual([r('flag')._meta.state, r('flag')._meta.interval], ['present', 1]);
  assert.deepEqual(r('flag')._meta.events, [{ name: 'on', when: 'contains ON', text: 'The flag went on.' }]);
  assert.equal(r('gone')._meta.state, 'absent');
  assert.match(r('gone')._meta.why, /no such program/);
  for (const id of ['terminal', 'file', 'git', 'container', 'browser', 'evidence']) assert.ok(r(id), id);
  const read = await rpc('resources/read', { uri: 'dry://capability/flag' });
  assert.equal(read.result.contents[0].text, 'off');
  const summary = (await (await fetch(`${base}/v1/capabilities`, { headers: { 'x-dry-token': token } })).json()).capabilities;
  assert.ok(summary.errors.some((e) => /broken\.md:4: an input cannot follow -c/.test(e.error)), 'the broken file is listed with its line');
});

test('a declared tool runs with its inputs as whole arguments; a missing capability says why', async () => {
  const attack = 'x; touch /tmp/dry-cap-attack $(id)';
  const said = await rpc('tools/call', { name: 'flag.say', arguments: { text: attack } });
  assert.equal(said.result.isError, false);
  assert.equal(said.result.content[0].text, attack);
  assert.equal(fs.existsSync('/tmp/dry-cap-attack'), false);
  const bad = await rpc('tools/call', { name: 'flag.say', arguments: {} });
  assert.equal(bad.result.isError, true);
  assert.match(bad.result.content[0].text, /needs text/);
});

test('the stream: a snapshot\'s event rides on resources/updated, a changed folder is list_changed', async () => {
  const ctl = new AbortController();
  const resp = await fetch(`${base}/mcp`, { headers: { accept: 'text/event-stream', authorization: `Bearer ${token}` }, signal: ctl.signal });
  assert.equal(resp.status, 200);
  const reader = resp.body.getReader();
  const seen = [];
  let buf = '';
  let pending = null;
  const until = async (pred, ms = 8000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const found = seen.find(pred);
      if (found) return found;
      pending ??= reader.read();
      const got = await Promise.race([pending, new Promise((r) => setTimeout(() => r(null), 200))]);
      if (!got) continue;
      pending = null;
      const { value, done } = got;
      if (done) break;
      if (value) {
        buf += Buffer.from(value).toString();
        let at;
        while ((at = buf.indexOf('\n\n')) >= 0) {
          const chunk = buf.slice(0, at);
          buf = buf.slice(at + 2);
          const data = chunk.split('\n').find((l) => l.startsWith('data:'));
          if (data) seen.push(JSON.parse(data.slice(5)));
        }
      }
    }
    return seen.find(pred);
  };
  await new Promise((r) => setTimeout(r, 1500)); // the first reading (never fires)
  fs.writeFileSync(path.join(caps, 'flag.txt'), 'ON\n');
  const ev = await until((m) => m.method === 'notifications/resources/updated' && m.params._meta?.event);
  assert.deepEqual(ev.params, { uri: 'dry://capability/flag', _meta: { event: { name: 'on', text: 'The flag went on.' } } });
  fs.writeFileSync(path.join(caps, 'more.md'), '# more\nOne more thing.\n');
  const changed = await until((m) => m.method === 'notifications/tools/list_changed');
  assert.ok(changed);
  ctl.abort();
});
