## Ask
Create a fresh standalone script tree.py that prints a center-aligned tree of '*' of height n.

## Steps
- [x] Create tree.py: parse n from argv (int), print each level k with 2k-1 stars, centered by left-padding with spaces (padding = (max_width - level_width)//2).
- [x] Run python3 tree.py 1, 4, and a bad argument to verify exit code and output.

## Proof
`python3 tree.py 4` prints 1, 3, 5, 7 stars center-aligned, exit 0. `python3 tree.py 1` prints '*', exit 0.