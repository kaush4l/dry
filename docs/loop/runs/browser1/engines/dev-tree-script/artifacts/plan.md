## Ask
One new stdlib-only script tree.py: print a center-aligned tree of height n; level k has 2k-1 stars (level 1 = 1 star); n from the command line, default 5 if none given.

## Steps
- [x] Install python3 (apk add python3) - not on PATH
- [x] Write tree.py: read n from sys.argv (default 5), print n levels, level k = (2k-1) stars, each line centered under the widest level
- [x] Run the script and verify output for n=1, n=3, n=5 (and default)

## Proof
`python3 tree.py 3` prints exactly three center-aligned lines:
  *
 ***
*****
`python3 tree.py 5` (or no args) prints five lines with 1, 3, 5, 7, 9 stars, each centered under the widest line.