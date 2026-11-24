#!/usr/bin/env python3
import sys


def main():
    if len(sys.argv) > 1:
        try:
            n = int(sys.argv[1])
        except ValueError:
            print('usage: tree.py [height]', file=sys.stderr)
            sys.exit(2)
        if n < 1:
            print('height must be at least 1', file=sys.stderr)
            sys.exit(2)
    else:
        n = 5

    width = 2 * n - 1
    for k in range(1, n + 1):
        line = '*' * (2 * k - 1)
        print(line.center(width))


if __name__ == '__main__':
    main()
