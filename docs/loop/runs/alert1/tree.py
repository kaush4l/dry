#!/usr/bin/env python3
'''Print a center-aligned tree of height n.

Usage: python3 tree.py <height>
Each level is 2 stars wider than the level above (level 1: 1, level 2: 3, ...).
Every line is padded so all lines are the same width: the width of the widest line.
'''
import sys

USAGE = 'Usage: python3 tree.py <height>'


def main():
    if len(sys.argv) != 2:
        print(USAGE)
        sys.exit(2)
    try:
        n = int(sys.argv[1])
    except ValueError:
        print(USAGE)
        sys.exit(2)
    if n < 1:
        print(USAGE)
        sys.exit(2)

    width = 2 * n - 1
    for level in range(1, n + 1):
        stars = '*' * (2 * level - 1)
        print(stars.center(width))


if __name__ == '__main__':
    main()
