"""The Python companion's pure parts: the capability parser (the same cases and
messages as dry_core's crates/core/src/capability/tests.rs), argv safety, the
folder's path rules, providers.json entries, conditions.

    python3 -m unittest companion/test_dry_companion.py

The wire contract is checked from outside by conformance/ (docs/companion-protocol.md §13).
"""

import os
import sys
import unittest

sys.dont_write_bytecode = True  # no __pycache__ beside the companion
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dry_companion as dc  # noqa: E402

CAPS = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'capabilities')


def read(name):
    with open(os.path.join(CAPS, name), encoding='utf-8') as fh:
        return fh.read()


def err(text, file='x.md'):
    try:
        dc.parse_capability(file, text)
    except dc.CapError as e:
        return str(e)
    raise AssertionError('it read: %r' % text)


class Parser(unittest.TestCase):
    def test_the_example_files_read(self):
        b = dc.parse_capability('battery.md', read('battery.md'))
        self.assertEqual((b['id'], b['interval'], len(b['tools']), len(b['events'])), ('battery', 60, 1, 2))
        self.assertEqual(b['probe'], ['pmset', '-g', 'batt'])
        self.assertFalse(any(t['approval'] for t in b['tools']))
        self.assertEqual(b['tools'][0]['timeout'], 10)
        g = dc.parse_capability('gpio.md', read('gpio.md'))
        self.assertEqual([t['name'] for t in g['tools'] if t['approval']], ['set'], 'setting a pin needs approval')
        self.assertEqual(g['tools'][1]['command'], [('text', 'pinctrl'), ('text', 'set'), ('input', 'pin'), ('text', 'op'), ('input', 'level')])
        self.assertEqual(g['interval'], 10)
        self.assertEqual(g['events'][0]['when'], {'kind': 'contains', 'text': '| lo // GPIO17'})
        s = dc.parse_capability('sensors.md', read('sensors.md'))
        self.assertEqual(s['events'][0]['when'], {'kind': 'compare', 'key': 'temp', 'op': '>', 'value': '70'})

    def test_a_broken_file_says_its_line(self):
        self.assertEqual(err('battery\n'), "x.md:1: the file starts with `# <id>`, the capability's id")
        self.assertEqual(err('# Bad Id\n'), 'x.md:1: `Bad Id` is not a capability id (lower case letters, digits, - and _)')
        self.assertEqual(err('# a\ndoc\n\n## widget: x\n'), 'x.md:4: `## widget: x`: a section is `## tool: <name>` or `## event: <name>`')
        self.assertEqual(err('# a\ndoc\n## tool: t\nno command\n'), 'x.md:3: tool t has no `command:`')
        self.assertEqual(err('# a\ndoc\nsnapshot: cat x\n## event: e\nwhen: temp ~ 3\n').split(': ')[0], 'x.md:5')
        self.assertEqual(err('# a\ndoc\n## tool: t\ncommand: x\napproval: maybe\n'), 'x.md:5: approval is yes or no, not `maybe`')
        self.assertEqual(err('# a\ndoc\n## tool: t\ncommand: x {n}\ninputs:\n- n (colour): what\n'),
                         "x.md:6: `colour`: an input's flags are its type (string, integer, number, boolean) and `required`")
        self.assertEqual(err('# a\n'), 'x.md:1: say what it is and how to use it, under the `# id` line')
        self.assertEqual(err('# a\ndoc\n## event: e\nwhen: contains x\n'), 'x.md:1: events are read from the snapshot: add a `snapshot:` command')
        self.assertEqual(err('# a\ndoc\ninterval: 0s\nsnapshot: x\n'), 'x.md:3: `0s` is not a duration (60s, 5m)')
        self.assertEqual(err('# a\ndoc\ninterval: 5s\n'), 'x.md:1: `interval:` without a `snapshot:` command')

    def test_inputs_go_in_as_whole_arguments_never_through_a_shell(self):
        d = dc.parse_capability('t.md', '# t\ndoc\n## tool: greet\ncommand: echo hello {who}\ninputs:\n- who (string, required): who\n- n (integer): times\n')
        attack = 'Ada; touch /tmp/x $(rm -rf ~) `id`'
        argv, env, timeout = dc.capability_argv(d, 'greet', {'who': attack})
        self.assertEqual(argv, ['echo', 'hello', attack], 'one whole argument, as given')
        self.assertEqual(env['DRY_INPUT_WHO'], attack)
        self.assertEqual(env['DRY_TOOL'], 't.greet')
        self.assertEqual(timeout, 30)

        def call_err(args):
            with self.assertRaises(dc.HttpError) as cm:
                dc.capability_argv(d, 'greet', args)
            return cm.exception.message

        self.assertEqual(call_err({}), 't.greet needs who="..."')
        self.assertEqual(call_err({'who': 'a', 'n': '2; ls'}), 'n must be a whole number')
        self.assertEqual(call_err({'who': 'a', 'x': '1'}), 't.greet has no input x; its inputs: who, n')
        self.assertEqual(call_err({'who': 'a\0b'}), 'who holds a zero byte')
        self.assertEqual(dc.capability_argv(d, 'greet', {'who': 'a', 'n': 3})[0], ['echo', 'hello', 'a'], 'an input with no placeholder in the command is no argument')

        def refuse(cmd):
            return err('# t\ndoc\n## tool: x\ncommand: %s\ninputs:\n- v (string): it\n' % cmd, 't.md')

        self.assertEqual(refuse('echo hi={v}'), 't.md:4: "hi={v}": an input goes in as a whole argument ({name}), never inside one')
        self.assertEqual(refuse('sh -c {v}'), 't.md:4: an input cannot follow -c: it would run as code')
        self.assertEqual(refuse('python3 -c {v}'), 't.md:4: an input cannot follow -c: it would run as code')
        self.assertEqual(refuse('{v} --help'), 't.md:4: the program itself cannot be an input')
        self.assertEqual(refuse('echo {w}'), 't.md:4: {w} is not one of its inputs')
        with self.assertRaises(dc.CapError):
            dc.parse_capability('t.md', '# t\ndoc\nsnapshot: cat {v}\n')
        j = dc.parse_capability('t.md', '# t\ndoc\n## tool: x\ncommand: ["say", "two words", "{v}"]\ninputs:\n- v: it\n')
        self.assertEqual(dc.capability_argv(j, 'x', {'v': 'hi'})[0], ['say', 'two words', 'hi'])

    def test_conditions(self):
        w = dc.parse_when
        self.assertTrue(dc.when_holds(w('temp > 70'), "temp=71.2'C"))
        self.assertFalse(dc.when_holds(w('temp > 70'), "temp=48.3'C"))
        self.assertTrue(dc.when_holds(w('state = charged'), 'x: 1\nstate: charged'))
        self.assertTrue(dc.when_holds(w('missing AC'), 'Battery Power'))
        self.assertEqual(dc.value_of('a: 1; b=2, c: 3', 'b'), '2')
        self.assertIsNone(dc.value_of('xb=2', 'b'), 'a key starts a word')
        with self.assertRaises(dc.CapError):
            w('temp > hot')
        self.assertEqual(dc.cap_duration('500ms'), 1)
        self.assertEqual(dc.cap_duration('2m'), 120)
        self.assertIsNone(dc.cap_duration('soon'))


class Rules(unittest.TestCase):
    def test_folder_paths(self):
        self.assertEqual(dc.folder_parts('src/./deep/../b.rs'), (['src', 'b.rs'], None))
        self.assertEqual(dc.folder_parts(''), ([], None))
        for p in ('../x', '/etc/passwd', 'C:\\Windows', 'a/../../b'):
            self.assertEqual(dc.folder_parts(p)[1][1], 'path_outside', p)
        for p in ('.env', 'src/.env.local', 'id_rsa', 'a/deploy.pem', '.aws/config', 'new/.npmrc'):
            self.assertEqual(dc.folder_parts(p)[1][1], 'secret_path', p)
        self.assertEqual(dc.folder_parts('.dry/state/x')[1][1], 'dry_state')

    def test_commands_and_keys(self):
        for c in ('git push origin main', 'cd x && git push', 'git -C repo push', 'gh pr create', 'echo; git clone u'):
            self.assertTrue(dc.needs_confirm(c), c)
        for c in ('git status', 'echo push', 'gitpush'):
            self.assertFalse(dc.needs_confirm(c), c)
        self.assertTrue(dc.secret_env('DRY_CONF_API_TOKEN'))
        self.assertFalse(dc.secret_env('PATH'))
        self.assertTrue(dc.store_path_ok('engines/e1/spec/goal.md'))
        for bad in ('../x.md', '/abs.md', 'a/../../b.md', 'a//b.md', ''):
            self.assertFalse(dc.store_path_ok(bad), bad)

    def test_origins_and_hosts(self):
        self.assertTrue(dc.origin_allowed('http://localhost:5173'))
        self.assertTrue(dc.origin_allowed('https://kaush4l.github.io'))
        for o in ('null', 'https://evil.example', 'http://localhost.evil.example', 'http://192.168.1.5:8080', None):
            self.assertFalse(dc.origin_allowed(o), o)
        self.assertTrue(dc.host_allowed('LOCALHOST:7777', 7777))
        self.assertFalse(dc.host_allowed('localhost', 7777))

    def test_providers(self):
        self.assertEqual(dc.check_provider({'name': 'lm', 'kind': 'port', 'url': 'http://127.0.0.1:1234/v1/'})['url'], 'http://127.0.0.1:1234')
        self.assertEqual(dc.check_provider({'name': 'c', 'kind': 'cli', 'cli': 'claude'})['cli'], 'claude')
        self.assertEqual(dc.check_provider({'name': 'n', 'kind': 'native', 'model': 'm/x'})['runner'], 'auto')
        for bad, why in (({'name': 'Bad', 'kind': 'port', 'url': 'http://x'}, 'not a provider name'),
                         ({'name': 'x', 'kind': 'cloud'}, 'kind is one of'),
                         ({'name': 'x', 'kind': 'port', 'url': 'ftp://nope'}, 'http:// or https://'),
                         ({'name': 'x', 'kind': 'native', 'runner': 'mlx'}, 'needs a model'),
                         ({'name': 'x', 'kind': 'cli', 'cli': 'gemini'}, 'cli is one of'),
                         ({'name': 'x', 'kind': 'cli', 'path': 'rel/claude'}, 'absolute path'),
                         ({'name': 'x', 'kind': 'native', 'model': 'm', 'port': 80}, '1024-65535')):
            with self.assertRaises(ValueError) as cm:
                dc.check_provider(bad)
            self.assertIn(why, str(cm.exception))
        providers, errors = dc.parse_providers('{"providers": [{"name": "a", "kind": "port", "url": "http://h:1"}, {"name": "a", "kind": "port", "url": "http://h:2"}]}')
        self.assertEqual([p['name'] for p in providers], ['a'])
        self.assertIn('a second provider named a', errors[0]['error'])

    def test_runner_by_hardware_never_downloads(self):
        self.assertEqual(dc.choose_runner('auto', plat='darwin', arch='arm64'), ('mlx', 'metal'))
        self.assertEqual(dc.choose_runner('auto', nvidia=True, plat='linux', arch='x86_64'), ('llama.cpp', 'cuda'))
        self.assertEqual(dc.choose_runner('auto', plat='linux', arch='x86_64'), ('llama.cpp', 'cpu'))
        argv = dc.runner_argv('llama.cpp', 'llama-server', '/m.gguf', 9000)
        self.assertNotIn('-hf', argv)
        self.assertEqual(argv, ['llama-server', '-m', '/m.gguf', '--host', '127.0.0.1', '--port', '9000'])

    def test_command_line(self):
        o = dc.parse_args(['--folder', 'x', '--port', '0', '--yes', '--on', 'terminal,gpio', '--heartbeat', '1', '--no-app', '--no-capabilities'])
        self.assertEqual((o['folder'], o['port'], o['yes'], o['on'], o['heartbeat'], o['app'], o['capabilities']),
                         ('x', 0, True, ['terminal', 'gpio'], 1.0, False, False))
        for bad in (['--port', 'x'], ['--nope'], ['--folder']):
            with self.assertRaises(ValueError):
                dc.parse_args(bad)


if __name__ == '__main__':
    unittest.main()
