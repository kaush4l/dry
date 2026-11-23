---
name: test
description: Build, run and exercise the change with whatever this machine offers (terminal, evidence, browser, native runtimes such as bun), then hand the result to critique.
---
# Test: build it, run it, use it

Only the declared checks count, run here, now, on the saved files.

1. **Build and test.** Run the project's own commands: `bun test`,
   `npm test`, `cargo test -q`, `cargo clippy -q --all-targets`,
   `make test`, as the project declares them. Read the exit code and the
   last lines. A long check goes to `start(command="...", wait="yes")`:
   your turn ends with no reply and goes on here when it ends; never reply early to wait.
2. **Run it.** Use the program the way its user would:
   - a CLI or library: `run` it with the input from the plan, read the output;
   - a web app: `background` its dev server, wait until it is up
     (`run(command="curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:PORT/")`),
     then browse it if you have a browser artifact (load browser-qa);
   - a project with an `evidence.md`: `evidence(op="run", expect="<the
     scenarios the plan said would change>")`. An expected change passes,
     anything else fails with its shot and text lines.
   - Bun's runtime runs TypeScript directly: `bun run file.ts`, `bun test`,
     `bun -e 'console.log(...)'` for a quick check of one function.
3. **Stop what you started.** A server you started with `background` is
   still running when you finish. Stop it (`run(command="kill <pid>")`, or
   the project's stop command) before you reply.
4. **Say what you could not run.** No browser artifact, no evidence.md, no
   network: that goes in the report as Unverified, with why.
5. **Never work around the environment.** A missing tool, a port in use or
   a timeout is reported as what it is. Do not change code to dodge it.

Then load critique.

Ideas from gstack's `build-check` (declared checks only, typed verdicts,
never weaken a test) and `qa`, both MIT; written for dry.
