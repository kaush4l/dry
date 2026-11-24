## Ask
Create tree.py: prints a centred star tree of height n (argv[1], default 5), level k has 2k-1 stars, leading spaces to width 2n-1, no trailing spaces; n<1 or non-integer -> usage on stderr, exit 1.

## Steps
- [x] tree.py — parse argv, validate, print n lines: ' '*(n-k) + '*'*(2k-1)

## Proof
- `python3 tree.py 3 | cat -e` prints exactly:
  `  *$`, ` ***$`, `*****$`
- `python3 tree.py 1` prints `*`
- `python3 tree.py` prints 5 lines, last `*********`
- `python3 tree.py 0; echo $?`, `python3 tree.py -2; echo $?`, `python3 tree.py abc; echo $?` each print `usage: python3 tree.py [n]  (n integer >= 1)` to stderr and exit 1 (nothing on stdout)
- `python3 tree.py 3 1 2` extra args: usage, exit 1