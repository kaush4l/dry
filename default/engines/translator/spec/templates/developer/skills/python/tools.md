# python
Check, run and test Python files in the project's root.

## tool: check
Compile one Python file without running it: exit 0 and no output when its syntax is fine, else the error with its line.
command: ["python3", "-m", "py_compile", "{file}"]
inputs:
- file (string, required): the file, relative to the root

## tool: run
Run one Python file, with up to three arguments.
command: ["python3", "{file}", "{arg}", "{arg2}", "{arg3}"]
inputs:
- file (string, required): the file, relative to the root
- arg (string): its first argument
- arg2 (string): its second argument
- arg3 (string): its third argument

## tool: test
Run the folder's unittest tests (test_*.py), each named with its result.
command: ["python3", "-m", "unittest", "discover", "-v"]
