## Ask
Create tree.py in the project root: print a centered tree of height n, level k has 2k-1 '*' stars (1,3,5,...), each line centered on the widest (bottom) line; n comes from argv[1]; invalid n reported with a clear message.
## Steps
- [x] Create tree.py: parse n (positive int), print centered lines of 2k-1 stars, level k from 1 to n, centered on the widest line (2n-1 stars).
## Proof
Run: python3 tree.py 1 / python3 tree.py 4 / python3 tree.py 6 / python3 tree.py abc
Expected: centered lines of 1,3,5,... stars per level, each line centered on the widest line; invalid n prints a clear message and a non-zero exit code.