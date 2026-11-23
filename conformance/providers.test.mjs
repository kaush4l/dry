// Conformance §8: providers.json, declared and probed, never scanned; the
// models through /v1/models, the handshake and a chat; nothing downloaded.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { start, fakeModels, freePort, answers, until, FIXTURES } from './harness.mjs';

let c, fake, deadUrl, runnerPort;
const runner = path.join(FIXTURES, 'fake-runner.mjs');
const cli = path.join(FIXTURES, 'fake-cli.mjs');
before(async () => {
  fake = await fakeModels(['m1', 'm2']);
  deadUrl = `http://127.0.0.1:${await freePort()}`;
  runnerPort = await freePort();
  let model;
  c = await start({
    name: 'providers',
    setup: ({ tmp, providersFile }) => {
      model = path.join(tmp, 'tiny.gguf');
      fs.writeFileSync(model, 'not really weights');
      fs.writeFileSync(providersFile, JSON.stringify({ version: 1, providers: [
        { name: 'fake', kind: 'port', url: `${fake.url}/v1/` },
        { name: 'dead', kind: 'port', url: deadUrl },
        { name: 'ghost', kind: 'cli', cli: 'claude', path: '/nonexistent/dry-conformance/claude' },
        { name: 'mycli', kind: 'cli', cli: 'copilot', path: cli },
        { name: 'nat', kind: 'native', runner: 'llama.cpp', path: runner, model, port: runnerPort },
        { name: 'nomodel', kind: 'native', runner: 'mlx', path: runner, model: 'dry-conformance/never-downloaded-7b' },
        { name: 'norunner', kind: 'native', runner: 'llama.cpp', path: '/nonexistent/dry-conformance/llama-server', model },
        { name: 'BAD NAME', kind: 'port', url: 'http://127.0.0.1:1' },
        { name: 'nokind', kind: 'cloud' },
      ] }, null, 2));
    },
  });
  await c.pair();
});
after(async () => { await c?.stop(); await fake?.close(); });

const byName = async (refresh = false) => Object.fromEntries((await c.call('GET', `/v1/providers${refresh ? '?refresh=1' : ''}`)).json.providers.map((p) => [p.name, p]));

test('each declared entry is probed: ok with its models, failed or absent with why and a hint', async () => {
  const r = await c.call('GET', '/v1/providers');
  assert.equal(r.status, 200, r.text);
  assert.equal(r.json.file, c.providersFile);
  assert.ok(Array.isArray(r.json.template.providers), 'the template is in the answer');
  const p = await byName();
  assert.deepEqual([p.fake.state, p.fake.models, p.fake.url], ['ok', ['fake/m1', 'fake/m2'], fake.url], 'a port: GET <url>/v1/models (a trailing /v1 dropped)');
  assert.equal(p.dead.state, 'failed');
  assert.match(p.dead.why, /v1\/models/);
  assert.equal(p.ghost.state, 'absent');
  assert.match(p.ghost.hint, /install/i, 'the hint says how to get it');
  assert.deepEqual([p.mycli.state, p.mycli.models], ['ok', ['mycli/auto']], 'a cli by path, answering --version');
  assert.equal(p.nomodel.state, 'failed');
  assert.match(p.nomodel.why, /never downloads/);
  assert.equal(p.norunner.state, 'absent');
  assert.match(p.norunner.hint, /llama/i);
  assert.ok(r.json.errors.some((e) => /not a provider name/.test(e.error)), 'a bad name is listed with why');
  assert.ok(r.json.errors.some((e) => /kind is one of/.test(e.error)));
});

test('a native runner is started on a local port and registered as a port', async () => {
  const p = await until(async () => { const x = (await byName()).nat; return x.state === 'ok' && x; }, 'the native runner to be ok', 30000);
  assert.equal(p.url, `http://127.0.0.1:${runnerPort}`);
  assert.deepEqual(p.models, ['nat/runner-model']);
  assert.equal(p.runner, 'llama.cpp');
  assert.ok(await answers(runnerPort));
});

test('/v1/models lists the ok providers\' models, as <name>/<model>', async () => {
  const ids = (await c.call('GET', '/v1/models')).json.data.map((m) => m.id);
  for (const id of ['fake/m1', 'fake/m2', 'mycli/auto', 'nat/runner-model']) assert.ok(ids.includes(id), id);
  assert.ok(!ids.some((id) => id.startsWith('dead/') || id.startsWith('ghost/')));
});

test('a chat goes to its provider: passed through to a port or a runner, run by a cli', async () => {
  const chat = (model) => c.call('POST', '/v1/chat/completions', { model, messages: [{ role: 'user', content: 'ping' }] });
  const viaPort = await chat('fake/m1');
  assert.equal(viaPort.status, 200, viaPort.text);
  assert.equal(viaPort.json.choices[0].message.content, 'pong (m1)', 'the model id without the provider');
  const viaRunner = await chat('nat/runner-model');
  assert.equal(viaRunner.json.choices[0].message.content, 'runner says hi (runner-model)');
  const viaCli = await chat('mycli/auto');
  assert.equal(viaCli.status, 200, viaCli.text);
  assert.match(viaCli.json.choices[0].message.content, /cli says hi to: ping/);
  const none = await chat('zzz/x');
  assert.deepEqual([none.status, none.json.code], [404, 'no_provider']);
  const down = await chat('dead/x');
  assert.deepEqual([down.status, down.json.code], [503, 'provider_down']);
});

test('each provider is a model.<name> capability through the handshake', async () => {
  const { resources } = (await c.rpc('resources/list')).result;
  const r = (id) => resources.find((x) => x._meta.capability === id);
  assert.equal(r('model.fake')._meta.state, 'present');
  assert.equal((await c.rpc('resources/read', { uri: 'dry://capability/model.fake' })).result.contents[0].text, 'fake/m1\nfake/m2');
  assert.equal(r('model.dead')._meta.state, 'absent');
  assert.match(r('model.dead')._meta.why, /failed/);
  assert.match(r('model.ghost')._meta.why, /install/i);
});

test('add a provider from the page: checked, written into the file, probed; a taken name is 409', async () => {
  const add = await c.call('POST', '/v1/providers', { provider: { name: 'fake2', kind: 'port', url: fake.url } });
  assert.equal(add.status, 200, add.text);
  assert.deepEqual([add.json.provider.state, add.json.provider.models], ['ok', ['fake2/m1', 'fake2/m2']]);
  const file = JSON.parse(fs.readFileSync(c.providersFile, 'utf8'));
  assert.ok(file.providers.some((p) => p.name === 'fake2' && p.url === fake.url));
  const again = await c.call('POST', '/v1/providers', { provider: { name: 'fake2', kind: 'port', url: fake.url } });
  assert.deepEqual([again.status, again.json.code], [409, 'exists']);
  for (const bad of [{ name: 'x', kind: 'port', url: 'ftp://nope' }, { name: 'x', kind: 'native', runner: 'mlx' }, { name: 'x', kind: 'cli', cli: 'gemini' }, { name: 'Bad', kind: 'port', url: fake.url }]) {
    const r = await c.call('POST', '/v1/providers', { provider: bad });
    assert.deepEqual([r.status, r.json.code], [400, 'bad_provider'], JSON.stringify(bad));
  }
  assert.ok((await c.call('GET', '/v1/models')).json.data.some((m) => m.id === 'fake2/m1'));
});

test('the runner it started stops with the companion', async () => {
  assert.ok(await answers(runnerPort));
  await c.stop();
  await until(async () => !(await answers(runnerPort)), 'the runner to stop', 10000);
  c = null;
});
