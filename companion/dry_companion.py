#!/usr/bin/env python3
"""dry companion, in Python: one folder of this machine (its files and a
terminal in it), its models and its capabilities, for the dry web app.

    python3 dry_companion.py [--folder DIR] [--port 7777] [--yes] [--on ID[,ID]] [--off ID[,ID]]
                             [--providers FILE] [--capabilities DIR] [--origin URL]... [--quiet]

Python 3.9+, the standard library only, one file. The wire contract every
companion keeps (the Node one, the Rust `dry companion`, this one) is
docs/companion-protocol.md, checked from outside by conformance/:

    node conformance/run.mjs --cmd "python3 /abs/path/companion/dry_companion.py"

It listens on 127.0.0.1 only, answers only allowed page origins, and every
call but /v1/health and /v1/pair needs the session token a page gets by
sending the one-time code printed here. Tokens live in this process's memory
only. The page's state goes in <folder>/.dry/.

Outside the contract it also serves the built app (--app), runs a project's
evidence with the `dry` binary when there is one, and git status. It has no
browser driver (the standard library has no WebSocket client for Chrome's
DevTools protocol): the `browser` capability is absent here, saying so.
"""

import codecs
import hmac
import http.client
import json
import os
import platform
import queue
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

VERSION = '0.1.0'
PROTOCOL = 1
DEFAULT_PORT = 7777
NAME = 'dry-companion'
IMPL = 'python'
HERE = os.path.dirname(os.path.realpath(__file__))
WIN = os.name == 'nt'

MAX_BODY = 8 * 1024 * 1024
MAX_READ = 1000000  # the editor's limit (dry_core::workbench::MAX_OPEN)
MAX_OUTPUT = 256 * 1024  # what a session keeps of its output
MAX_TIMEOUT_MS = 60 * 60 * 1000
MAX_EVENTS = 1000  # the event log a page resumes from
DEFAULT_TTL_HOURS = 12
FAILED_PAIRS = 5  # wrong codes before the code changes
HEARTBEAT_S = 15.0
PROBE_S = 30.0  # a declared probe's answer is kept this long
DRY_DIR = '.dry'
CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'
KEY_RE = re.compile(r'^[A-Za-z0-9._:/-]{1,200}$')
HIDDEN = {'.git', 'target', 'node_modules', '.DS_Store'}
CODES = {400: 'bad_request', 401: 'not_paired', 403: 'forbidden', 404: 'not_found', 405: 'method', 409: 'exists',
         413: 'too_large', 428: 'needs_confirm', 500: 'internal', 502: 'upstream', 503: 'unavailable', 504: 'timeout'}
MIME = {
    '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
    '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.wasm': 'application/wasm', '.svg': 'image/svg+xml',
    '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.gif': 'image/gif', '.webp': 'image/webp',
    '.ico': 'image/x-icon', '.ttf': 'font/ttf', '.otf': 'font/otf', '.woff': 'font/woff', '.woff2': 'font/woff2',
    '.txt': 'text/plain; charset=utf-8', '.md': 'text/markdown; charset=utf-8', '.map': 'application/json',
    '.ts': 'text/plain; charset=utf-8', '.gz': 'application/gzip', '.bin': 'application/octet-stream',
    '.img': 'application/octet-stream', '.iso': 'application/octet-stream',
}
IMAGE_RE = re.compile(r'\.(png|jpe?g|gif|webp|svg|bmp|ico|avif)$', re.I)
PROVIDERS_TEMPLATE = {
    'version': 1, 'providers': [],
    'examples': [
        {'name': 'claude', 'kind': 'cli', 'cli': 'claude', 'model': 'sonnet'},
        {'name': 'codex', 'kind': 'cli', 'cli': 'codex'},
        {'name': 'copilot', 'kind': 'cli', 'cli': 'copilot', 'path': '/usr/local/bin/copilot'},
        {'name': 'lmstudio', 'kind': 'port', 'url': 'http://127.0.0.1:1234'},
        {'name': 'ollama', 'kind': 'port', 'url': 'http://127.0.0.1:11434'},
        {'name': 'local', 'kind': 'native', 'runner': 'auto', 'model': 'mlx-community/Qwen3-4B-4bit'},
    ],
}
# The runtimes looked for on PATH (for /v1/capabilities), and how each says its version.
RUNTIMES = {'git': ['--version'], 'gh': ['--version'], 'node': ['--version'], 'python3': ['--version'],
            'cargo': ['--version'], 'mvn': ['-v'], 'java': ['-version'], 'docker': ['--version'],
            'podman': ['--version'], 'claude': ['--version'], 'copilot': ['--version']}


class HttpError(Exception):
    """An error with its status and code (docs/companion-protocol.md §2)."""

    def __init__(self, status, message, code=None):
        Exception.__init__(self, message)
        self.status = status
        self.message = message
        self.code = code or CODES.get(status, 'internal')


class CapError(ValueError):
    """A capability file that does not read: `file:line: why`."""


def jdump(v):
    return json.dumps(v, ensure_ascii=False, separators=(',', ':'))


def short_time():
    return time.strftime('%H:%M:%S')


def last_lines(text, n):
    lines = text.split('\n')
    if lines and lines[-1] == '':
        lines.pop()
    return '\n'.join(lines[max(0, len(lines) - n):])


def num(v, default):
    """JavaScript's `Number(v) || default`."""
    try:
        n = float(v)
    except (TypeError, ValueError):
        return default
    if n != n or n == 0:
        return default
    return n


# ---- rules, pure ----

def origin_allowed(origin, extra=()):
    """Origins a page may call from: the hosted app, any local copy, and the extras given."""
    if not origin or origin == 'null':
        return False
    if origin == 'https://kaush4l.github.io':
        return True
    if re.match(r'^http://(localhost|127\.0\.0\.1|\[::1\])(:\d{1,5})?$', origin):
        return True
    return origin.rstrip('/') in extra


def host_allowed(host, port):
    """The Host header names this server by a loopback name (no DNS rebinding)."""
    if not host:
        return False
    return host.lower() in ('127.0.0.1:%d' % port, 'localhost:%d' % port, '[::1]:%d' % port)


def secret_name(name):
    """Whether a file name says it holds secrets (dry_core::file::secret_file, plus key folders)."""
    n = name.lower()
    return (n == '.env' or n.startswith('.env.') or n in ('.netrc', '.npmrc', '.pypirc', '.pgpass', '.git-credentials')
            or n.startswith(('id_rsa', 'id_ed25519', 'id_ecdsa', 'credentials', 'secrets.'))
            or n.endswith(('.pem', '.key', '.p12', '.pfx', '.keystore'))
            or n in ('.ssh', '.gnupg', '.aws', '.kube', '.docker'))


def secret_env(name):
    """Whether an environment variable's name says it holds a secret (dry_core::terminal::secret_env)."""
    n = name.upper()
    return any(w in n for w in ('KEY', 'TOKEN', 'SECRET', 'PASSWORD', 'PASSWD', 'CREDENTIAL', 'AUTH', 'COOKIE', 'SESSION'))


_PUSH_RE = re.compile(r'(^|[\s;&|(`])git(\s+-\S+(\s+[^-\s]\S*)?)*\s+(push|clone)\b')
_GH_RE = re.compile(r'(^|[\s;&|(`])gh\s+(repo\s+(create|clone|delete|fork)|pr\s+(create|merge)|release\s+create)\b')


def needs_confirm(command):
    """Commands that publish or fetch from outside: only through git endpoints, with a person's click."""
    return bool(_PUSH_RE.search(command) or _GH_RE.search(command))


def store_path_ok(p):
    """A store path: relative, `/`-separated, no empty, `.` or `..` part."""
    return (isinstance(p, str) and 0 < len(p) < 512 and not p.startswith('/') and '\\' not in p and '\0' not in p
            and all(s not in ('', '.', '..') for s in p.split('/')))


def within(real, base):
    return real == base or real.startswith(base if base.endswith(os.sep) else base + os.sep)


def folder_parts(p):
    """A path the page gives for the folder as its parts, or (error, code)."""
    if not isinstance(p, str) or '\0' in p:
        return None, ('a path is text', 'bad_request')
    t = p.replace('\\', '/')
    if t.startswith('/') or re.match(r'^[A-Za-z]:', t):
        return None, ('%s is absolute: paths are relative to the folder' % p, 'path_outside')
    parts = []
    for s in t.split('/'):
        if s in ('', '.'):
            continue
        if s == '..':
            if not parts:
                return None, ('%s leads outside the folder' % p, 'path_outside')
            parts.pop()
            continue
        parts.append(s)
    for s in parts:
        if secret_name(s):
            return None, ('%s may hold secrets, so it is never read, written or listed' % s, 'secret_path')
    if parts and parts[0] == DRY_DIR:
        return None, ("%s/ is dry's own state: it goes through /v1/store and /v1/workspace" % DRY_DIR, 'dry_state')
    return parts, None


# ---- providers.json (docs/companion-protocol.md §8), pure ----

PROVIDER_KINDS = ('cli', 'port', 'native')
CLIS = ('claude', 'codex', 'copilot')
RUNNERS = ('auto', 'mlx', 'llama.cpp')
RUNNER_BIN = {'mlx': 'mlx_lm.server', 'llama.cpp': 'llama-server'}
INSTALL = {
    'claude': 'npm install -g @anthropic-ai/claude-code',
    'codex': 'npm install -g @openai/codex',
    'copilot': 'npm install -g @github/copilot',
    'mlx': 'pip install mlx-lm   (Apple silicon; gives mlx_lm.server)',
    'llama.cpp': 'brew install llama.cpp, or build https://github.com/ggml-org/llama.cpp '
                 '(-DGGML_CUDA=ON on NVIDIA, -DGGML_VULKAN=ON for Vulkan)',
}
MODEL_RE = re.compile(r'^[A-Za-z0-9._:@/+-]{1,200}$')
CLI_MODELS = {'claude': ['sonnet', 'opus', 'haiku'], 'codex': ['default'], 'copilot': ['auto']}


def home(p):
    return os.path.expanduser(p) if p.startswith('~') else p


def check_provider(e):
    """One providers.json entry, checked and normalised; ValueError says why."""
    if not isinstance(e, dict):
        raise ValueError('a provider is an object')
    raw = e.get('name')
    name = '' if raw is None else (raw if isinstance(raw, str) else jdump(raw))
    if not re.match(r'^[a-z0-9][a-z0-9_-]{0,39}$', name) or name.endswith('\n'):
        raise ValueError('name: %s is not a provider name (lower case letters, digits, - and _)' % jdump(raw))
    kind = e.get('kind')
    if kind not in PROVIDER_KINDS:
        raise ValueError('%s: kind is one of %s' % (name, ', '.join(PROVIDER_KINDS)))
    out = {'name': name, 'kind': kind}

    def text(k, rx, why):
        v = e.get(k)
        if v is None or v == '':
            return
        if not isinstance(v, str) or (rx and not rx.match(v)) or '\0' in v:
            raise ValueError('%s: %s %s' % (name, k, why))
        out[k] = v

    text('model', MODEL_RE, 'is a model name or path')
    text('path', None, "is the program's path")
    if 'path' in out and not os.path.isabs(home(out['path'])):
        raise ValueError('%s: path is an absolute path to the program' % name)
    if kind == 'cli':
        cli = e.get('cli') if e.get('cli') is not None else name
        if cli not in CLIS:
            raise ValueError('%s: cli is one of %s' % (name, ', '.join(CLIS)))
        out['cli'] = cli
    elif kind == 'port':
        u = '' if e.get('url') is None else str(e.get('url'))
        bad = ValueError("%s: url is the server's address (http://127.0.0.1:1234)" % name)
        try:
            s = urllib.parse.urlsplit(u)
            s.port  # a port that does not read raises
        except ValueError:
            raise bad
        if not s.scheme or not s.netloc:
            raise bad
        if s.scheme.lower() not in ('http', 'https'):
            raise ValueError('%s: url is http:// or https://' % name)
        href = '%s://%s%s%s' % (s.scheme.lower(), s.netloc.lower(), s.path or '/', ('?' + s.query) if s.query else '')
        href = href.rstrip('/')
        if href.endswith('/v1'):
            href = href[:-3]
        out['url'] = href
    else:
        runner = e.get('runner') if e.get('runner') is not None else 'auto'
        if runner not in RUNNERS:
            raise ValueError('%s: runner is one of %s' % (name, ', '.join(RUNNERS)))
        out['runner'] = runner
        if 'model' not in out:
            raise ValueError('%s: a native provider needs a model (a local file or folder, or a model id already downloaded)' % name)
        if e.get('port') is not None:
            p = e.get('port')
            if not isinstance(p, int) or isinstance(p, bool) or p < 1024 or p > 65535:
                raise ValueError('%s: port is 1024-65535' % name)
            out['port'] = p
    return out


def parse_providers(text):
    """A providers.json text: its entries, and the ones that did not read (with why)."""
    try:
        v = json.loads(text)
    except ValueError as e:
        return [], [{'entry': None, 'error': 'providers.json is not JSON: %s' % e}]
    if not isinstance(v, dict) or not isinstance(v.get('providers'), list):
        return [], [{'entry': None, 'error': 'providers.json needs "providers": [ ... ]'}]
    providers, errors = [], []
    for e in v['providers']:
        try:
            p = check_provider(e)
            if any(x['name'] == p['name'] for x in providers):
                raise ValueError('a second provider named %s' % p['name'])
            providers.append(p)
        except ValueError as err:
            errors.append({'entry': e.get('name') if isinstance(e, dict) else None, 'error': str(err)})
    return providers, errors


def choose_runner(runner, nvidia=False, plat=None, arch=None):
    """Apple silicon runs mlx, NVIDIA llama.cpp with CUDA, anything else llama.cpp on the CPU."""
    plat = plat or sys.platform
    arch = arch or platform.machine().lower()
    if runner and runner != 'auto':
        accel = 'metal' if runner == 'mlx' else 'cuda' if nvidia else 'metal' if plat == 'darwin' else 'cpu'
        return runner, accel
    if plat == 'darwin' and arch in ('arm64', 'aarch64'):
        return 'mlx', 'metal'
    return 'llama.cpp', 'cuda' if nvidia else 'cpu'


def runner_argv(runner, bin_, model, port):
    """The runner serving the model on a local port. Never a download flag."""
    if runner == 'mlx':
        return [bin_, '--model', model, '--host', '127.0.0.1', '--port', str(port)]
    return [bin_, '-m', model, '--host', '127.0.0.1', '--port', str(port)]


# ---- declared capabilities (docs/capabilities.md §3), pure: dry_core::capability::declared ----

CODE_FLAGS = ('-c', '-e', '--eval', '-p', '--print', '-Command', '-command', '/c', '/C', '-r', '--exec')
TYPES = ('string', 'integer', 'number', 'boolean')
INTERVAL = 60
TIMEOUT = 30
MAX_INPUT = 64 * 1024
_ID_CHARS = set('abcdefghijklmnopqrstuvwxyz0123456789-_')
_ALPHA = set('abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ')
_DIGITS = set('0123456789')
_WORD = _ALPHA | _DIGITS | {'_'}


def _valid_id(s):
    return 0 < len(s.encode('utf-8')) <= 64 and all(c in _ID_CHARS for c in s)


def _valid_input(n):
    return 0 < len(n.encode('utf-8')) <= 64 and (n[0] in _ALPHA or n[0] == '_') and all(c in _WORD for c in n)


def cap_duration(v):
    """`60s`, `2m`, `500ms` (rounded up to a second), `90` (seconds), or None."""
    v = v.strip()
    i = next((k for k, c in enumerate(v) if c not in _DIGITS), len(v))
    n, unit = v[:i], v[i:].strip()
    if not n:
        return None
    n = int(n)
    if unit in ('', 's'):
        return n
    if unit == 'm':
        return n * 60
    if unit == 'h':
        return n * 3600
    if unit == 'ms':
        return -(-n // 1000)
    return None


def _argv(v):
    v = v.strip()
    if v.startswith('['):
        try:
            lst = json.loads(v)
        except ValueError as e:
            raise CapError('not a JSON list of strings: %s' % e)
        if not isinstance(lst, list) or not all(isinstance(x, str) for x in lst):
            raise CapError('not a JSON list of strings')
    else:
        lst = v.split()
    if not lst or any(x == '' for x in lst):
        raise CapError('a command is an argument list, the program first')
    return lst


def _placeholders(arg):
    out, rest = [], arg
    while True:
        o = rest.find('{')
        if o < 0:
            return out
        after = rest[o + 1:]
        c = after.find('}')
        if c >= 0 and _valid_input(after[:c]):
            out.append(after[:c])
            rest = after[c + 1:]
        else:
            rest = after


def _command(lst, inputs):
    """A tool's command checked: inputs only as whole arguments, never the program, never after a code flag."""
    names = {i['name'] for i in inputs}
    out = []
    for n, arg in enumerate(lst):
        inner = arg[1:-1] if len(arg) >= 2 and arg.startswith('{') and arg.endswith('}') else None
        if inner is not None and _valid_input(inner):
            if inner not in names:
                raise CapError('{%s} is not one of its inputs' % inner)
            if n == 0:
                raise CapError('the program itself cannot be an input')
            if lst[n - 1] in CODE_FLAGS:
                raise CapError('an input cannot follow %s: it would run as code' % lst[n - 1])
            out.append(('input', inner))
        else:
            if any(p in names for p in _placeholders(arg)):
                raise CapError('"%s": an input goes in as a whole argument ({name}), never inside one' % arg)
            out.append(('text', arg))
    return out


def _input_line(line):
    """`- pin (integer, required): the pin number`: None when the line is not one."""
    s = line.lstrip()
    if not s.startswith('- '):
        return None
    rest = s[2:].strip()
    end = next((k for k, c in enumerate(rest) if c not in _WORD), len(rest))
    name, rest = rest[:end], rest[end:].lstrip()
    ty, required = 'string', False
    if rest.startswith('('):
        r = rest[1:]
        close = r.find(')')
        if close < 0:
            raise CapError('`%s`: a `(` with no `)`' % line)
        for flag in (f.strip() for f in r[:close].split(',')):
            if not flag:
                continue
            if flag == 'required':
                required = True
            elif flag in TYPES:
                ty = flag
            else:
                raise CapError("`%s`: an input's flags are its type (string, integer, number, boolean) and `required`" % flag)
        rest = r[close + 1:].lstrip()
    if not _valid_input(name):
        raise CapError('`%s` is not an input name (letters, digits, _)' % name)
    doc = (rest[1:] if rest.startswith(':') else rest).strip()
    return {'name': name, 'type': ty, 'required': required, 'description': doc}


def _f64(s):
    """Rust's `str::parse::<f64>` (no underscores, no spaces)."""
    if not s or '_' in s or s != s.strip():
        return None
    try:
        return float(s)
    except ValueError:
        return None


_OPS = {'<': '<', '<=': '<=', '>': '>', '>=': '>=', '=': '=', '==': '=', '!=': '!='}


def parse_when(text):
    """`contains <text>`, `missing <text>`, `<key> <op> <value>`."""
    t = text.strip()

    def unq(s):
        return s.strip().strip('"')

    if t.startswith('contains '):
        return {'kind': 'contains', 'text': unq(t[9:])}
    if t.startswith('missing '):
        return {'kind': 'missing', 'text': unq(t[8:])}
    parts = t.split(' ', 2)
    parts += [''] * (3 - len(parts))
    key, op, value = parts
    if op not in _OPS:
        raise CapError('`%s`: a condition is `contains <text>`, `missing <text>` or `<key> <op> <value>` (op: < <= > >= = !=)' % t)
    op = _OPS[op]
    if not key or not value.strip():
        raise CapError('`%s`: a comparison needs a key and a value' % t)
    if op in ('<', '<=', '>', '>=') and _f64(value.strip()) is None:
        raise CapError('`%s`: %s compares numbers' % (t, op))
    return {'kind': 'compare', 'key': key, 'op': op, 'value': unq(value)}


def when_text(w):
    if w['kind'] == 'compare':
        return '%s %s %s' % (w['key'], w['op'], w['value'])
    return '%s %s' % (w['kind'], w['text'])


def _lines(text):
    out = text.split('\n')
    if out and out[-1] == '':
        out.pop()
    return [l[:-1] if l.endswith('\r') else l for l in out]


def value_of(snapshot, key):
    """The first `key: value` or `key=value`, at a line's start or after a space, a tab, `;` or `,`."""
    if not key:
        return None
    for line in _lines(snapshot):
        frm = 0
        while True:
            at = line.find(key, frm)
            if at < 0:
                break
            before = at == 0 or line[:at].endswith((' ', '\t', ';', ','))
            rest = line[at + len(key):]
            if before and rest[:1] in (':', '='):
                v = rest[1:].lstrip()
                ends = [i for i in (v.find(';'), v.find(','), v.find('\t')) if i >= 0]
                return v[:min(ends)].strip() if ends else v.strip()
            frm = at + len(key)
    return None


def _leading_number(v):
    end = len(v)
    for i, c in enumerate(v):
        if not (c in _DIGITS or c == '.' or (i == 0 and c == '-')):
            end = i
            break
    return _f64(v[:end])


def when_holds(w, snapshot):
    if w['kind'] == 'contains':
        return w['text'] in snapshot
    if w['kind'] == 'missing':
        return w['text'] not in snapshot
    found = value_of(snapshot, w['key'])
    if found is None:
        return False
    if w['op'] == '=':
        return found == w['value']
    if w['op'] == '!=':
        return found != w['value']
    a, b = _leading_number(found), _f64(w['value'])
    if a is None or b is None:
        return False
    return {'<': a < b, '<=': a <= b, '>': a > b, '>=': a >= b}[w['op']]


def parse_capability(file, text):
    """One `capabilities/<id>.md` (dry_core::capability::declared::parse, the same rules)."""
    def err(n, why):
        return CapError('%s:%d: %s' % (file, n, why))

    d = {'id': '', 'doc': '', 'probe': None, 'snapshot': None, 'interval': INTERVAL, 'tools': [], 'events': []}
    doc = []
    part = 'head'
    sec = None
    in_inputs = False
    interval_set = False

    def new_sec(start=0):
        return {'start': start, 'name': '', 'doc': [], 'cmd': None, 'inputs': [], 'timeout': None, 'approval': False, 'when': None}

    def finish(sec, part):
        if part == 'tool':
            if sec['cmd'] is None:
                raise err(sec['start'], 'tool %s has no `command:`' % sec['name'])
            at, lst = sec['cmd']
            try:
                command = _command(lst, sec['inputs'])
            except CapError as e:
                raise err(at, str(e))
            if any(t['name'] == sec['name'] for t in d['tools']):
                raise err(sec['start'], 'a second tool named %s' % sec['name'])
            d['tools'].append({'name': sec['name'], 'doc': ' '.join(sec['doc']), 'inputs': sec['inputs'],
                               'approval': sec['approval'], 'command': command,
                               'timeout': sec['timeout'] if sec['timeout'] is not None else TIMEOUT})
        elif part == 'event':
            if sec['when'] is None:
                raise err(sec['start'], 'event %s has no `when:`' % sec['name'])
            if any(e['name'] == sec['name'] for e in d['events']):
                raise err(sec['start'], 'a second event named %s' % sec['name'])
            txt = ' '.join(sec['doc']) if sec['doc'] else '%s: %s' % (sec['name'], when_text(sec['when']))
            d['events'].append({'name': sec['name'], 'when': sec['when'], 'text': txt})

    sec = new_sec()
    for i, raw in enumerate(_lines(text)):
        n = i + 1
        line = raw.rstrip()
        t = line.strip()
        if not d['id']:
            if not t:
                continue
            if not t.startswith('# '):
                raise err(n, "the file starts with `# <id>`, the capability's id")
            cid = t[2:].strip()
            if not _valid_id(cid):
                raise err(n, '`%s` is not a capability id (lower case letters, digits, - and _)' % cid)
            d['id'] = cid
            continue
        if t.startswith('## '):
            head = t[3:]
            finish(sec, part)
            in_inputs = False
            sec = new_sec(n)
            if ':' in head:
                what, name = head.split(':', 1)
                what, name = what.strip(), name.strip()
            else:
                what, name = head.strip(), ''
            if what not in ('tool', 'event'):
                raise err(n, '`## %s`: a section is `## tool: <name>` or `## event: <name>`' % head)
            part = what
            if not _valid_input(name):
                raise err(n, '`%s` is not a %s name (letters, digits, _)' % (name, what))
            sec['name'] = name
            continue
        if not t:
            continue
        if in_inputs:
            try:
                inp = _input_line(line)
            except CapError as e:
                raise err(n, str(e))
            if inp is not None:
                if any(x['name'] == inp['name'] for x in sec['inputs']):
                    raise err(n, 'a second input named %s' % inp['name'])
                sec['inputs'].append(inp)
                continue
        in_inputs = False
        k, v = (None, None)
        if ':' in t:
            k, v = t.split(':', 1)
            k, v = k.strip(), v.strip()
        try:
            if part == 'head' and k == 'probe':
                d['probe'] = _argv(v)
            elif part == 'head' and k == 'snapshot':
                lst = _argv(v)
                if any(_placeholders(a) for a in lst):
                    raise CapError('a snapshot command takes no inputs')
                d['snapshot'] = lst
            elif part == 'head' and k == 'interval':
                s = cap_duration(v)
                if not s:
                    raise CapError('`%s` is not a duration (60s, 5m)' % v)
                d['interval'] = s
                interval_set = True
            elif part == 'tool' and k == 'command':
                sec['cmd'] = (n, _argv(v))
            elif part == 'tool' and k == 'timeout':
                s = cap_duration(v)
                if not s:
                    raise CapError('`%s` is not a duration' % v)
                sec['timeout'] = s
            elif part == 'tool' and k == 'approval':
                if v in ('yes', 'true'):
                    sec['approval'] = True
                elif v in ('no', 'false'):
                    sec['approval'] = False
                else:
                    raise CapError('approval is yes or no, not `%s`' % v)
            elif part == 'tool' and k == 'inputs' and v == '':
                in_inputs = True
            elif part == 'event' and k == 'when':
                sec['when'] = parse_when(v)
            elif part == 'head':
                doc.append(t)
            else:
                sec['doc'].append(t)
        except CapError as e:
            if str(e).startswith(file + ':'):
                raise
            raise err(n, str(e))
    if not d['id']:
        raise err(1, 'empty: the file starts with `# <id>`')
    finish(sec, part)
    if d['probe'] and any(_placeholders(a) for a in d['probe']):
        raise err(1, 'a probe takes no inputs')
    if interval_set and d['snapshot'] is None:
        raise err(1, '`interval:` without a `snapshot:` command')
    if d['events'] and d['snapshot'] is None:
        raise err(1, 'events are read from the snapshot: add a `snapshot:` command')
    d['doc'] = '\n'.join(doc)
    if not d['doc']:
        raise err(1, 'say what it is and how to use it, under the `# id` line')
    return d


def _accepts(ty, v):
    if ty == 'string':
        return True
    if ty == 'integer':
        dg = v[1:] if v.startswith('-') else v
        return 0 < len(dg) <= 15 and all(c in _DIGITS for c in dg)
    if ty == 'number':
        f = _f64(v)
        return f is not None and f == f and f not in (float('inf'), float('-inf')) and not any(c in v for c in 'xXnNiI')
    return v in ('true', 'false')


def _arg_text(v):
    if isinstance(v, str):
        return v
    if isinstance(v, bool):
        return 'true' if v else 'false'
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        return str(int(v)) if v.is_integer() else repr(v)
    return None


def capability_argv(d, tool, args):
    """The argv a declared tool's call becomes: inputs checked (dry_core's Tool::check), each a
    whole argument (left out when optional and not given) and DRY_INPUT_<NAME>. Never a shell."""
    t = next((x for x in d['tools'] if x['name'] == tool), None)
    if t is None:
        raise HttpError(404, '%s has no tool %s' % (d['id'], tool))
    if args is None:
        args = {}
    if not isinstance(args, dict):
        raise HttpError(400, 'arguments is an object')
    call = '%s.%s' % (d['id'], tool)
    given = {}
    for k, raw in args.items():
        if raw is None:
            continue
        if k == 'artifact':
            continue
        i = next((x for x in t['inputs'] if x['name'] == k), None)
        if i is None:
            known = [x['name'] for x in t['inputs']]
            raise HttpError(400, ('%s has no input %s; its inputs: %s' % (call, k, ', '.join(known))) if known else '%s takes no inputs' % call)
        v = _arg_text(raw)
        if v is None:
            raise HttpError(400, '%s must be text' % k)
        if '\0' in v:
            raise HttpError(400, '%s holds a zero byte' % k)
        if len(v.encode('utf-8')) > MAX_INPUT:
            raise HttpError(400, '%s is longer than 64 KB' % k)
        if not _accepts(i['type'], v):
            raise HttpError(400, '%s must be %s' % (k, {'integer': 'a whole number', 'number': 'a number', 'boolean': 'true or false'}.get(i['type'], 'text')))
        if v:
            given[k] = v
    for i in t['inputs']:
        if i['required'] and i['name'] not in given:
            raise HttpError(400, '%s needs %s="..."' % (call, i['name']))
    argv = []
    for kind, a in t['command']:
        if kind == 'text':
            argv.append(a)
        elif a in given:
            argv.append(given[a])
    env = {'DRY_INPUT_%s' % i['name'].upper(): given[i['name']] for i in t['inputs'] if i['name'] in given}
    env['DRY_TOOL'] = call
    return argv, env, t['timeout']


def tool_schema(t):
    props = {}
    for i in t['inputs']:
        p = {'type': i['type']}
        if i['description']:
            p['description'] = i['description']
        props[i['name']] = p
    return {'type': 'object', 'properties': props, 'required': [i['name'] for i in t['inputs'] if i['required']]}


# ---- processes ----

def clean_env(extra=None):
    env = {k: v for k, v in os.environ.items() if not secret_env(k)}
    if extra:
        env.update(extra)
    return env


def popen_group(argv, **kw):
    """A process in its own group (so a stop reaches its children)."""
    if WIN:
        kw['creationflags'] = kw.get('creationflags', 0) | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kw['start_new_session'] = True
    return subprocess.Popen(argv, **kw)


def kill_group(proc, sig=None):
    try:
        if WIN:
            subprocess.run(['taskkill', '/T', '/F', '/PID', str(proc.pid)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        else:
            os.killpg(proc.pid, sig or signal.SIGTERM)
    except (OSError, ProcessLookupError):
        pass


def run_argv(argv, env=None, timeout=30.0, cwd=None):
    """A program and its arguments, no shell, no input, secrets left out; killed at the timeout.
    Returns (code, output, error)."""
    try:
        proc = popen_group(argv, cwd=cwd, env=clean_env(env), stdin=subprocess.DEVNULL,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    except FileNotFoundError:
        return None, '', '%s: no such program' % argv[0]
    except OSError as e:
        return None, '', str(e)
    try:
        out, _ = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        kill_group(proc, getattr(signal, 'SIGKILL', None))
        try:
            out, _ = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            out = b''
        return None, out.decode('utf-8', 'replace')[-65536:], 'timed out after %ds' % round(timeout)
    return proc.returncode, out.decode('utf-8', 'replace')[-65536:], None


def version_of(bin_, args, timeout=8):
    try:
        r = subprocess.run([bin_] + list(args), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           timeout=timeout, env=dict(os.environ, NO_COLOR='1'))
    except (OSError, subprocess.TimeoutExpired):
        return None
    txt = (r.stdout.decode('utf-8', 'replace') + '\n' + r.stderr.decode('utf-8', 'replace'))
    first = next((l.strip() for l in txt.split('\n') if l.strip()), '')
    if r.returncode != 0 and not first:
        return None
    return first[:120]


def free_port():
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(('127.0.0.1', 0))
    p = s.getsockname()[1]
    s.close()
    return p


class Upstream(Exception):
    """A server that answered, but not well."""


def connection(base, timeout):
    """An HTTP connection to `base` (never through a proxy: the providers are this machine's)."""
    s = urllib.parse.urlsplit(base)
    cls = http.client.HTTPSConnection if s.scheme == 'https' else http.client.HTTPConnection
    return cls(s.hostname, s.port, timeout=timeout), s.path or ''


def get_json(url, timeout=3.0):
    conn, path = connection(url, timeout)
    try:
        q = urllib.parse.urlsplit(url).query
        conn.request('GET', (path or '/') + ('?' + q if q else ''))
        r = conn.getresponse()
        body = r.read()
        if r.status >= 400:
            raise Upstream('answered %d' % r.status)
        return json.loads(body.decode('utf-8'))
    finally:
        conn.close()


def models_at(url, timeout=3.0):
    v = get_json(url + '/v1/models', timeout)
    return [m['id'] for m in (v.get('data') or []) if isinstance(m, dict) and isinstance(m.get('id'), str)]


def why_of(e):
    if isinstance(e, ConnectionRefusedError):
        return 'ECONNREFUSED'
    if isinstance(e, (socket.timeout, TimeoutError)):
        return 'timed out'
    if isinstance(e, ValueError) and not isinstance(e, UnicodeError):
        return 'not JSON: %s' % e
    return str(e) or e.__class__.__name__


def parallel(fn, items):
    """fn over items, each in its own thread (probes wait on the network and on programs)."""
    out = [None] * len(items)

    def run(i, x):
        out[i] = fn(x)

    threads = [threading.Thread(target=run, args=(i, x), daemon=True) for i, x in enumerate(items)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    return out


# ---- streams (server-sent events) ----

class Stream:
    """One open stream: what to send goes in its queue; None ends it."""

    def __init__(self, pairing, sid=None, all_=False):
        self.q = queue.Queue()
        self.pairing = pairing
        self.id = sid
        self.all = all_
        self.closed = False

    def send(self, text):
        if not self.closed:
            self.q.put(text)

    def close(self):
        self.closed = True
        self.q.put(None)


class Session:
    def __init__(self, sid, key, cwd, command, mode):
        self.id, self.key, self.cwd, self.command, self.mode = sid, key, cwd, command, mode
        self.status, self.exit, self.signal, self.timed_out = 'running', None, None, False
        self.started, self.ended = int(time.time()), None
        self.output = ''
        self.proc = None
        self.killed = False
        self.done = threading.Event()


# ---- the companion ----

class Companion:
    def __init__(self, o):
        self.o = o
        self.quiet = o['quiet']
        self.verbose = o['verbose']
        self.port = o['port']
        self.extra_origins = [x.rstrip('/') for x in o['origins']]
        self.lmstudio = o['lmstudio'].rstrip('/') if o.get('lmstudio') else None
        self.ollama = o['ollama'].rstrip('/') if o.get('ollama') else None
        folder = os.path.abspath(home(o.get('folder') or os.getcwd()))
        if not os.path.isdir(folder):
            raise ValueError('the folder %s is not a folder' % folder)
        real = os.path.realpath(folder)
        if os.path.dirname(real) == real:
            raise ValueError('the whole disk cannot be the folder: give one folder with --folder')
        self.folder = real
        self.dry_dir = os.path.join(real, DRY_DIR)
        self.store_dir = os.path.abspath(home(o['store'])) if o.get('store') else os.path.join(self.dry_dir, 'state')
        self.snapshot_file = os.path.join(self.dry_dir, 'snapshot.json')
        self.config_file = os.path.abspath(home(o['config'])) if o.get('config') else os.path.join(self.dry_dir, 'companion.json')
        self.providers_file = os.path.abspath(home(o['providers'])) if o.get('providers') else os.path.join(self.dry_dir, 'providers.json')
        self.heartbeat = o['heartbeat']
        self.ttl = (o['ttl'] if o.get('ttl') else DEFAULT_TTL_HOURS) * 3600.0
        self.app_dir = self._app_dir()
        self.cap_dir = self._cap_dir()
        self.lock = threading.RLock()
        self.provider_lock = threading.RLock()
        self.print_lock = threading.Lock()
        self.code = self.new_code()
        self.failed = 0
        self.pairings = {}  # token -> pairing; memory only
        self.next_pairing = 1
        self.sessions = {}  # id -> Session (insertion ordered)
        self.next_id = 1
        self.listeners = set()  # /v1/events streams
        self.mcp_streams = set()  # GET /mcp streams
        self.events = []
        self.next_event = 1
        self.provider_states = {}
        self.nvidia = None
        self.probes = {}
        self.snaps = {}
        self.cap_signature = None
        self.runtimes = None
        self.evidence_runs = set()
        self.config = self.read_config()
        for i in o['on']:
            self.config['switches'][i] = True
        for i in o['off']:
            self.config['switches'][i] = False
        self.server = None
        self.stopped = threading.Event()

    def _app_dir(self):
        a = self.o.get('app')
        if a is False:
            return None
        d = os.path.abspath(a) if a else os.path.join(HERE, '..', 'dist')
        if os.path.exists(os.path.join(d, 'app', 'index.html')) or os.path.exists(os.path.join(d, 'index.html')):
            return os.path.realpath(d)
        if a:
            raise ValueError('--app %s: no index.html there (build the app first)' % d)
        return None

    def _cap_dir(self):
        c = self.o.get('capabilities')
        if c is False:
            return None
        d = os.path.abspath(home(c)) if c else os.path.join(HERE, '..', 'capabilities')
        if os.path.isdir(d):
            return os.path.realpath(d)
        if c:
            raise ValueError('--capabilities %s: not a folder' % d)
        return None

    # ---- lines ----

    def log(self, line):
        if not self.quiet:
            with self.print_lock:
                print(line, flush=True)

    def new_code(self):
        return ''.join(secrets.choice(CODE_ALPHABET) for _ in range(6))

    def announce_code(self):
        f = self.o.get('pair_code_file')
        if f:
            try:
                with open(f, 'w', encoding='utf-8') as fh:
                    fh.write(self.code + '\n')
            except OSError as e:
                sys.stderr.write('could not write %s: %s\n' % (f, e))
        if not self.quiet:
            self.log('\n  pairing code: %s   (type it in dry → Settings → Companion; it works once)\n' % self.code)

    # ---- the workspace folder (docs/companion-protocol.md §6) ----

    def ws_path(self, p, for_write=False, dir_=False, may_miss=False):
        parts, bad = folder_parts('' if p is None else p)
        if bad:
            raise HttpError(400 if bad[1] == 'bad_request' else 403, bad[0], bad[1])
        ab = os.path.join(self.folder, *parts) if parts else self.folder
        exists = os.path.exists(ab)
        if exists:
            real = os.path.realpath(ab)
        else:
            if os.path.islink(ab):
                raise HttpError(403, '%s is a link that leads nowhere' % p, 'path_outside')
            up, rest = os.path.dirname(ab), [os.path.basename(ab)]
            while not os.path.exists(up):
                if os.path.islink(up):
                    raise HttpError(403, '%s is a link that leads nowhere' % p, 'path_outside')
                rest.insert(0, os.path.basename(up))
                up = os.path.dirname(up)
            real = os.path.join(os.path.realpath(up), *rest)
        if not within(real, self.folder):
            raise HttpError(403, '%s leads outside the folder (a link?)' % p, 'path_outside')
        rel = [x for x in os.path.relpath(real, self.folder).split(os.sep) if x and x != '.']
        for part in rel:
            if secret_name(part):
                raise HttpError(403, '%s may hold secrets, so it is never read, written or listed' % part, 'secret_path')
        if rel and rel[0] == DRY_DIR:
            raise HttpError(403, "%s/ is dry's own state" % DRY_DIR, 'dry_state')
        if for_write and '.git' in rel:
            raise HttpError(403, 'files inside .git are written only by git', 'dry_state')
        if not exists and not (for_write or may_miss):
            raise HttpError(404, 'no such file: %s' % p, 'not_found')
        if dir_ and exists and not os.path.isdir(real):
            raise HttpError(400, '%s is not a folder' % p, 'not_a_folder')
        return real, '/'.join(rel), exists

    def ws_list(self, d, mx):
        base, rel0, _ = self.ws_path(d, dir_=True)
        out, q, truncated = [], [''], False
        while q:
            rel = q.pop(0)
            try:
                entries = sorted(os.scandir(os.path.join(base, rel) if rel else base), key=lambda e: e.name)
            except OSError:
                continue
            for e in entries:
                if e.name in HIDDEN or e.name == DRY_DIR or secret_name(e.name):
                    continue
                r = rel + '/' + e.name if rel else e.name
                try:
                    is_dir = e.is_dir(follow_symlinks=False)
                    if e.is_symlink():
                        real = os.path.realpath(e.path)
                        if not within(real, self.folder) or not os.path.exists(real):
                            continue
                        is_dir = os.path.isdir(real)
                except OSError:
                    continue
                if is_dir:
                    q.append(r)
                elif len(out) >= mx:
                    truncated = True
                    break
                else:
                    out.append(r)
            if truncated:
                break
        return {'path': rel0, 'files': out, 'truncated': truncated}

    def ws_read(self, p):
        real, rel, _ = self.ws_path(p)
        if os.path.isdir(real):
            raise HttpError(400, '%s is a folder' % p, 'not_a_file')
        size = os.stat(real).st_size
        if size > MAX_READ:
            return {'path': rel, 'kind': 'too_large', 'bytes': size}
        with open(real, 'rb') as fh:
            data = fh.read()
        text = None
        if b'\0' not in data:
            try:
                text = data.decode('utf-8')
            except UnicodeDecodeError:
                pass
        if text is None:
            return {'path': rel, 'kind': 'binary', 'bytes': size, 'image': bool(IMAGE_RE.search(real))}
        return {'path': rel, 'kind': 'text', 'text': text, 'bytes': size}

    def ws_write(self, p, text):
        if not isinstance(text, str):
            raise HttpError(400, 'write takes text')
        real, rel, _ = self.ws_path(p, for_write=True)
        if os.path.isdir(real):
            raise HttpError(400, '%s is a folder' % p, 'not_a_file')
        os.makedirs(os.path.dirname(real), exist_ok=True)
        data = text.encode('utf-8')
        atomic_write(real, data)
        return {'ok': True, 'path': rel, 'bytes': len(data)}

    def ws_stat(self, p):
        real, rel, exists = self.ws_path(p, may_miss=True)  # a stat reads: only writes into .git are refused
        if not exists:
            return {'path': rel, 'kind': 'missing'}
        st = os.stat(real)
        d = os.path.isdir(real)
        return {'path': rel, 'kind': 'dir' if d else 'file', 'size': 0 if d else st.st_size, 'modified': int(st.st_mtime)}

    # ---- switches ----

    def read_config(self):
        try:
            with open(self.config_file, encoding='utf-8') as fh:
                v = json.load(fh)
            sw = v.get('switches') if isinstance(v, dict) else None
            return {'version': 1, 'switches': sw if isinstance(sw, dict) else {}}
        except (OSError, ValueError):
            return {'version': 1, 'switches': {}}

    def save_config(self):
        os.makedirs(os.path.dirname(self.config_file), exist_ok=True)
        atomic_write(self.config_file, (json.dumps(self.config, indent=2) + '\n').encode('utf-8'))

    def actuator(self, cid):
        if cid == 'terminal':
            return True
        d = next((x for x in self.load_declared()[0] if x['id'] == cid), None)
        return bool(d and any(t['approval'] for t in d['tools']))

    def is_on(self, cid):
        v = self.config['switches'].get(cid)
        return v if isinstance(v, bool) else not self.actuator(cid)

    def need(self, cid):
        if not self.is_on(cid):
            raise HttpError(403, "%s is switched off in the companion's terminal (type: on %s)" % (cid, cid), 'capability_off')

    def set_switch(self, cid, on):
        self.config['switches'][cid] = bool(on)
        self.save_config()
        self.log('%s  %s switched %s (saved in %s)' % (short_time(), cid, 'on' if on else 'off', self.config_file))
        self.notify('notifications/tools/list_changed', {})

    # ---- the workspace's own state ----

    def snapshot_read(self):
        try:
            with open(self.snapshot_file, encoding='utf-8') as fh:
                return json.load(fh)
        except (OSError, ValueError):
            return None

    def snapshot_write(self, v):
        if not isinstance(v, dict) or v.get('version') != 1 or isinstance(v.get('version'), bool):
            raise HttpError(400, 'a snapshot is {"version": 1, ...} (docs/companion-protocol.md §7)', 'bad_snapshot')
        os.makedirs(self.dry_dir, exist_ok=True)
        atomic_write(self.snapshot_file, (json.dumps(v, indent=1, ensure_ascii=False) + '\n').encode('utf-8'))
        return {'ok': True, 'path': DRY_DIR + '/snapshot.json'}

    def workspace_info(self):
        st = self.store_dir
        state = os.path.relpath(st, self.folder).replace(os.sep, '/') if within(st, self.folder) else st
        return {'protocol': PROTOCOL, 'folder': self.folder, 'name': os.path.basename(self.folder), 'state': state,
                'snapshot': self.snapshot_read(), 'switches': {i: self.is_on(i) for i in ('file', 'terminal')}}

    def store_read(self):
        texts = {}
        for root, dirs, files in os.walk(self.store_dir):
            dirs.sort()
            for f in sorted(files):
                if f.endswith('.tmp'):
                    continue
                full = os.path.join(root, f)
                rel = os.path.relpath(full, self.store_dir).replace(os.sep, '/')
                try:
                    with open(full, encoding='utf-8', errors='replace') as fh:
                        texts[rel] = fh.read()
                except OSError:
                    pass
        return texts

    def store_write(self, put, remove):
        if not isinstance(put, dict) or not isinstance(remove, list):
            raise HttpError(400, 'the store takes {put: {path: text}, remove: [path]}')
        for p in list(put.keys()) + remove:
            if not store_path_ok(p):
                raise HttpError(400, 'not a store path: %s' % p)
        for p, text in put.items():
            full = os.path.join(self.store_dir, *p.split('/'))
            os.makedirs(os.path.dirname(full), exist_ok=True)
            atomic_write(full, (text if isinstance(text, str) else jdump(text)).encode('utf-8'), tmp=full + '.tmp')
        for p in remove:
            try:
                os.remove(os.path.join(self.store_dir, *p.split('/')))
            except OSError:
                pass

    # ---- pairings ----

    def pairing_of(self, tok):
        if not tok:
            return None
        with self.lock:
            p = self.pairings.get(tok)
            if not p:
                return None
            if time.time() >= p['expires']:
                del self.pairings[tok]
                self.log('%s  pairing %s expired' % (short_time(), p['id']))
                return None
            p['seen'] = time.time()
            return p

    def list_pairings(self, current):
        now = time.time()
        out = []
        with self.lock:
            for t, p in self.pairings.items():
                if p['expires'] <= now:
                    continue
                x = {'id': p['id'], 'origin': p['origin'], 'created': int(p['created']), 'expires': int(p['expires']),
                     'seen': int(p['seen']), 'current': hmac.compare_digest(t, current) if current else False}
                if p.get('hello'):
                    x['hello'] = p['hello']
                x['streams'] = p.get('streams', 0)
                out.append(x)
        return out

    def revoke(self, pid):
        with self.lock:
            for t, p in list(self.pairings.items()):
                if p['id'] == pid:
                    del self.pairings[t]
                    self.close_streams(p)
                    self.log('%s  pairing %s revoked' % (short_time(), pid))
                    return True
        return False

    def close_streams(self, pairing):
        with self.lock:
            for s in [x for x in self.listeners if x.pairing is pairing]:
                self.listeners.discard(s)
                s.close()
            for s in [x for x in self.mcp_streams if x.pairing is pairing]:
                self.mcp_streams.discard(s)
                s.close()

    def pairings_text(self):
        lst = self.list_pairings(None)
        if not lst:
            return '  no pairings'
        lines = []
        for p in lst:
            h = p.get('hello')
            hello = ' [%s %s%s]' % (h.get('runtime', '?'), h.get('platform', ''), ', listening' if p['streams'] else '') if isinstance(h, dict) else ''
            lines.append('  %s%s  %s  since %s  until %s' % (p['id'], hello, p['origin'] or '(no origin)',
                                                           time.strftime('%H:%M:%S', time.localtime(p['created'])),
                                                           time.strftime('%H:%M:%S', time.localtime(p['expires']))))
        return '\n'.join(lines) + '\n  (type: revoke <id>)'

    def pair(self, given, origin):
        """A pairing code sent: the pairing, or None (a wrong code)."""
        with self.lock:
            if not isinstance(given, str) or not hmac.compare_digest(given.strip().upper().encode('utf-8'), self.code.encode('utf-8')):
                self.failed += 1
                if self.failed >= FAILED_PAIRS:
                    self.code = self.new_code()
                    self.failed = 0
                    self.log('%s  %d wrong codes: the code changed' % (short_time(), FAILED_PAIRS))
                    self.announce_code()
                return None
            token = secrets.token_hex(32)
            now = time.time()
            p = {'id': 'p%d' % self.next_pairing, 'origin': origin or '', 'created': now, 'expires': now + self.ttl, 'seen': now, 'streams': 0}
            self.next_pairing += 1
            self.pairings[token] = p
            self.code = self.new_code()
            self.failed = 0
        self.log('%s  connected: %s from %s until %s' % (short_time(), p['id'], origin or '(no origin: a program)',
                                                       time.strftime('%H:%M', time.localtime(p['expires']))))
        self.announce_code()
        return token, p

    # ---- the event log and the terminal's sessions ----

    def emit(self, event, data, all_):
        with self.lock:
            e = {'id': self.next_event, 'event': event, 'data': data, 'all': all_, 'at': int(time.time())}
            self.next_event += 1
            self.events.append(e)
            if len(self.events) > MAX_EVENTS:
                self.events.pop(0)
            for l in list(self.listeners):
                if self.wants(l, e):
                    l.send(sse_text(e))
        return e

    @staticmethod
    def wants(l, e):
        return e['all'] or l.all or (l.id is not None and l.id == (e['data'] or {}).get('id'))

    def public_session(self, s, tail=None):
        out = {'id': s.id, 'key': s.key, 'cwd': s.cwd, 'command': s.command, 'mode': s.mode, 'status': s.status, 'exit': s.exit,
               'signal': s.signal, 'timed_out': s.timed_out, 'started': s.started, 'ended': s.ended}
        if tail is not None:
            out['tail'] = last_lines(s.output, tail)
        return out

    def term_session(self, s, lines=None):
        out = {'key': s.key, 'id': s.id, 'mode': s.mode, 'status': s.status, 'exit': s.exit, 'signal': s.signal,
               'timed_out': s.timed_out, 'started': s.started, 'ended': s.ended, 'command': s.command}
        if lines is not None:
            out['tail'] = last_lines(s.output, lines)
        return out

    def launch(self, cwd, command, mode, key, timeout_ms):
        with self.lock:
            sid = str(self.next_id)
            self.next_id += 1
            s = Session(sid, key or None, cwd, command, mode)
            if s.key:
                for oid, ot in list(self.sessions.items()):
                    if ot.key == s.key and ot.status != 'running':
                        del self.sessions[oid]
            self.sessions[sid] = s
            if len(self.sessions) > 200:
                for oid, ot in list(self.sessions.items()):
                    if ot.status != 'running':
                        del self.sessions[oid]
                        if len(self.sessions) <= 200:
                            break
        try:
            if WIN:
                comspec = os.environ.get('ComSpec', 'cmd.exe')
                proc = popen_group('"%s" /d /s /c "%s"' % (comspec, command), cwd=cwd, env=clean_env(),
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            else:
                proc = popen_group(['/bin/sh', '-c', command], cwd=cwd, env=clean_env(),
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        except OSError as e:
            self._finish(s, None, None, 'could not start the shell: %s' % e)
            return s
        s.proc = proc
        timer = None
        if timeout_ms:
            def on_timeout():
                s.timed_out = True
                self.kill(s)
            timer = threading.Timer(timeout_ms / 1000.0, on_timeout)
            timer.daemon = True
            timer.start()

        def read():
            dec = codecs.getincrementaldecoder('utf-8')('replace')
            while True:
                try:
                    b = proc.stdout.read1(65536)
                except (OSError, ValueError):
                    b = b''
                if not b:
                    break
                self._take(s, dec.decode(b))
            tail = dec.decode(b'', final=True)
            if tail:
                self._take(s, tail)
            rc = proc.wait()
            if timer:
                timer.cancel()
            if rc < 0:
                try:
                    sig = signal.Signals(-rc).name
                except ValueError:
                    sig = 'SIG%d' % -rc
                self._finish(s, None, sig)
            elif WIN and s.killed:
                self._finish(s, None, 'SIGTERM')
            else:
                self._finish(s, rc, None)

        threading.Thread(target=read, daemon=True).start()
        return s

    def _take(self, s, text):
        with self.lock:
            s.output += text
            if len(s.output) > MAX_OUTPUT:
                s.output = s.output[-MAX_OUTPUT:]
            msg = 'event: output\ndata: %s\n\n' % jdump({'id': s.id, 'key': s.key, 'text': text})
            for l in list(self.listeners):
                if l.id == s.id:
                    l.send(msg)

    def _finish(self, s, code, sig, why=None):
        with self.lock:
            if s.status != 'running':
                return
            if why:
                s.output += '\n%s\n' % why
            s.status, s.exit, s.signal, s.ended, s.proc = 'exited', code, sig, int(time.time()), None
        self.log('%s  exec %s %s ended %s%s: %s' % (short_time(), s.id, s.mode, code if code is not None else sig,
                                                   ' (timed out)' if s.timed_out else '', s.command[:60]))
        # A start's exit is a callback: every stream hears it; a run's or a background's only by id or ?all=1.
        self.emit('exit', self.public_session(s), s.mode == 'start')
        s.done.set()

    def kill(self, s):
        p = s.proc
        if p is None:
            return
        s.killed = True
        kill_group(p, signal.SIGTERM if not WIN else None)
        if not WIN:
            def later():
                if s.proc is p:
                    kill_group(p, signal.SIGKILL)
            t = threading.Timer(2.0, later)
            t.daemon = True
            t.start()

    def find_session(self, key):
        with self.lock:
            found = None
            for s in self.sessions.values():
                if s.key == key:
                    found = s
            return found

    def term_key(self, k):
        if not isinstance(k, str) or not KEY_RE.match(k) or k.endswith('\n'):
            raise HttpError(400, 'key names a session: letters, digits and ._:/- (at most 200)', 'bad_key')
        return k

    def term_prep(self, b):
        self.need('terminal')
        key = self.term_key(b.get('key'))
        command = b.get('command') if isinstance(b.get('command'), str) else ''
        if not command.strip():
            raise HttpError(400, 'a command is needed')
        if '\0' in command:
            raise HttpError(400, 'the command holds a zero byte')
        if needs_confirm(command):
            raise HttpError(428, "push, clone and publishing commands go through the git endpoints with a person's click (docs/companion.md)")
        cwd = self.ws_path(b.get('cwd') if b.get('cwd') is not None else '', dir_=True)[0]
        return key, command, cwd

    def stop_all(self):
        n = 0
        with self.lock:
            running = [s for s in self.sessions.values() if s.status == 'running']
            ev = list(self.evidence_runs)
        for s in running:
            self.kill(s)
            n += 1
        for p in ev:
            kill_group(p)
            n += 1
        self.log('%s  stop all: %d session%s stopped' % (short_time(), n, '' if n == 1 else 's'))
        return {'stopped': n, 'browser': False}

    # ---- runtimes ----

    def detect_runtimes(self, names=None):
        names = names or list(RUNTIMES)

        def one(n):
            where = shutil.which(n)
            return n, ({'path': where, 'version': version_of(where, RUNTIMES[n])} if where else None)

        found = dict(parallel(one, names))
        with self.lock:
            self.runtimes = dict(self.runtimes or {}, **found)
        return found

    def runtime(self, n):
        with self.lock:
            if self.runtimes is not None and n in self.runtimes:
                return self.runtimes[n]
        return self.detect_runtimes([n])[n]

    # ---- providers (docs/companion-protocol.md §8) ----

    def providers_template(self):
        try:
            with open(os.path.join(HERE, '..', 'providers.json'), encoding='utf-8') as fh:
                return json.load(fh)
        except (OSError, ValueError):
            return PROVIDERS_TEMPLATE

    def declared_providers(self):
        text = None
        try:
            with open(self.providers_file, encoding='utf-8') as fh:
                text = fh.read()
        except OSError:
            pass
        providers, errors = parse_providers(text) if text is not None else ([], [])
        for name, url in (('lmstudio', self.lmstudio), ('ollama', self.ollama)):
            if url and not any(p['name'] == name for p in providers):
                providers.append(check_provider({'name': name, 'kind': 'port', 'url': url}))
        return providers, errors, text is not None

    def write_providers(self, v):
        os.makedirs(os.path.dirname(self.providers_file), exist_ok=True)
        atomic_write(self.providers_file, (json.dumps(v, indent=2, ensure_ascii=False) + '\n').encode('utf-8'))

    def add_provider(self, entry):
        try:
            p = check_provider(entry)
        except ValueError as e:
            raise HttpError(400, str(e), 'bad_provider')
        with self.provider_lock:
            try:
                with open(self.providers_file, encoding='utf-8') as fh:
                    v = json.load(fh)
                if not isinstance(v, dict):
                    raise ValueError
            except (OSError, ValueError):
                v = self.providers_template()
            if not isinstance(v.get('providers'), list):
                v['providers'] = []
            if any(isinstance(x, dict) and x.get('name') == p['name'] for x in v['providers']) or \
                    any(x['name'] == p['name'] for x in self.declared_providers()[0]):
                raise HttpError(409, 'a provider named %s is already declared' % p['name'], 'exists')
            v['providers'].append(p)
            self.write_providers(v)
            self.log('%s  provider %s (%s) added to %s' % (short_time(), p['name'], p['kind'], self.providers_file))
            r = self._probe_provider(p)
        self.notify('notifications/tools/list_changed', {})
        return self.public_provider(r)

    @staticmethod
    def hf_cached(mid):
        hub = os.environ.get('HF_HUB_CACHE') or os.path.join(os.environ.get('HF_HOME') or os.path.join(os.path.expanduser('~'), '.cache', 'huggingface'), 'hub')
        return os.path.exists(os.path.join(hub, 'models--' + mid.replace('/', '--')))

    def _probe_provider(self, entry):
        """One provider probed: ok (its models), failed (why), or absent (the hint says how to get it)."""
        had = self.provider_states.get(entry['name'])
        r = {'entry': entry, 'state': 'failed', 'why': '', 'hint': '', 'models': [], 'url': None, 'bin': None, 'version': None,
             'child': had.get('child') if had else None, 'at': time.time()}
        kind = entry['kind']
        if kind == 'cli':
            b = home(entry['path']) if entry.get('path') else shutil.which(entry['cli'])
            if not b or not os.path.exists(b):
                r.update(state='absent', why=('%s: no such program' % entry['path']) if entry.get('path') else '%s is not on PATH' % entry['cli'],
                         hint=('fix "path", or %s' % INSTALL[entry['cli']]) if entry.get('path') else INSTALL[entry['cli']])
            else:
                v = version_of(b, ['--version'])
                if v is None:
                    r.update(state='failed', why='%s --version did not answer' % b, bin=b)
                else:
                    r.update(state='ok', bin=b, version=v, models=[entry['model']] if entry.get('model') else list(CLI_MODELS[entry['cli']]))
        elif kind == 'port':
            try:
                r.update(state='ok', url=entry['url'], models=models_at(entry['url']))
            except Exception as e:  # noqa: BLE001 - every failure is a why
                r.update(state='failed', url=entry['url'], why='GET %s/v1/models: %s' % (entry['url'], why_of(e)),
                         hint='is its server running, on that port?')
        else:
            if self.nvidia is None:
                self.nvidia = bool(shutil.which('nvidia-smi'))
            runner, accel = choose_runner(entry['runner'], self.nvidia)
            r['runner'], r['accel'] = runner, accel
            b = home(entry['path']) if entry.get('path') else shutil.which(RUNNER_BIN[runner])
            local = os.path.exists(home(entry['model']))
            if not b or not os.path.exists(b):
                r.update(state='absent', why='%s (%s, %s) is not on this machine' % (entry.get('path') or RUNNER_BIN[runner], runner, accel),
                         hint=INSTALL[runner])
            elif not local and not (runner == 'mlx' and self.hf_cached(entry['model'])):
                r.update(state='failed', bin=b, why='the model %s is not on this machine (the companion never downloads)' % entry['model'],
                         hint=('download it yourself: huggingface-cli download %s' % entry['model']) if runner == 'mlx'
                         else 'give "model" the path of a .gguf file on this machine')
            elif had and had.get('child') and had.get('url'):
                try:
                    r.update(state='ok', bin=b, url=had['url'], models=models_at(had['url']))
                except Exception:  # noqa: BLE001
                    r.update(state='starting', bin=b, url=had['url'], why='the runner is starting')
            else:
                r.update(self._start_runner(entry, runner, b))
        self.provider_states[entry['name']] = r
        return r

    def _start_runner(self, entry, runner, b):
        """A native runner started on a local port, then a port (its /v1/models answering makes it ok)."""
        port = entry.get('port') or free_port()
        argv = runner_argv(runner, b, home(entry['model']), port)
        url = 'http://127.0.0.1:%d' % port
        try:
            child = popen_group(argv, env=clean_env(), stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        except OSError as e:
            return {'state': 'failed', 'bin': b, 'url': url, 'why': 'the runner did not start: %s' % e, 'child': None}
        errs = {'text': ''}

        def drain():
            for line in iter(child.stderr.readline, b''):
                errs['text'] = (errs['text'] + line.decode('utf-8', 'replace'))[-2000:]

        threading.Thread(target=drain, daemon=True).start()
        self.log('%s  provider %s: started %s (%s) on %s' % (short_time(), entry['name'], os.path.basename(b), runner, url))
        name = entry['name']

        def last_err():
            lines = [l for l in errs['text'].strip().split('\n') if l]
            return lines[-1] if lines else 'no output'

        def watch():
            child.wait()
            st = self.provider_states.get(name)
            if st and st.get('child') is child:
                st['child'] = None
                st['state'] = 'failed'
                st['why'] = 'the runner exited: %s' % last_err()

        threading.Thread(target=watch, daemon=True).start()
        t0 = time.time()
        wait = self.o.get('runner_wait', 120.0)
        while time.time() - t0 < wait:
            if child.poll() is not None:
                return {'state': 'failed', 'bin': b, 'url': url, 'why': 'the runner exited (%s): %s' % (child.returncode, last_err()), 'child': None}
            try:
                return {'state': 'ok', 'bin': b, 'url': url, 'models': models_at(url, 1.0), 'child': child}
            except Exception:  # noqa: BLE001 - not yet
                time.sleep(0.3)
        return {'state': 'starting', 'bin': b, 'url': url, 'why': 'the runner is still loading its model', 'child': child}

    def probe_providers(self):
        with self.provider_lock:
            providers = self.declared_providers()[0]

            def one(p):
                try:
                    return self._probe_provider(p)
                except Exception as e:  # noqa: BLE001
                    r = {'entry': p, 'state': 'failed', 'why': str(e), 'hint': '', 'models': []}
                    self.provider_states[p['name']] = r
                    return r

            out = parallel(one, providers)
            for name in list(self.provider_states):
                if not any(p['name'] == name for p in providers):
                    self.stop_runner(self.provider_states.pop(name))
            return out

    @staticmethod
    def stop_runner(st):
        c = st.get('child') if st else None
        if c is None:
            return
        kill_group(c)
        st['child'] = None

    @staticmethod
    def public_provider(r):
        e = r['entry']
        out = {'name': e['name'], 'kind': e['kind'], 'entry': e, 'state': r['state'], 'why': r.get('why') or '', 'hint': r.get('hint') or '',
               'models': ['%s/%s' % (e['name'], m) for m in r.get('models') or []]}
        if r.get('url'):
            out['url'] = r['url']
        if r.get('runner'):
            out['runner'], out['accel'] = r['runner'], r['accel']
        if r.get('version'):
            out['version'] = r['version']
        return out

    def providers_now(self, refresh):
        providers, errors, exists = self.declared_providers()
        if refresh or any(p['name'] not in self.provider_states for p in providers):
            self.probe_providers()
        pub = []
        for p in providers:
            st = self.provider_states.get(p['name']) or {'entry': p, 'state': 'failed', 'why': 'not probed', 'models': []}
            pub.append(self.public_provider(st))
        return {'file': self.providers_file, 'exists': exists, 'template': self.providers_template(), 'errors': errors, 'providers': pub}

    def all_models(self):
        return [{'id': i, 'object': 'model', 'owned_by': p['name'], 'provider': p['name'], 'kind': p['kind']}
                for p in self.providers_now(False)['providers'] if p['state'] == 'ok' for i in p['models']]

    # ---- a cli provider's chat ----

    @staticmethod
    def text_of(content):
        if isinstance(content, str):
            return content
        if isinstance(content, list):
            return ''.join(p if isinstance(p, str) else (p.get('text') or '') if isinstance(p, dict) else '' for p in content)
        return ''

    def flatten(self, messages):
        msgs = [m for m in messages if isinstance(m, dict)]
        system = '\n\n'.join(self.text_of(m.get('content')) for m in msgs if m.get('role') in ('system', 'developer'))
        turns = [m for m in msgs if m.get('role') not in ('system', 'developer')]
        if len(turns) == 1 and turns[0].get('role') == 'user':
            return system, self.text_of(turns[0].get('content'))
        who = {'assistant': 'Assistant', 'tool': 'Tool'}
        prompt = '\n\n'.join('%s: %s' % (who.get(m.get('role'), 'User'), self.text_of(m.get('content'))) for m in turns)
        return system, prompt + '\n\nReply as the Assistant to the last message.'

    @staticmethod
    def run_cli(b, args, stdin_text, timeout):
        try:
            r = subprocess.run([b] + args, cwd=tempfile.gettempdir(), env=dict(os.environ, NO_COLOR='1'),
                               input=(stdin_text or '').encode('utf-8'), stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        except subprocess.TimeoutExpired:
            raise HttpError(504, '%s took longer than %d s' % (os.path.basename(b), timeout))
        except OSError as e:
            raise HttpError(502, '%s did not run: %s' % (b, e))
        return r.returncode, r.stdout.decode('utf-8', 'replace'), r.stderr.decode('utf-8', 'replace')

    @staticmethod
    def completion(model, content, usage=None):
        return {'id': 'chatcmpl-' + secrets.token_hex(8), 'object': 'chat.completion', 'created': int(time.time()), 'model': model,
                'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': content}, 'finish_reason': 'stop'}],
                'usage': usage or {'prompt_tokens': 0, 'completion_tokens': 0, 'total_tokens': 0}}

    def cli_chat(self, st, full, model, body):
        system, prompt = self.flatten(body.get('messages') or [])
        b = st['bin']
        cli = st['entry']['cli']
        if cli == 'claude':
            args = ['-p', '--output-format', 'json', '--no-session-persistence', '--tools', '', '--strict-mcp-config']
            if model and model != 'default':
                args += ['--model', model]
            if system:
                args += ['--system-prompt', system]
            code, out, err = self.run_cli(b, args, prompt, 600)
            try:
                v = json.loads(out)
            except ValueError:
                v = None
            if not isinstance(v, dict) or v.get('is_error') or code != 0:
                said = (v.get('result') if isinstance(v, dict) else None) or err or out
                raise HttpError(502, 'claude -p failed (exit %s): %s' % (code, str(said)[:400]))
            u = v.get('usage') or {}
            pin = (u.get('input_tokens') or 0) + (u.get('cache_read_input_tokens') or 0) + (u.get('cache_creation_input_tokens') or 0)
            outt = u.get('output_tokens') or 0
            return self.completion(full, v.get('result') or '', {'prompt_tokens': pin, 'completion_tokens': outt, 'total_tokens': pin + outt})
        if cli == 'codex':
            out_file = os.path.join(tempfile.gettempdir(), 'dry-codex-%s.txt' % secrets.token_hex(6))
            args = ['exec', '--color', 'never', '--sandbox', 'read-only', '--skip-git-repo-check', '--ephemeral', '-o', out_file]
            if model and model != 'default':
                args += ['--model', model]
            args.append('-')
            code, out, err = self.run_cli(b, args, ('%s\n\n%s' % (system, prompt)) if system else prompt, 600)
            text = ''
            try:
                with open(out_file, encoding='utf-8') as fh:
                    text = fh.read()
                os.remove(out_file)
            except OSError:
                pass
            if code != 0 and not text:
                raise HttpError(502, 'codex exec failed (exit %s): %s' % (code, (err or out)[-400:]))
            return self.completion(full, text.strip())
        args = ['-p', ('%s\n\n%s' % (system, prompt)) if system else prompt, '-s', '--no-color', '--available-tools', '']
        if model and model != 'auto':
            args += ['--model', model]
        code, out, err = self.run_cli(b, args, None, 600)
        if code != 0:
            raise HttpError(502, 'copilot -p failed (exit %s): %s' % (code, (err or out)[:400]))
        return self.completion(full, out.strip())

    # ---- capabilities (docs/capabilities.md) ----

    def load_declared(self):
        out, errors = [], []
        if not self.cap_dir:
            return out, errors
        try:
            files = sorted(f for f in os.listdir(self.cap_dir) if f.endswith('.md'))
        except OSError:
            return out, errors
        for f in files:
            full = os.path.join(self.cap_dir, f)
            try:
                with open(full, encoding='utf-8') as fh:
                    d = parse_capability(f, fh.read())
                if d['id'] + '.md' != f:
                    raise CapError('%s: its id is `%s`: name the file %s.md' % (f, d['id'], d['id']))
                out.append(d)
            except (CapError, OSError, UnicodeDecodeError) as e:
                errors.append({'file': full, 'error': str(e)})
        return out, errors

    def cap_run(self, argv, env=None, timeout=30.0):
        return run_argv(argv, env, timeout, self.cap_dir or os.path.expanduser('~'))

    def probe_of(self, d, refresh=False):
        had = self.probes.get(d['id'])
        if had and not refresh and time.time() - had['at'] < PROBE_S:
            return had
        r = {'state': 'present', 'why': ''}
        if d['probe']:
            code, _, error = self.cap_run(d['probe'], None, 30)
            if code != 0:
                cmd = ' '.join(d['probe'])
                r = {'state': 'absent', 'why': ('probe `%s` did not run: %s' % (cmd, error)) if error else 'probe `%s` exited %s' % (cmd, code)}
        r['at'] = time.time()
        self.probes[d['id']] = r
        return r

    def find_dry(self):
        for p in (self.o.get('dry'), os.environ.get('DRY_BIN'), os.path.join(HERE, '..', 'target', 'release', 'dry'),
                  os.path.join(HERE, '..', 'target', 'debug', 'dry')):
            if p and os.path.isfile(p):
                return p
        return shutil.which('dry')

    def builtin_caps(self):
        def tool(name, desc, props, required, approval=False):
            return {'name': name, 'description': desc, 'inputSchema': {'type': 'object', 'properties': props, 'required': required}, 'approval': approval}

        git = self.runtime('git')
        containers = [n for n in ('docker', 'podman') if self.runtime(n)]
        dry = self.find_dry()

        def term_snap():
            with self.lock:
                run = [x for x in self.sessions.values() if x.status == 'running']
            return '\n'.join('running %s: %s' % (x.key or x.id, x.command) for x in run) if run else 'nothing running'

        def container_snap():
            code, out, error = run_argv([self.runtime(containers[0])['path'], 'ps', '--format', '{{.Names}} {{.Status}}'], None, 10)
            return (out.strip() or 'no containers running') if code == 0 else '%s ps failed: %s' % (containers[0], (error or out).strip())

        out = [
            {'id': 'file', 'state': 'present', 'doc': "The workspace folder's files: list, read, write, stat (over /v1/fs; list and read here). "
                                                      'The folder is %s. Secrets, .git and .dry are never shown.' % self.folder,
             'tools': [tool('file.list', 'The files under a folder of the workspace (at most 200), paths relative to it.',
                            {'path': {'type': 'string', 'description': 'a folder in the workspace; empty: all of it'}}, []),
                       tool('file.read', "One file's text.", {'path': {'type': 'string', 'description': 'relative to the workspace folder'}}, ['path'])]},
            {'id': 'terminal', 'state': 'present', 'doc': "Commands in the workspace folder, in this machine's shell (%s): run, start, tail, stop "
                                                          "(over /v1/term). A dry engine's terminal artifact uses it through the companion." % sys.platform,
             'tools': [tool('terminal.run', 'Run a command in the workspace folder (a shell) and wait: its exit code and last lines.',
                            {'command': {'type': 'string'}, 'cwd': {'type': 'string', 'description': 'a folder in the workspace (default: the folder)'}},
                            ['command'], True)],
             'snapshot': term_snap},
            {'id': 'browser', 'state': 'absent',
             'why': "the Python companion drives no browser (the standard library has no WebSocket client for Chrome's DevTools protocol): use the Node companion for it",
             'doc': 'A headless browser on this machine for its own pages (localhost): open, look, click, type, screenshot.',
             'tools': [dict(t, approval=False) for t in BROWSER_TOOLS]},
            {'id': 'evidence', 'state': 'present' if dry else 'absent', 'why': '' if dry else 'no dry binary to run evidence with (--dry PATH)',
             'doc': "A project's evidence (its evidence.md): run it, read the last one.", 'tools': [dict(t, approval=False) for t in EVIDENCE_TOOLS]},
            {'id': 'git', 'state': 'present' if git else 'absent', 'why': '' if git else 'no git on PATH',
             'doc': "git in this machine's projects, with its own credentials: status here; push and clone need a person's click.",
             'tools': [tool('git.status', "A project's branch and changed files.", {'path': {'type': 'string'}}, ['path'])]},
            {'id': 'container', 'state': 'present' if containers else 'absent', 'why': '' if containers else 'no docker or podman on PATH',
             'doc': 'Containers on this machine (%s): run them with the terminal.' % (', '.join(containers) or 'none'), 'tools': [],
             'snapshot': container_snap if containers else None},
        ]
        for pr in self.providers_now(False)['providers']:
            ok = pr['state'] == 'ok'
            models = pr['models']
            out.append({'id': 'model.' + pr['name'], 'state': 'present' if ok else 'absent',
                        'why': '' if ok else '%s: %s%s' % (pr['state'], pr['why'], (' (%s)' % pr['hint']) if pr['hint'] else ''),
                        'doc': 'Models from %s (%s%s), through this companion\'s /v1/chat/completions: %s.' % (
                            pr['name'], pr['kind'], (' at ' + pr['url']) if pr.get('url') else '', ', '.join(models) or 'none'),
                        'tools': [], 'models': models, 'snapshot': (lambda m=models: '\n'.join(m) or 'no models')})
        return out

    def switched(self, c):
        if self.is_on(c['id']):
            return c
        return dict(c, state='absent', off=True, why="switched off in the companion's terminal (type: on %s)" % c['id'])

    def capabilities(self, refresh=False):
        out = self.builtin_caps()
        declared, errors = self.load_declared()
        for d in declared:
            p = self.probe_of(d, refresh)
            out.append({'id': d['id'], 'state': p['state'], 'why': p['why'], 'doc': d['doc'], 'declared': d, 'interval': d['interval'],
                        'events': d['events'],
                        'tools': [{'name': '%s.%s' % (d['id'], t['name']), 'description': t['doc'], 'inputSchema': tool_schema(t),
                                   'approval': t['approval']} for t in d['tools']]})
        return [self.switched(c) for c in out], errors

    @staticmethod
    def cap_meta(c, extra):
        m = {'capability': c['id'], 'doc': c['doc'], 'state': c['state']}
        if c.get('why'):
            m['why'] = c['why']
        m.update(extra)
        return m

    def cap_tools(self):
        caps, _ = self.capabilities()
        return [dict({k: v for k, v in t.items() if k != 'approval'}, _meta=self.cap_meta(c, {'approval': bool(t.get('approval'))}))
                for c in caps for t in c['tools']]

    def cap_resources(self):
        caps, _ = self.capabilities()
        out = []
        for c in caps:
            d = c.get('declared')
            out.append({'uri': 'dry://capability/' + c['id'], 'name': c['id'], 'description': c['doc'].split('\n')[0], 'mimeType': 'text/plain',
                        '_meta': self.cap_meta(c, {'snapshot': bool((d and d['snapshot']) or c.get('snapshot')),
                                                   'interval': c['interval'] if d else None,
                                                   'events': [{'name': e['name'], 'when': when_text(e['when']), 'text': e['text']} for e in c.get('events') or []]})})
        return out

    def cap_read(self, uri):
        cid = re.sub(r'^dry://capability/', '', str(uri or ''))
        caps, _ = self.capabilities()
        c = next((x for x in caps if x['id'] == cid), None)
        if c is None:
            raise HttpError(404, 'no capability %s' % cid)
        if c['state'] != 'present':
            return '(%s: %s)' % (c['state'], c.get('why'))
        d = c.get('declared')
        if d and d['snapshot']:
            code, out, error = self.cap_run(d['snapshot'], None, 30)
            return out.rstrip() if code == 0 else 'snapshot failed: %s' % (error or out).strip()
        if c.get('snapshot'):
            return c['snapshot']()
        return ''

    def cap_call(self, name, args):
        """A call to a capability's tool; None: not one of them."""
        def text(t, is_error=False):
            return {'content': [{'type': 'text', 'text': t}], 'isError': is_error}

        args = args if isinstance(args, dict) else {}
        cap_id = name[:name.rfind('.')] if '.' in name else 'browser' if name.startswith('browser_') else 'evidence' if name.startswith('evidence_') else None
        if cap_id and not self.is_on(cap_id) and (cap_id in BUILTIN_IDS or any(d['id'] == cap_id for d in self.load_declared()[0])):
            self.need(cap_id)
        if name == 'terminal.run':
            command = str(args.get('command') or '')
            if not command:
                raise HttpError(400, 'terminal.run needs command')
            if needs_confirm(command):
                raise HttpError(428, "push, clone and publishing go through the git endpoints with a person's click")
            cwd = self.ws_path(str(args.get('cwd') or ''), dir_=True)[0]
            s = self.launch(cwd, command, 'run', None, 30 * 60 * 1000)
            s.done.wait()
            return text('exit %s\n%s' % (s.exit if s.exit is not None else s.signal, last_lines(s.output, 15)), s.exit != 0)
        if name == 'file.read':
            r = self.ws_read(str(args.get('path') or ''))
            return text(r['text']) if r['kind'] == 'text' else text('%s: %s (%s bytes)' % (r['path'], r['kind'], r['bytes']), True)
        if name == 'file.list':
            r = self.ws_list(str(args.get('path') or args.get('root') or ''), 200)
            return text('\n'.join(r['files']) + ('\n…' if r['truncated'] else ''))
        if name == 'git.status':
            proj = self.git_project(str(args.get('path') or ''))
            code, out, err = git(['status', '--porcelain=v1', '-b'], proj)
            return text(out if code == 0 else err, code != 0)
        if name in ('browser_open', 'browser_look', 'browser_click', 'browser_type', 'browser_screenshot'):
            return text('browser is absent: the Python companion drives no browser (use the Node companion for it)', True)
        if name in ('evidence_run', 'evidence_read'):
            r = self.evidence_run(args) if name == 'evidence_run' else self.evidence_read(self.project_dir(str(args.get('project') or '')))
            ex = '' if r.get('code') is None else ' (exit %s)' % r['code']
            return {'content': [{'type': 'text', 'text': 'evidence: %s%s\n\n%s' % (r['line'], ex, r.get('summary') or r.get('output') or '')}],
                    'isError': r.get('code') is not None and r['code'] > 1}
        if '.' not in name:
            return None
        dot = name.find('.')
        d = next((x for x in self.load_declared()[0] if x['id'] == name[:dot]), None)
        if d is None:
            return None
        p = self.probe_of(d)
        if p['state'] != 'present':
            return text('%s is %s: %s' % (d['id'], p['state'], p['why']), True)
        argv, env, timeout = capability_argv(d, name[dot + 1:], args)
        code, out, error = self.cap_run(argv, env, timeout)
        if error:
            return text(error, True)
        return text(out.rstrip() or '(done, no output)', code != 0)

    def mcp_call(self, name, args):
        try:
            r = self.cap_call(name, args)
        except HttpError as e:
            return {'content': [{'type': 'text', 'text': e.message}], 'isError': True, '_meta': {'code': e.code}}
        except Exception as e:  # noqa: BLE001
            return {'content': [{'type': 'text', 'text': str(e)}], 'isError': True, '_meta': {'code': 'internal'}}
        if r is None:
            raise HttpError(404, 'no tool %s' % name)
        return r

    def capabilities_summary(self, refresh):
        caps, errors = self.capabilities(refresh)
        return {'dir': self.cap_dir, 'list': [{'id': c['id'], 'state': c['state'], 'why': c.get('why') or '', 'tools': [t['name'] for t in c['tools']],
                                                'declared': bool(c.get('declared'))} for c in caps], 'errors': errors}

    def notify(self, method, params):
        msg = 'data: %s\n\n' % jdump({'jsonrpc': '2.0', 'method': method, 'params': params})
        with self.lock:
            for s in list(self.mcp_streams):
                s.send(msg)

    def cap_tick(self):
        """While a peer listens: the declared snapshots read at their intervals (an event that became
        true rides on its update), and the folder watched (a change is tools/list_changed)."""
        declared, _ = self.load_declared()
        for d in declared:
            self.probe_of(d)
        sig = jdump([[d['id'], d['doc'], [[t['name'], t['doc'], t['inputs'], t['command'], t['approval'], t['timeout']] for t in d['tools']],
                      d['events'], d['interval'], self.probes.get(d['id'], {}).get('state')] for d in declared])
        if self.cap_signature is not None and sig != self.cap_signature:
            self.notify('notifications/tools/list_changed', {})
        self.cap_signature = sig
        for d in declared:
            if not d['snapshot'] or self.probe_of(d)['state'] != 'present':
                continue
            had = self.snaps.get(d['id'])
            if had and time.time() - had['at'] < d['interval']:
                continue
            code, out, error = self.cap_run(d['snapshot'], None, 30)
            txt = out.rstrip() if code == 0 else 'snapshot failed: %s' % (error or out).strip()
            holds = {e['name']: when_holds(e['when'], txt) for e in d['events']}
            self.snaps[d['id']] = {'text': txt, 'at': time.time(), 'holds': holds}
            if not had:
                continue  # the first reading never fires
            uri = 'dry://capability/' + d['id']
            fired = [e for e in d['events'] if holds[e['name']] and not had['holds'].get(e['name'])]
            for e in fired:
                self.log('%s  capability %s: event %s' % (short_time(), d['id'], e['name']))
                self.notify('notifications/resources/updated', {'uri': uri, '_meta': {'event': {'name': e['name'], 'text': e['text']}}})
            if not fired and txt != had['text']:
                self.notify('notifications/resources/updated', {'uri': uri})

    def ticker(self):
        while not self.stopped.is_set():
            time.sleep(1.0)
            with self.lock:
                listening = bool(self.mcp_streams)
            if listening:
                try:
                    self.cap_tick()
                except Exception as e:  # noqa: BLE001
                    self.log('%s  capabilities: %s' % (short_time(), e))

    # ---- git and evidence (outside the contract) ----

    def project_dir(self, p):
        """A project folder in the workspace: relative to it, or absolute inside it."""
        if p and os.path.isabs(p):
            real = os.path.realpath(p)
            if not within(real, self.folder):
                raise HttpError(403, '%s is outside the folder' % p, 'path_outside')
            p = os.path.relpath(real, self.folder).replace(os.sep, '/')
        return self.ws_path(p, dir_=True)[0]

    def git_project(self, p):
        d = self.project_dir(p)
        while True:
            if os.path.exists(os.path.join(d, '.git')):
                return d
            if d == self.folder:
                raise HttpError(400, '%s is not inside a git project' % p)
            d = os.path.dirname(d)

    def evidence_read(self, project):
        d = os.path.join(project, 'evidence')
        manifest, summary = None, ''
        try:
            with open(os.path.join(d, 'manifest.json'), encoding='utf-8') as fh:
                manifest = json.load(fh)
        except (OSError, ValueError):
            pass
        try:
            with open(os.path.join(d, 'summary.md'), encoding='utf-8') as fh:
                summary = fh.read()
        except OSError:
            pass
        line = next((l for l in summary.split('\n') if re.search(r' · (pass|fail)$', l)), None)
        if line is None:
            line = ('%d shots · %s' % (len(manifest.get('shots') or []), manifest.get('status'))) if isinstance(manifest, dict) else 'no evidence yet'
        return {'project': project, 'line': line, 'summary': summary, 'manifest': manifest}

    def evidence_run(self, b):
        project = self.project_dir(str(b.get('project') or ''))
        if not os.path.exists(os.path.join(project, 'evidence.md')):
            raise HttpError(404, '%s has no evidence.md (docs/evidence.md)' % project)
        binp = self.find_dry()
        if not binp:
            raise HttpError(503, 'no dry binary: build one (cargo build -p dry-tools) or give --dry PATH')
        args = [binp, 'evidence', '--project', project, '--quiet']
        if b.get('update'):
            args.append('--update')
        if b.get('expect'):
            e = re.sub(r'\s+', '', str(b['expect']))
            if not re.match(r'^[A-Za-z0-9_@,-]+$', e):
                raise HttpError(400, 'expect is scenario names (or name@WxH), comma separated')
            args += ['--expect', e]
        t0 = time.time()
        self.log('%s  evidence %s' % (short_time(), project))
        try:
            proc = popen_group(args, cwd=project, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        except OSError as e:
            return dict(self.evidence_read(project), code=-1, output=str(e), ms=0)
        with self.lock:
            self.evidence_runs.add(proc)
        out, _ = proc.communicate()
        with self.lock:
            self.evidence_runs.discard(proc)
        r = dict(self.evidence_read(project), code=proc.returncode, output=last_lines(out.decode('utf-8', 'replace')[-MAX_OUTPUT:], 20),
                 ms=int((time.time() - t0) * 1000))
        self.log('%s  evidence: %s (exit %s)' % (short_time(), r['line'], proc.returncode))
        return r

    # ---- start-up ----

    def checks(self):
        """What this machine has, each found or missing with how to fix it."""
        out = [{'what': 'folder', 'name': 'folder', 'ok': True, 'line': '%s (state in %s/)' % (self.folder, DRY_DIR)}]
        providers, perr, exists = self.declared_providers()
        if not exists and not providers:
            out.append({'what': 'providers', 'name': self.providers_file, 'ok': False, 'line': 'none declared',
                        'hint': 'paste entries into %s (the examples are in it), or add one in dry → Settings → Companion' % self.providers_file})
        for e in perr:
            out.append({'what': 'providers', 'name': e['entry'] if e['entry'] is not None else 'providers.json', 'ok': False, 'line': e['error']})
        self.detect_runtimes(['git', 'docker', 'podman'])
        for r in self.probe_providers():
            pr = self.public_provider(r)
            ok = pr['state'] == 'ok'
            if ok:
                n = len(pr['models'])
                line = '%s%s: %d model%s%s' % (pr['kind'], (' ' + pr['url']) if pr.get('url') else '', n, '' if n == 1 else 's',
                                              (' (%s, %s)' % (pr['runner'], pr['accel'])) if pr.get('runner') else '')
            else:
                line = '%s: %s' % (pr['kind'], pr['why'])
            out.append({'what': 'provider', 'name': pr['name'], 'ok': ok, 'state': pr['state'], 'line': line, 'hint': pr['hint']})
        declared, errors = self.load_declared()
        for d in declared:
            pb = self.probe_of(d, True)
            ok = pb['state'] == 'present'
            out.append({'what': 'capability', 'name': d['id'], 'ok': ok, 'state': None if ok else 'absent',
                        'line': d['doc'].split('\n')[0] if ok else pb['why'], 'hint': '' if ok else 'its probe says this machine lacks it'})
        for e in errors:
            out.append({'what': 'capability', 'name': os.path.basename(e['file']), 'ok': False, 'line': e['error'], 'hint': 'fix the line it names'})
        g = self.runtime('git')
        out.append({'what': 'git', 'name': 'git', 'ok': True, 'line': g['version'] or g['path']} if g else
                   {'what': 'git', 'name': 'git', 'ok': False, 'line': 'not on PATH', 'hint': 'install git (https://git-scm.com/downloads)'})
        out.append({'what': 'chrome', 'name': 'browser', 'ok': False, 'state': 'absent', 'line': 'the Python companion drives no browser',
                    'hint': 'use the Node companion (companion/dry-companion.mjs) for the browser capability'})
        ct = next((n for n in ('docker', 'podman') if self.runtime(n)), None)
        out.append({'what': 'container', 'name': ct, 'ok': True, 'line': self.runtime(ct)['version'] or self.runtime(ct)['path']} if ct else
                   {'what': 'container', 'name': 'container', 'ok': False, 'line': 'no docker or podman',
                    'hint': 'install Docker Desktop or Podman (optional: containers)'})
        return out

    def switch_list(self):
        caps, _ = self.capabilities()
        return [{'id': c['id'], 'on': self.is_on(c['id']), 'actuator': self.actuator(c['id']), 'saved': isinstance(self.config['switches'].get(c['id']), bool),
                 'doc': c['doc'].split('\n')[0], 'state': 'present' if c.get('off') else c['state'], 'why': '' if c.get('off') else c.get('why') or ''}
                for c in caps]

    def listen(self):
        self.server = CompanionServer(('127.0.0.1', self.port), Handler)
        self.server.companion = self
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, kwargs={'poll_interval': 0.25}, daemon=True).start()
        threading.Thread(target=self.ticker, daemon=True).start()
        return self.port

    def announce(self):
        p = self.port
        print('%s %s on http://127.0.0.1:%d  (protocol %d, python %s)' % (NAME, VERSION, p, PROTOCOL, platform.python_version()), flush=True)
        print('listening http://127.0.0.1:%d' % p, flush=True)
        print('folder: %s' % self.folder, flush=True)
        print('pages allowed: https://kaush4l.github.io, http://localhost:*, http://127.0.0.1:*%s' % (
            (', ' + ', '.join(self.extra_origins)) if self.extra_origins else ''), flush=True)
        if self.app_dir:
            print('the app: http://localhost:%d/app/  (from %s)' % (p, self.app_dir), flush=True)
        print('MCP: http://localhost:%d/mcp  (Bearer: a pairing token)' % p, flush=True)
        print('type: pairings · revoke <id> · on|off <id> · switches · providers · code · capabilities · stop-all   (Esc: stop all)', flush=True)
        self.announce_code()

    def command(self, line):
        """A line typed in the companion's terminal: what to print."""
        w = line.strip().split()
        cmd, arg = (w[0] if w else ''), (w[1] if len(w) > 1 else '')
        if cmd == '':
            return ''
        if cmd == 'pairings':
            return self.pairings_text()
        if cmd == 'revoke':
            return ('  revoked %s' % arg) if self.revoke(arg) else '  no pairing %s' % (arg or '(give its id)')
        if cmd == 'stop-all':
            r = self.stop_all()
            return '  stopped %d session(s)' % r['stopped']
        if cmd == 'code':
            return '  pairing code: %s' % self.code
        if cmd in ('on', 'off'):
            if not arg:
                return '  %s <id>: which capability (type: switches)' % cmd
            if not any(x['id'] == arg for x in self.switch_list()):
                return '  no capability %s (type: switches)' % arg
            self.set_switch(arg, cmd == 'on')
            return '  %s %s' % (arg, cmd)
        if cmd == 'switches':
            return '\n'.join('  %s  %s%s%s' % ('on ' if x['on'] else 'off', x['id'].ljust(16), ' actuator' if x['actuator'] else '',
                                              (' (%s: %s)' % (x['state'], x['why'])) if x['state'] != 'present' else '') for x in self.switch_list())
        if cmd == 'providers':
            v = self.providers_now(True)
            lines = ['  %s%s' % (v['file'], '' if v['exists'] else ' (not there yet)')]
            for x in v['providers']:
                lines.append('  %s %s %s%s' % (x['state'].ljust(8), x['name'].ljust(12), x['kind'],
                                                (': ' + ', '.join(x['models'])) if x['state'] == 'ok' else ': %s%s' % (x['why'], (' → ' + x['hint']) if x['hint'] else '')))
            lines += ['  ! %s' % e['error'] for e in v['errors']]
            return '\n'.join(lines)
        if cmd == 'capabilities':
            c = self.capabilities_summary(True)
            lines = ['  %s %s%s%s%s' % (x['id'].ljust(10), x['state'].ljust(12), ' declared' if x['declared'] else '', (' — ' + x['why']) if x['why'] else '',
                                        ('\n             tools: ' + ', '.join(x['tools'])) if x['tools'] else '') for x in c['list']]
            return '\n'.join(lines + ['  ! %s' % e['error'] for e in c['errors']])
        if cmd == 'tools':
            return '  no custom tools in the Python companion (tools/*.md: the Node companion)'
        return '  commands: pairings · revoke <id> · on <id> · off <id> · switches · providers · stop-all · code · capabilities'

    def close(self):
        self.stopped.set()
        with self.lock:
            sessions = list(self.sessions.values())
            ev = list(self.evidence_runs)
            streams = list(self.listeners) + list(self.mcp_streams)
        for s in sessions:
            self.kill(s)
        for p in ev:
            kill_group(p)
        for st in list(self.provider_states.values()):
            self.stop_runner(st)
        for s in streams:
            s.close()
        if self.server:
            threading.Thread(target=self.server.shutdown, daemon=True).start()


def git(args, cwd, timeout=120):
    try:
        r = subprocess.run(['git'] + args, cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout,
                           env=dict(os.environ, GIT_TERMINAL_PROMPT='0', LC_ALL='C'))
    except (OSError, subprocess.TimeoutExpired) as e:
        return None, '', str(e)
    return r.returncode, r.stdout.decode('utf-8', 'replace'), r.stderr.decode('utf-8', 'replace')


def atomic_write(path, data, tmp=None):
    tmp = tmp or '%s.dry-%s.tmp' % (path, secrets.token_hex(4))
    with open(tmp, 'wb') as fh:
        fh.write(data)
    os.replace(tmp, path)


def sse_text(e):
    return 'id: %d\nevent: %s\ndata: %s\n\n' % (e['id'], e['event'], jdump(e['data']))


BUILTIN_IDS = ('file', 'terminal', 'browser', 'evidence', 'git', 'container')
MCP_MODERN = ('2026-07-28',)
MCP_LEGACY = ('2025-11-25', '2025-06-18', '2025-03-26')
BROWSER_TOOLS = [
    {'name': 'browser_open', 'description': 'Open a page of this machine (localhost) in a headless browser: its text, console errors and a screenshot.',
     'inputSchema': {'type': 'object', 'properties': {'url': {'type': 'string'}}, 'required': ['url']}},
    {'name': 'browser_look', 'description': 'The open page as text (its accessibility tree) and its console errors.', 'inputSchema': {'type': 'object', 'properties': {}}},
    {'name': 'browser_click', 'description': 'Click an element of the open page: a CSS selector or the text it shows.',
     'inputSchema': {'type': 'object', 'properties': {'target': {'type': 'string'}}, 'required': ['target']}},
    {'name': 'browser_type', 'description': 'Type into a field of the open page; press: a key after (Enter).',
     'inputSchema': {'type': 'object', 'properties': {'target': {'type': 'string'}, 'text': {'type': 'string'}, 'press': {'type': 'string'}}, 'required': ['target', 'text']}},
    {'name': 'browser_screenshot', 'description': 'A screenshot of the open page (a PNG file, and a small JPEG).', 'inputSchema': {'type': 'object', 'properties': {}}},
]
EVIDENCE_TOOLS = [
    {'name': 'evidence_run', 'description': "Run a project's evidence (its evidence.md): build, start the app, screenshot every scenario and compare "
                                            'with the committed evidence/ folder. One line back, then the summary.',
     'inputSchema': {'type': 'object', 'properties': {'project': {'type': 'string'}, 'expect': {'type': 'string', 'description': 'scenarios expected to change, comma separated'},
                                                      'update': {'type': 'boolean'}}, 'required': ['project']}},
    {'name': 'evidence_read', 'description': "A project's last evidence: one line, then the summary (which shots changed, by how much, the page-text lines).",
     'inputSchema': {'type': 'object', 'properties': {'project': {'type': 'string'}}, 'required': ['project']}},
]
QUIET_GETS = {'/v1/fs/read', '/v1/fs/list', '/v1/fs/stat', '/v1/term/tail', '/v1/health', '/v1/events/log', '/v1/workspace', '/v1/pairings',
              '/v1/capabilities', '/v1/providers', '/v1/models', '/v1/store', '/v1/switches'}
QUIET_POSTS = {'/v1/store', '/v1/workspace/snapshot'}


# ---- HTTP ----

class CompanionServer(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = not WIN

    def handle_error(self, request, client_address):
        pass  # a peer that left mid-answer is not news


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    server_version = NAME + '/' + VERSION
    sys_version = ''

    def log_message(self, fmt, *args):
        pass

    def do_GET(self):
        self.handle_any()

    do_POST = do_OPTIONS = do_HEAD = do_PUT = do_DELETE = do_PATCH = do_GET

    # ---- answers ----

    def cors(self):
        c = self.server.companion
        o = self.origin
        return {'Access-Control-Allow-Origin': o, 'Vary': 'Origin'} if origin_allowed(o, c.extra_origins) else {}

    def answer(self, status, headers, body=b''):
        self.send_response(status)
        for k, v in headers.items():
            self.send_header(k, v)
        if status not in (204, 304):
            self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        if body and self.command != 'HEAD':
            self.wfile.write(body)
        self.status = status

    def send_json(self, status, obj, cors=True):
        h = dict(self.cors()) if cors else {}
        h['Content-Type'] = 'application/json; charset=utf-8'
        h['Cache-Control'] = 'no-store'
        self.answer(status, h, jdump(obj).encode('utf-8'))

    def start_chunked(self, status, headers):
        self.send_response(status)
        for k, v in headers.items():
            self.send_header(k, v)
        self.send_header('Transfer-Encoding', 'chunked')
        self.end_headers()
        self.status = status
        self.chunked = True

    def chunk(self, data):
        if isinstance(data, str):
            data = data.encode('utf-8')
        if data:
            self.wfile.write(b'%x\r\n%s\r\n' % (len(data), data))

    def end_chunked(self):
        try:
            self.wfile.write(b'0\r\n\r\n')
        except OSError:
            pass

    def serve_stream(self, stream, first, on_end=None):
        """A server-sent stream: `first`, then what its queue gets, a `: ping` every heartbeat."""
        c = self.server.companion
        self.close_connection = True
        try:
            self.start_chunked(200, dict(self.cors(), **{'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store'}))
            self.chunk(first)
            hb = c.heartbeat
            next_ping = time.time() + hb
            while not c.stopped.is_set():
                try:
                    msg = stream.q.get(timeout=max(0.0, next_ping - time.time()))
                except queue.Empty:
                    self.chunk(': ping\n\n')
                    next_ping = time.time() + hb
                    continue
                if msg is None:
                    break
                self.chunk(msg)
            self.end_chunked()
        except OSError:
            pass
        finally:
            stream.closed = True
            if on_end:
                on_end()

    # ---- the body ----

    def read_raw(self):
        if self.raw is not None:
            return self.raw
        te = (self.headers.get('Transfer-Encoding') or '').lower()
        data = b''
        if 'chunked' in te:
            while True:
                line = self.rfile.readline(65537)
                try:
                    n = int(line.split(b';')[0].strip() or b'0', 16)
                except ValueError:
                    self.close_connection = True
                    raise HttpError(400, 'the body does not read')
                if n == 0:
                    while self.rfile.readline(65537) not in (b'\r\n', b'\n', b''):
                        pass
                    break
                if len(data) + n > MAX_BODY:
                    self.close_connection = True
                    raise HttpError(413, 'the body is too large')
                data += self.rfile.read(n)
                self.rfile.readline(3)
        else:
            try:
                n = int(self.headers.get('Content-Length') or 0)
            except ValueError:
                n = 0
            if n > MAX_BODY:
                self.close_connection = True
                raise HttpError(413, 'the body is too large')
            data = self.rfile.read(n) if n > 0 else b''
        self.raw = data
        return data

    def body(self):
        raw = self.read_raw()
        if not raw:
            return {}
        try:
            v = json.loads(raw.decode('utf-8'))
        except ValueError:
            raise HttpError(400, 'the body is not JSON')
        return v if isinstance(v, dict) else {}

    def token(self):
        h = self.headers.get('x-dry-token')
        if h:
            return h
        a = self.headers.get('Authorization') or ''
        if a.startswith('Bearer '):
            return a[7:].strip()
        return None

    # ---- the routes ----

    def handle_any(self):
        c = self.server.companion
        t0 = time.time()
        self.origin = self.headers.get('Origin')
        self.raw = None
        self.status = 200
        self.chunked = False
        self.note = ''
        u = urllib.parse.urlsplit(self.path)
        p = u.path
        self.p = p
        q = dict(urllib.parse.parse_qsl(u.query, keep_blank_values=True))
        try:
            self.route(c, p, q, t0)
        except HttpError as e:
            self.fail(e.status, e.message, e.code)
        except (BrokenPipeError, ConnectionResetError):
            self.close_connection = True
        except Exception as e:  # noqa: BLE001
            self.fail(500, '%s: %s' % (e.__class__.__name__, e), 'internal')
        finally:
            if self.raw is None and self.command in ('POST', 'PUT', 'PATCH'):
                self.close_connection = True  # a body never read: the connection cannot be reused
            self.done(c, p, t0)

    def fail(self, status, message, code):
        self.note = message[:80]
        if self.chunked:
            self.close_connection = True
            return
        try:
            self.send_json(status, {'error': message, 'code': code if re.match(r'^[a-z_]+$', code or '') else CODES.get(status, 'internal')})
        except OSError:
            self.close_connection = True

    def done(self, c, p, t0):
        if not c.verbose:
            if p == '/favicon.ico':
                return
            if self.status < 400 and (self.note == 'app' or (self.command == 'GET' and p in QUIET_GETS)
                                      or (self.command == 'POST' and p in QUIET_POSTS) or (p == '/mcp' and self.note == '')):
                return
        o = self.origin
        c.log('%s  %s %s %s %dms%s%s' % (short_time(), self.command.ljust(4), p, self.status, int((time.time() - t0) * 1000),
                                       (' ' + re.sub(r'^https?://', '', o)) if o else '', (' ' + self.note) if self.note else ''))

    def route(self, c, p, q, t0):
        m = self.command
        if not host_allowed(self.headers.get('Host'), c.port):
            return self.send_json(403, {'error': 'this server answers only to 127.0.0.1 / localhost', 'code': 'host_refused'}, cors=False)
        if self.origin and not origin_allowed(self.origin, c.extra_origins):
            return self.send_json(403, {'error': 'origin %s is not allowed (add it with --origin)' % self.origin, 'code': 'origin_refused'}, cors=False)
        if m == 'OPTIONS':
            return self.answer(204, dict(self.cors(), **{
                'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
                'Access-Control-Allow-Headers': 'content-type, authorization, x-dry-token, last-event-id, mcp-protocol-version, mcp-method, mcp-name, accept',
                'Access-Control-Allow-Private-Network': 'true',
                'Access-Control-Max-Age': '600'}))
        if not p.startswith('/v1/') and not p.startswith('/api/') and p != '/mcp':
            if self.serve_app(c, p):
                self.note = 'app'
                return
            if c.app_dir and m in ('GET', 'HEAD'):
                raise HttpError(404, 'no file %s in the app' % p, 'not_found')
        tok = self.token()
        if p == '/mcp':
            pairing = c.pairing_of(tok)
            if not pairing:
                h = dict(self.cors(), **{'Content-Type': 'application/json', 'WWW-Authenticate': 'Bearer realm="dry-companion"'})
                return self.answer(401, h, jdump({'error': 'not paired: use the pairing token as a Bearer token', 'code': 'not_paired'}).encode('utf-8'))
            return self.handle_mcp(c, pairing)
        if p == '/v1/health' and m == 'GET':
            return self.send_json(200, {'ok': True, 'name': NAME, 'version': VERSION, 'protocol': PROTOCOL, 'paired': bool(c.pairing_of(tok)),
                                        'app': bool(c.app_dir), 'impl': IMPL})
        if p == '/v1/pair' and m == 'POST':
            b = self.body()
            r = c.pair(b.get('code'), self.origin)
            if r is None:
                raise HttpError(401, 'wrong pairing code: use the one this companion printed', 'wrong_code')
            token, pr = r
            self.note = 'paired ' + pr['id']
            return self.send_json(200, {'token': token, 'id': pr['id'], 'expires': int(pr['expires']), 'ttl': int(c.ttl), 'name': NAME,
                                        'version': VERSION, 'protocol': PROTOCOL})
        pairing = c.pairing_of(tok)
        if not pairing:
            raise HttpError(401, 'not paired (or the pairing expired or was revoked): send a pairing code to /v1/pair', 'not_paired')
        get, post = m == 'GET', m == 'POST'
        if p == '/v1/unpair' and post:
            with c.lock:
                pr = c.pairings.pop(tok, None)
            if pr:
                c.close_streams(pr)
                c.log('%s  pairing %s left' % (short_time(), pr['id']))
            return self.send_json(200, {'ok': True})
        if p == '/v1/pairings' and get:
            return self.send_json(200, {'pairings': c.list_pairings(tok)})
        if p == '/v1/pairings/revoke' and post:
            b = self.body()
            pid = str(b.get('id') or '')
            if not c.revoke(pid):
                raise HttpError(404, 'no pairing %s' % pid, 'not_found')
            self.note = 'revoked ' + pid
            return self.send_json(200, {'ok': True})
        if p == '/v1/stop-all' and post:
            r = c.stop_all()
            self.note = 'stopped %d' % r['stopped']
            return self.send_json(200, r)
        if p == '/v1/tools' and get:
            return self.send_json(200, {'tools': [], 'errors': []})
        if p == '/v1/evidence/run' and post:
            c.need('evidence')
            r = c.evidence_run(self.body())
            self.note = 'evidence ' + r['line']
            return self.send_json(200, r)
        if p == '/v1/evidence' and get:
            return self.send_json(200, c.evidence_read(c.project_dir(q.get('project', ''))))
        if p == '/v1/evidence/file' and get:
            project = c.project_dir(q.get('project', ''))
            n = q.get('name', '')
            if not re.match(r'^[A-Za-z0-9_@.-]+\.png$', n) or n.startswith('.'):
                raise HttpError(400, 'not an evidence image')
            full = os.path.realpath(os.path.join(project, 'evidence', n))
            if not within(full, os.path.join(project, 'evidence')) or not os.path.isfile(full):
                raise HttpError(404, 'no such file: %s' % n)
            with open(full, 'rb') as fh:
                data = fh.read()
            return self.answer(200, dict(self.cors(), **{'Content-Type': 'image/png', 'Cache-Control': 'no-store'}), data)
        if p.startswith('/v1/browser/') and post:
            c.need('browser')
            raise HttpError(503, 'the Python companion drives no browser (use the Node companion for it)', 'unavailable')
        if p == '/v1/events/log' and get:
            since = num(q.get('since'), 0)
            with c.lock:
                evs = [e for e in c.events if e['id'] > since]
                last = c.next_event - 1
            return self.send_json(200, {'events': evs, 'last': last})
        if p == '/v1/capabilities' and get:
            rt = c.detect_runtimes() if (q.get('refresh') or c.runtimes is None or len(c.runtimes) < len(RUNTIMES)) else c.runtimes
            return self.send_json(200, {
                'name': NAME, 'version': VERSION, 'protocol': PROTOCOL, 'impl': IMPL, 'platform': '%s %s' % (sys.platform, platform.machine()),
                'hostname': socket.gethostname(), 'python': platform.python_version(), 'roots': [c.folder], 'folder': c.folder, 'runtimes': rt,
                'providers': c.providers_now(False)['providers'], 'store': c.store_dir, 'tools': [], 'tool_errors': [],
                'browser': {'chrome': None, 'why': 'the Python companion drives no browser'}, 'evidence': {'dry': c.find_dry()},
                'capabilities': c.capabilities_summary(bool(q.get('refresh'))), 'app': c.app_dir, 'mcp': '/mcp', 'events': c.next_event - 1})
        if p == '/v1/workspace' and get:
            return self.send_json(200, c.workspace_info())
        if p == '/v1/workspace/snapshot' and post:
            r = c.snapshot_write(self.body().get('snapshot'))
            self.note = 'snapshot saved'
            return self.send_json(200, r)
        if p.startswith('/v1/fs/'):
            c.need('file')
            op = p[len('/v1/fs/'):]
            if op == 'list' and get:
                mx = int(max(1, min(num(q.get('max'), 1000), 10000)))
                return self.send_json(200, c.ws_list(q.get('path', ''), mx))
            if op == 'read' and get:
                return self.send_json(200, c.ws_read(q.get('path', '')))
            if op == 'stat' and get:
                return self.send_json(200, c.ws_stat(q.get('path', '')))
            if op == 'write' and post:
                b = self.body()
                r = c.ws_write(b.get('path'), b.get('text'))
                self.note = 'wrote %s (%d bytes)' % (r['path'], r['bytes'])
                return self.send_json(200, r)
        if p.startswith('/v1/term/'):
            op = p[len('/v1/term/'):]
            if op == 'run' and post:
                b = self.body()
                key, command, cwd = c.term_prep(b)
                timeout = min(max(num(b.get('timeout_ms'), 30 * 60 * 1000), 100), MAX_TIMEOUT_MS)
                c.log('%s  run %s: %s' % (short_time(), key, command[:80]))
                s = c.launch(cwd, command, 'run', key, timeout)
                s.done.wait()
                self.note = '%s exit %s' % (key, s.exit if s.exit is not None else s.signal)
                return self.send_json(200, dict(c.term_session(s), output=s.output[-64 * 1024:], ms=int((time.time() - t0) * 1000)))
            if op == 'start' and post:
                b = self.body()
                key, command, cwd = c.term_prep(b)
                mode = 'background' if b.get('callback') is False else 'start'
                timeout = min(max(num(b.get('timeout_ms'), 0), 100), MAX_TIMEOUT_MS) if b.get('timeout_ms') else 0
                c.log('%s  %s %s: %s' % (short_time(), mode, key, command[:80]))
                s = c.launch(cwd, command, mode, key, timeout)
                return self.send_json(200, c.term_session(s))
            if op == 'tail' and get:
                c.need('terminal')
                key = c.term_key(q.get('key'))
                s = c.find_session(key)
                lines = int(max(0, min(num(q.get('lines'), 30), 2000)))
                return self.send_json(200, c.term_session(s, lines) if s else {'key': key, 'status': 'unknown', 'exit': None, 'tail': ''})
            if op == 'stop' and post:
                c.need('terminal')
                b = self.body()
                s = c.find_session(c.term_key(b.get('key')))
                if not s:
                    raise HttpError(404, 'no session %s' % b.get('key'), 'not_found')
                c.kill(s)
                self.note = 'stop %s' % b.get('key')
                return self.send_json(200, c.term_session(s))
        if p == '/v1/providers' and get:
            return self.send_json(200, c.providers_now(bool(q.get('refresh'))))
        if p == '/v1/providers' and post:
            b = self.body()
            r = c.add_provider(b.get('provider') if 'provider' in b else b)
            self.note = 'provider %s %s' % (r['name'], r['state'])
            return self.send_json(200, {'provider': r})
        if p == '/v1/switches' and get:
            caps, _ = c.capabilities()
            return self.send_json(200, {'file': c.config_file, 'switches': [{'id': x['id'], 'on': c.is_on(x['id']), 'actuator': c.actuator(x['id'])} for x in caps]})
        if p == '/v1/events' and get:
            return self.events_stream(c, q, pairing)
        if p in ('/v1/models', '/api/v0/models') and get:
            return self.send_json(200, {'object': 'list', 'data': c.all_models()})
        if p == '/v1/chat/completions' and post:
            return self.chat(c, self.body())
        if p == '/v1/store' and get:
            return self.send_json(200, {'texts': c.store_read()})
        if p == '/v1/store' and post:
            b = self.body()
            put, remove = b.get('put') or {}, b.get('remove') or []
            c.store_write(put, remove)
            self.note = '%d put, %d removed' % (len(put), len(remove))
            return self.send_json(200, {'ok': True})
        raise HttpError(404, 'no %s %s' % (m, p), 'not_found')

    def events_stream(self, c, q, pairing):
        since_raw = q.get('since') if 'since' in q else self.headers.get('Last-Event-ID')
        since = None
        if since_raw is not None:
            try:
                since = float(since_raw) if since_raw.strip() else 0.0
            except ValueError:
                since = None
        s = Stream(pairing, q.get('id') or None, q.get('all') == '1')
        with c.lock:
            running = len([x for x in c.sessions.values() if x.status == 'running'])
            first = 'event: hello\ndata: %s\n\n' % jdump({'sessions': running, 'last': c.next_event - 1})
            if since is not None and since == since:
                first += ''.join(sse_text(e) for e in c.events if e['id'] > since and c.wants(s, e))
            c.listeners.add(s)
        self.note = 'stream open'

        def end():
            with c.lock:
                c.listeners.discard(s)

        self.serve_stream(s, first, end)

    def chat(self, c, b):
        model = str(b.get('model') or '')
        self.note = model
        name, _, mid = model.partition('/')
        if name not in c.provider_states:
            c.providers_now(False)
        st = c.provider_states.get(name)
        if not st or not any(p['name'] == name for p in c.declared_providers()[0]):
            raise HttpError(404, 'no provider %s: declare it in %s (docs/companion-protocol.md §8)' % (name, c.providers_file), 'no_provider')
        if st['state'] != 'ok':
            raise HttpError(503, 'the provider %s is %s: %s' % (name, st['state'], st.get('why')), 'provider_down')
        if st['entry']['kind'] != 'cli':
            return self.passthrough(st['url'], mid, b)
        reply = c.cli_chat(st, model, mid, b)
        if b.get('stream'):
            chunk = dict(reply, object='chat.completion.chunk',
                         choices=[{'index': 0, 'delta': {'role': 'assistant', 'content': reply['choices'][0]['message']['content']}, 'finish_reason': 'stop'}])
            body = ('data: %s\n\ndata: [DONE]\n\n' % jdump(chunk)).encode('utf-8')
            return self.answer(200, dict(self.cors(), **{'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store'}), body)
        return self.send_json(200, reply)

    def passthrough(self, base, model, b):
        conn, path = connection(base, 600)
        try:
            data = jdump(dict(b, model=model)).encode('utf-8')
            conn.request('POST', path + '/v1/chat/completions', body=data, headers={'Content-Type': 'application/json'})
            up = conn.getresponse()
        except (OSError, http.client.HTTPException) as e:
            raise HttpError(502, 'could not reach %s (%s): is the model server running?' % (base, why_of(e)))
        ctype = up.getheader('Content-Type') or 'application/json'
        try:
            if 'event-stream' in ctype:
                self.start_chunked(up.status, dict(self.cors(), **{'Content-Type': ctype}))
                while True:
                    line = up.readline()
                    if not line:
                        break
                    self.chunk(line)
                self.end_chunked()
            else:
                self.answer(up.status, dict(self.cors(), **{'Content-Type': ctype}), up.read())
        finally:
            conn.close()

    def handle_mcp(self, c, pairing):
        def reply(status, body, extra=None):
            h = dict(self.cors(), **{'Content-Type': 'application/json'})
            if extra:
                h.update(extra)
            self.answer(status, h, b'' if body is None else jdump(body).encode('utf-8'))

        def err(status, mid, code, message, data=None):
            e = {'code': code, 'message': message}
            if data:
                e['data'] = data
            body = {'jsonrpc': '2.0'}
            if mid is not _MISSING:
                body['id'] = mid
            body['error'] = e
            reply(status, body)

        m = self.command
        if m == 'GET' and 'text/event-stream' in (self.headers.get('Accept') or ''):
            return self.mcp_stream(c, pairing)
        if m != 'POST':
            return self.answer(405, dict(self.cors(), Allow='POST'))
        raw = self.read_raw()
        try:
            msg = json.loads(raw.decode('utf-8')) if raw else {}
        except ValueError:
            return err(400, None, -32700, 'Parse error')
        if not isinstance(msg, dict) or msg.get('jsonrpc') != '2.0' or not isinstance(msg.get('method'), str):
            return err(400, None, -32600, 'Invalid Request')
        if 'id' not in msg:
            return self.answer(202, self.cors())
        mid, method = msg['id'], msg['method']
        params = msg.get('params') if isinstance(msg.get('params'), dict) else {}
        meta = params.get('_meta') if isinstance(params.get('_meta'), dict) else {}
        ver = meta.get('io.modelcontextprotocol/protocolVersion', _MISSING)
        header = self.headers.get('MCP-Protocol-Version')
        supported = list(MCP_MODERN) + list(MCP_LEGACY)
        if ver is not _MISSING:
            if ver not in MCP_MODERN:
                return err(400, mid, -32022, 'Unsupported protocol version', {'supported': supported, 'requested': ver})
            if header != ver:
                return err(400, mid, -32020, 'Header mismatch: MCP-Protocol-Version %s does not match %s' % (header or '(missing)', ver))
            if self.headers.get('Mcp-Method') != method:
                return err(400, mid, -32020, 'Header mismatch: Mcp-Method %s does not match %s' % (self.headers.get('Mcp-Method') or '(missing)', method))
            if method == 'tools/call' and decode_header(self.headers.get('Mcp-Name')) != params.get('name'):
                return err(400, mid, -32020, 'Header mismatch: Mcp-Name does not match %s' % params.get('name'))
        elif header and header not in MCP_LEGACY:
            return err(400, mid, -32022, 'Unsupported protocol version', {'supported': supported, 'requested': header})
        info = {'name': NAME, 'version': VERSION}
        if method == 'initialize':
            hello = meta.get('hello')
            if hello:
                pairing['hello'] = hello
                h = hello if isinstance(hello, dict) else {}
                c.log('%s  %s is %s (%s%s)' % (short_time(), pairing['id'], h.get('runtime') or '?', h.get('platform') or '?',
                                              (' %s' % h['version']) if h.get('version') else ''))
            asked = params.get('protocolVersion')
            result = {'protocolVersion': asked if asked in MCP_LEGACY else MCP_LEGACY[0],
                      'capabilities': {'tools': {'listChanged': True}, 'resources': {'listChanged': True, 'subscribe': False}},
                      'serverInfo': info,
                      'instructions': "This machine's capabilities (terminal, file, git, container, models, and capabilities/*.md), grouped by _meta.capability.",
                      '_meta': {'hello': {'runtime': NAME, 'platform': '%s %s python %s' % (sys.platform, platform.machine(), platform.python_version()),
                                          'version': VERSION, 'protocol': 'dry-capabilities/1'}}}
        elif method == 'server/discover':
            result = {'supportedVersions': supported, 'capabilities': {'tools': {}}, 'serverInfo': info}
        elif method == 'ping':
            result = {}
        elif method == 'tools/list':
            result = {'tools': c.cap_tools()}
        elif method == 'resources/list':
            result = {'resources': c.cap_resources()}
        elif method == 'resources/read':
            try:
                result = {'contents': [{'uri': params.get('uri'), 'mimeType': 'text/plain', 'text': c.cap_read(params.get('uri'))}]}
            except HttpError as e:
                return err(200 if e.status == 404 else 400, mid, -32002, e.message)
        elif method == 'tools/call':
            name = str(params.get('name') or '')
            t1 = time.time()
            who = '%s (%s)' % (pairing['id'], pairing['hello'].get('runtime')) if isinstance(pairing.get('hello'), dict) and pairing['hello'].get('runtime') else pairing['id']
            c.log('%s  call %s from %s …' % (short_time(), name, who))
            try:
                result = c.mcp_call(name, params.get('arguments'))
            except HttpError as e:
                c.log('%s  call %s: %s' % (short_time(), name, e.message))
                return err(200 if e.status == 404 else 400, mid, -32602, e.message)
            c.log('%s  call %s %s %dms' % (short_time(), name, 'failed' if result.get('isError') else 'done', int((time.time() - t1) * 1000)))
        else:
            if ver is not _MISSING:
                return err(404, mid, -32601, 'Method not found')
            return reply(200, {'jsonrpc': '2.0', 'id': mid, 'error': {'code': -32601, 'message': 'Method not found'}})
        reply(200, {'jsonrpc': '2.0', 'id': mid, 'result': result})

    def mcp_stream(self, c, pairing):
        s = Stream(pairing)
        with c.lock:
            c.mcp_streams.add(s)
            pairing['streams'] = pairing.get('streams', 0) + 1
        h = pairing.get('hello')
        c.log('%s  peer %s%s listening' % (short_time(), pairing['id'], (' (%s)' % h.get('runtime')) if isinstance(h, dict) else ''))

        def end():
            with c.lock:
                c.mcp_streams.discard(s)
                pairing['streams'] = pairing.get('streams', 1) - 1
            c.log('%s  peer %s stopped listening' % (short_time(), pairing['id']))

        self.serve_stream(s, ': dry-companion capabilities stream\n\n', end)

    def serve_app(self, c, p):
        if not c.app_dir or self.command not in ('GET', 'HEAD'):
            return False
        rel = urllib.parse.unquote(p)
        if rel == '/app':
            self.answer(301, {'Location': '/app/'})
            return True
        full = os.path.join(c.app_dir, *[x for x in rel.split('/') if x])
        try:
            if os.path.isdir(full):
                full = os.path.join(full, 'index.html')
            real = os.path.realpath(full)
            if not within(real, c.app_dir) or not os.path.isfile(real):
                return False
            with open(real, 'rb') as fh:
                data = fh.read()
        except OSError:
            return False
        self.answer(200, {'Content-Type': MIME.get(os.path.splitext(real)[1].lower(), 'application/octet-stream'), 'Cache-Control': 'no-cache',
                          'X-Content-Type-Options': 'nosniff', 'Cross-Origin-Opener-Policy': 'same-origin',
                          'Cross-Origin-Embedder-Policy': 'require-corp', 'Cross-Origin-Resource-Policy': 'same-origin'}, data)
        return True


_MISSING = object()


def decode_header(v):
    m = re.match(r'^=\?base64\?(.*)\?=$', v or '')
    if not m:
        return v
    import base64
    try:
        return base64.b64decode(m.group(1)).decode('utf-8')
    except ValueError:
        return v


# ---- the command line ----

HELP = """dry companion %s (Python): one folder of this machine (its files and a terminal), its models and tools, for the dry web app.

  python3 dry_companion.py [options]

  --folder DIR     the folder the page gets (default: the current one); dry keeps its state in DIR/.dry/
  --port N         listen on 127.0.0.1:N (default %d; 0: any free port)
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
  --dry PATH       the dry binary that runs a project's evidence
  --ttl HOURS      how long a pairing lasts (default %d)
  --lmstudio URL, --ollama URL   the same as declaring a port provider named lmstudio / ollama
  --store DIR      where the page's state goes (default: DIR/.dry/state)
  --verbose        a line for every read too; --quiet: no lines

While it runs: Esc stops everything it runs. Type pairings · revoke <id> · on <id> · off <id> · switches · providers · code · capabilities · stop-all

Then in dry: Settings → Companion, type the code printed below, Connect.""" % (VERSION, DEFAULT_PORT, DEFAULT_TTL_HOURS)


def parse_args(argv):
    o = {'port': DEFAULT_PORT, 'origins': [], 'on': [], 'off': [], 'yes': False, 'quiet': False, 'verbose': False, 'heartbeat': HEARTBEAT_S,
         'help': False}
    i = 0

    def nxt():
        nonlocal i
        i += 1
        if i >= len(argv):
            raise ValueError('%s needs a value' % argv[i - 1])
        return argv[i]

    while i < len(argv):
        a = argv[i]
        if a == '--port':
            v = nxt()
            if not re.match(r'^\d+$', v) or int(v) > 65535:
                raise ValueError('--port takes a port number')
            o['port'] = int(v)
        elif a == '--folder':
            o['folder'] = nxt()
        elif a in ('--yes', '-y'):
            o['yes'] = True
        elif a == '--on':
            o['on'] += [x for x in nxt().split(',') if x]
        elif a == '--off':
            o['off'] += [x for x in nxt().split(',') if x]
        elif a == '--pair-code-file':
            o['pair_code_file'] = nxt()
        elif a == '--providers':
            o['providers'] = nxt()
        elif a == '--config':
            o['config'] = nxt()
        elif a == '--heartbeat':
            try:
                o['heartbeat'] = max(0.1, float(nxt()))
            except ValueError:
                raise ValueError('--heartbeat takes seconds')
        elif a == '--verbose':
            o['verbose'] = True
        elif a == '--quiet':
            o['quiet'] = True
        elif a == '--origin':
            o['origins'].append(nxt())
        elif a == '--lmstudio':
            o['lmstudio'] = nxt()
        elif a == '--ollama':
            o['ollama'] = nxt()
        elif a == '--store':
            o['store'] = nxt()
        elif a == '--app':
            o['app'] = nxt()
        elif a == '--no-app':
            o['app'] = False
        elif a == '--dry':
            o['dry'] = nxt()
        elif a == '--ttl':
            try:
                o['ttl'] = float(nxt())
            except ValueError:
                raise ValueError('--ttl takes hours')
        elif a == '--capabilities':
            o['capabilities'] = nxt()
        elif a == '--no-capabilities':
            o['capabilities'] = False
        elif a in ('--root', '--tools', '--allow-url', '--chrome'):
            raise ValueError('%s is the Node companion\'s (projects mode, custom tools, the browser): not in the Python one' % a)
        elif a in ('--help', '-h'):
            o['help'] = True
        else:
            raise ValueError('unknown option %s (see --help)' % a)
        i += 1
    return o


def mark(ok, state):
    return (state if state and state != 'ok' else 'ok' if ok else 'missing').ljust(8)


class Keys:
    """The companion's terminal: lines typed (commands), and Esc, which stops everything it runs.
    termios on macOS and Linux, msvcrt on Windows: plain stdin and stdout, no screen drawing."""

    def __init__(self, c, stop):
        self.c, self.stop = c, stop
        self.saved = None

    def out(self, text):
        if text:
            with self.c.print_lock:
                sys.stdout.write(text + '\n')
                sys.stdout.flush()

    def line(self, text):
        try:
            self.out(self.c.command(text))
        except Exception as e:  # noqa: BLE001
            self.out('  %s' % e)

    def run(self):
        try:
            if WIN:
                self.run_windows()
            else:
                self.run_posix()
        finally:
            self.restore()

    def restore(self):
        if self.saved is not None:
            import termios
            try:
                termios.tcsetattr(sys.stdin.fileno(), termios.TCSADRAIN, self.saved)
            except Exception:  # noqa: BLE001
                pass
            self.saved = None

    def echo(self, s):
        sys.stdout.write(s)
        sys.stdout.flush()

    def run_posix(self):
        import select
        import termios
        fd = sys.stdin.fileno()
        self.saved = termios.tcgetattr(fd)
        mode = termios.tcgetattr(fd)
        mode[3] &= ~(termios.ICANON | termios.ECHO)
        mode[6][termios.VMIN] = 1
        mode[6][termios.VTIME] = 0
        termios.tcsetattr(fd, termios.TCSADRAIN, mode)
        dec = codecs.getincrementaldecoder('utf-8')('replace')
        buf = ''
        while not self.stop.is_set():
            r, _, _ = select.select([fd], [], [], 0.25)
            if not r:
                continue
            b = os.read(fd, 1)
            if not b:
                self.stop.set()  # the terminal closed
                return
            if b == b'\x1b':
                more, _, _ = select.select([fd], [], [], 0.03)
                if more:
                    os.read(fd, 32)  # an arrow or another key's sequence: not Esc alone
                    continue
                self.echo('\n')
                self.line('stop-all')
                buf = ''
            elif b in (b'\r', b'\n'):
                self.echo('\n')
                text, buf = buf, ''
                self.line(text)
            elif b in (b'\x7f', b'\x08'):
                if buf:
                    buf = buf[:-1]
                    self.echo('\b \b')
            elif b == b'\x04':
                if not buf:
                    self.stop.set()
                    return
            elif b == b'\x15':
                self.echo('\b \b' * len(buf))
                buf = ''
            elif b >= b' ' or b == b'\t':
                ch = dec.decode(b)
                if ch:
                    buf += ch
                    self.echo(ch)

    def run_windows(self):
        import msvcrt
        buf = ''
        while not self.stop.is_set():
            if not msvcrt.kbhit():
                time.sleep(0.05)
                continue
            ch = msvcrt.getwch()
            if ch in ('\x00', '\xe0'):
                msvcrt.getwch()  # a function or arrow key
            elif ch == '\x1b':
                self.echo('\n')
                self.line('stop-all')
                buf = ''
            elif ch in ('\r', '\n'):
                self.echo('\n')
                text, buf = buf, ''
                self.line(text)
            elif ch == '\x08':
                if buf:
                    buf = buf[:-1]
                    self.echo('\b \b')
            elif ch in ('\x03', '\x1a'):
                self.stop.set()
                return
            elif ch >= ' ':
                buf += ch
                self.echo(ch)


def main(argv=None):
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, ValueError):
        pass
    if sys.version_info < (3, 9):
        sys.stderr.write('dry companion needs Python 3.9 or newer (this is %s)\n' % platform.python_version())
        return 2
    try:
        o = parse_args(sys.argv[1:] if argv is None else argv)
    except ValueError as e:
        sys.stderr.write('%s\n' % e)
        return 2
    if o['help']:
        print(HELP)
        return 0
    tty = sys.stdin.isatty()
    interactive = tty and not o['yes']
    folder = os.path.abspath(home(o.get('folder') or os.getcwd()))
    print('dry companion %s (python %s)' % (VERSION, platform.python_version()))
    if interactive:
        try:
            a = input('the page gets this folder: %s\n  its files, and a terminal in it (state in %s/). Use it? [Y/n] ' % (folder, DRY_DIR)).strip()
        except EOFError:
            a = 'n'
        if a.lower().startswith('n'):
            print('give the folder with --folder DIR')
            return 0
    else:
        print('folder: %s' % folder)
    o['folder'] = folder
    try:
        c = Companion(o)
    except ValueError as e:
        sys.stderr.write('%s\n' % e)
        return 2
    if o['on'] or o['off']:
        c.save_config()
    if not os.path.exists(c.providers_file):
        v = c.providers_template()
        try:
            c.write_providers(dict(v, providers=v.get('providers') or []))
            print('providers: made %s from the template: paste entries into "providers"' % c.providers_file)
        except OSError as e:
            print('providers: could not make %s: %s' % (c.providers_file, e))
    print('\nchecks')
    for k in c.checks():
        name = '' if k['name'] == k['what'] else '%s: ' % k['name']
        hint = ('\n             → %s' % k['hint']) if not k['ok'] and k.get('hint') else ''
        print('  %s %s %s%s%s' % (mark(k['ok'], k.get('state')), k['what'].ljust(10), name, k['line'], hint))
    while True:
        lst = c.switch_list()
        print('\nswitches (saved in %s; actuators start off)' % c.config_file)
        for n, x in enumerate(lst):
            print('  %2d  %s  %s %s%s%s' % (n + 1, 'on ' if x['on'] else 'off', x['id'].ljust(16), '(actuator) ' if x['actuator'] else '',
                                            ('[%s] ' % x['state']) if x['state'] != 'present' else '', x['doc'][:70]))
        if not interactive:
            break
        try:
            a = input('flip some (their numbers, e.g. "2 5"), or Enter to go on: ').strip()
        except EOFError:
            a = ''
        if not a:
            c.save_config()
            break
        for w in re.split(r'[\s,]+', a):
            if w.isdigit() and 0 < int(w) <= len(lst):
                x = lst[int(w) - 1]
                c.set_switch(x['id'], not x['on'])
    try:
        c.listen()
    except OSError as e:
        sys.stderr.write('could not listen on 127.0.0.1:%d: %s\n' % (o['port'], e))
        return 1
    print('')
    c.announce()
    stop = threading.Event()

    def on_signal(*_):
        stop.set()

    for name in ('SIGINT', 'SIGTERM', 'SIGHUP', 'SIGBREAK'):
        if hasattr(signal, name):
            try:
                signal.signal(getattr(signal, name), on_signal)
            except (OSError, ValueError):
                pass
    keys = None
    if tty:
        keys = Keys(c, stop)
        threading.Thread(target=keys.run, daemon=True).start()
    try:
        while not stop.wait(0.25):
            pass
    except KeyboardInterrupt:
        pass
    if keys:
        keys.restore()
    print('\nstopping', flush=True)
    c.close()
    time.sleep(0.05)
    return 0


if __name__ == '__main__':
    sys.exit(main())
