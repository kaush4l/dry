#!/usr/bin/env node
// A stand-in for a model CLI (copilot) in the conformance suite: `--version`
// answers; `-p <prompt> ...` prints one line back.
const argv = process.argv.slice(2);
if (argv.includes('--version')) { console.log('fake-cli 1.0.0'); process.exit(0); }
const i = argv.indexOf('-p');
console.log(`cli says hi to: ${i >= 0 ? argv[i + 1] : '(no prompt)'}`);
