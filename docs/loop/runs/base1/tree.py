#!/usr/bin/env python3
"""Print a centered tree of height n.

Level k (1-indexed) has 2k-1 '*', level 1 is 1 star.
Each line is centered on the widest (bottom) line.

Usage: python3 tree.py N
"""

import sys


def main():
    if len(sys.argv) != 2:
        print("Usage: python3 tree.py N", file=sys.stderr)
        return 2
    try:
        n = int(sys.argv[1])
    except ValueError:
        print(f"Error: invalid height {sys.argv[1]!r}: not a number", file=sys.stderr)
        return 2
    if n < 1:
        print("Error: height must be a positive integer", file=sys.stderr)
        return 2

    width = 2 * n - 1
    for k in range(1, n + 1):
        stars = "*" * (2 * k - 1)
        pad = width - len(stars)
        left = (pad + 1) // 2
        print(" " * left + stars + " " * (pad - left))
    return 0


if __name__ == "__main__":
    sys.exit(main())
