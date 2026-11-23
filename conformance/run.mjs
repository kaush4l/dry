#!/usr/bin/env node
// Run the conformance suite against a companion's start command:
//
//   node conformance/run.mjs --cmd "python3 dry_companion.py"
//   node conformance/run.mjs                      (the Node companion)
//
// The same as DRY_COMPANION_CMD="<cmd>" node --test conformance/.

import { spawn } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const env = { ...process.env };
const files = [];
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === '--cmd') env.DRY_COMPANION_CMD = argv[++i];
  else if (argv[i] === '--') continue;
  else files.push(argv[i]);
}
const tests = files.length ? files : fs.readdirSync(here).filter((f) => f.endsWith('.test.mjs')).sort().map((f) => path.join(here, f));
console.log(`conformance: ${env.DRY_COMPANION_CMD || 'the Node companion (companion/dry-companion.mjs)'}`);
const child = spawn(process.execPath, ['--test', ...tests], { stdio: 'inherit', env });
child.on('exit', (code) => process.exit(code ?? 1));
