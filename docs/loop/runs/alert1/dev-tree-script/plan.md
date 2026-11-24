## Ask
One new Python 3 stdlib-only script `tree.py` in the folder that prints a center-aligned tree of height n (n from argv), each level 2 stars wider than the one above (level 1: 1, level 2: 3, level 3: 5, level 4: 7), all lines padded to the widest line's width.

## Steps
- [x] Create tree.py in /Volumes/Backup/dry-build/loop-run/alert1/proj

## Proof
`python3 tree.py 4` prints exactly 4 lines of widths 1,3,5,7, each centered on width 7, the longest line with no leading/trailing spaces:
```
   *   
  ***  
 ***** 
*******
```
(`python3 tree.py` with no arg or a non-integer arg prints a one-line usage message and exits non-zero.)