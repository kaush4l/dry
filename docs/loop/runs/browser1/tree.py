#!/usr/bin/env python3
import sys

def main():
    n = 5
    if len(sys.argv) > 1:
        try:
            n = int(sys.argv[1])
        except ValueError:
            print('Usage: tree.py [n]', file=sys.stderr)
            sys.exit(1)
    if n < 1:
        print('n must be >= 1', file=sys.stderr)
        sys.exit(1)
    stars = [2 * k - 1 for k in range(1, n + 1)]
    width = stars[-1]
    for s in stars:
        line = '*' * s
        pad = (width - s) // 2
        print(' ' * pad + line)

if __name__ == '__main__':
    main()
