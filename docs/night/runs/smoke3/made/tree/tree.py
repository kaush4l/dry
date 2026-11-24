"""Print a centred star tree of height n (default 5)."""
import sys

USAGE = "usage: python3 tree.py [n]  (n integer >= 1)"


def tree_lines(n):
    return [" " * (n - k) + "*" * (2 * k - 1) for k in range(1, n + 1)]


def main(argv):
    if len(argv) > 1:
        print(USAGE, file=sys.stderr)
        return 1
    if argv:
        try:
            n = int(argv[0])
        except ValueError:
            print(USAGE, file=sys.stderr)
            return 1
    else:
        n = 5
    if n < 1:
        print(USAGE, file=sys.stderr)
        return 1
    for line in tree_lines(n):
        print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
