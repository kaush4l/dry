---
name: python
description: Write, check, run and test a Python file with tools made for it - load it when the work is Python.
---
# Python: check it, run it, read it

This skill brings three tools. Each runs in your terminal, in the root,
and comes back with the exit code and the last lines, as `run` does.

- `python.check(file="tree.py")` compiles the file without running it.
  No output and exit 0 means the syntax is fine. Call it after every save.
- `python.run(file="tree.py", arg="5")` runs the file with one argument
  (`arg2`, `arg3` for more; leave them out for none). Use it instead of
  writing a command line.
- `python.test()` runs the tests the folder holds (`test_*.py`, with the
  standard library's unittest). Use it only when there are any.

How to work:

1. **Write the whole file, save, check, run, in one act.**
   `edit(path="tree.py", new="..."); save(path="tree.py"); python.check(file="tree.py"); python.run(file="tree.py", arg="3")`
2. **Inside `new="..."`** a double quote is written `\"` and a backslash is
   written `\\` (so Python's `"\n"` is `\"\\n\"`). Single-quoted Python
   strings need no escape: prefer `'*'` to `"*"`.
3. **Read every line that came back.** Count the characters of a line
   when the ask is about width or alignment. A traceback names the line:
   fix that line with `edit(path, old="the exact line", new="...")`, save,
   and run again.
4. **Input from the command line** is `sys.argv[1]`; say what happens with
   no argument (a usage line and a non-zero exit, or a default).
5. **Only the standard library** unless the project already uses more:
   nothing can be installed with pip here.
6. **`python3: not found`**: your environment line gives the command that
   installs it; run that once, then go on.
