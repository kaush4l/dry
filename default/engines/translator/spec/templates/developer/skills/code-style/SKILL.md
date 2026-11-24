---
name: code-style
description: Make the edits in the style of the code around them - naming, comments, errors, tests - save each file and compile it; no drive-by changes.
---
# Code: edits in the repository's style

1. **Read the neighbours first.** Before you write a function, open one or
   two like it in the same folder. Copy their naming, error handling,
   comment style, imports and test layout. The repository's way wins over
   your habit.
2. **Edit exactly.** `edit(path, old, new)` needs `old` to appear once.
   Take it from the file as the artifact shows it now, with its
   indentation and without the line numbers. Make one edit per change.
   `save(path)` when the file is done. An unsaved edit is not a change.
   A new file is `edit(path, new)` with no `old`: `new` is its whole text.
3. **Only what the plan names.** No renames, reformatting or "while I'm
   here" fixes. If you see a real problem elsewhere, write it in the
   notes for the report.
4. **Tests beside the code.** Put the new test where the project keeps its
   tests, written the way its tests are written.
5. **Keep it small.** Prefer changing a line to adding a layer. Prefer the
   standard library and what the project already uses to a new dependency.
6. **Never weaken a check to go green.** Do not edit a test's expected
   value to match wrong output, skip a test, or loosen a lint.
7. **Secrets stay out.** Never write a key, token or password into a file.
   Files that hold secrets are not opened.
8. **Compile after each save.** Run the quickest check the toolchain
   gives, on the saved file or its package: `cargo check -q`, `go vet
   ./...`, `bunx tsc --noEmit` when the project has a `tsconfig.json`,
   `bun build <file> --target bun > /dev/null` for TypeScript without one,
   `python3 -c 'import ast,sys; ast.parse(open(sys.argv[1]).read())' <file>`
   (it writes nothing). Read the errors. Fix the ones your edit
   caused and run it again until they are gone. Errors that were there
   before you started are not yours: note them, leave them.
9. **Three tries, then back.** The same error after three fixes means the
   plan is wrong or the cause is elsewhere. Stop editing, write the error
   and what you tried in the notes, and go back to triage.
10. **Tick the step.** When a step's file is saved and compiles, write the
    plan again with that step as `- [x]`.

When every step in the plan is ticked, `goto_phase(name="test")`.

Ideas 8–10 from VS Code's agent (errors come back after each edit; stop
after three tries on one file; the to-do list), MIT; written for dry.
