## Ask
No tree script exists → `python3 tree.py N` prints a tree of N levels, level k has 2k-1 '*' (level 1: 1, level 2: 3, ...), each line centered on the widest (bottom) line (width 2N-1).

## Now
- project root — empty (ls: only . and ..); no code, no tests, no git repo

## Steps
- [x] 1. tree.py — new script: argv[1] = height n; for k in 1..n, print (2k-1) '*' centered on width 2n-1 (total line width = 2n-1; pad equally on both sides, remainder to the left). Handle bad/missing argv with a clear usage message and non-zero exit.

## Proof
- `python3 tree.py 5` — passes when output is:
    *
   ***
  *****
 *******
*********
  (each line centered, e.g. level 1 = 4 spaces + '*' + 4 spaces = 9-wide line)
- `python3 tree.py` (no arg) — prints usage and exits non-zero

## Out of scope
- tests/CI (no test convention in the project; adding one is beyond the ask)
- any change to other files (none exist)