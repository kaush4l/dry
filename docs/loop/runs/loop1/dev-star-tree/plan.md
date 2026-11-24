## Ask
A stdlib-only script that prints a centered tree of height n (first CLI arg, default 5), each level having 2 more stars than the one above (level 1 = 1 star).
## Steps
- [x] star_tree.py: read n from argv (default 5), build lines '*'*(2*i-1) for i in 1..n, center each on the widest line, print
## Proof
python3 star_tree.py 5 → prints 5 lines, each centered on the widest, star counts 1,3,5,7,9; run with no arg → same as n=5