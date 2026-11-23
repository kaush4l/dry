#!/usr/bin/env node
// A stand-in for mlx_lm.server / llama-server in the conformance suite: it
// serves an OpenAI-compatible API on the --port it is given, for the model
// it is given (-m or --model). It downloads nothing and loads nothing.
import http from 'node:http';
const argv = process.argv.slice(2);
const val = (...names) => { for (const n of names) { const i = argv.indexOf(n); if (i >= 0) return argv[i + 1]; } return null; };
const port = Number(val('--port'));
const model = val('-m', '--model');
if (!port || !model) { console.error('fake-runner: needs --port and -m/--model'); process.exit(2); }
http.createServer((req, res) => {
  let body = '';
  req.on('data', (d) => { body += d; });
  req.on('end', () => {
    res.writeHead(req.url === '/v1/models' || req.url === '/v1/chat/completions' ? 200 : 404, { 'content-type': 'application/json' });
    if (req.url === '/v1/models') res.end(JSON.stringify({ object: 'list', data: [{ id: 'runner-model', object: 'model', meta: { file: model } }] }));
    else if (req.url === '/v1/chat/completions') res.end(JSON.stringify({ id: 'r', object: 'chat.completion', choices: [{ index: 0, message: { role: 'assistant', content: `runner says hi (${JSON.parse(body || '{}').model})` }, finish_reason: 'stop' }] }));
    else res.end('{}');
  });
}).listen(port, '127.0.0.1');
