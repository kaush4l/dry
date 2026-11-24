#!/usr/bin/env python3
"""Print a centered tree of height n.

Level k (1-indexed) has 2k - 1 '*' stars, so level 1 has 1, level 2 has 3, etc.
Each line is centered horizontally on the width of the widest (bottom) line.
"""
import sys


def main():
    if len(sys.argv) != 2:
        print("Usage: python3 tree.py <n>", file=sys.stderr)
        return 2
    try:
        n = int(sys.argv[1])
    except ValueError:
        print(f"Error: invalid height '{sys.argv[1]}': expected a positive integer", file=sys.stderr)
        return 2
    if n < 1:
        print(f"Error: height must be a positive integer, got {n}", file=sys.stderr)
        return 2

    width = 2 * n - 1
    for k in range(1, n + 1):
        stars = "*" * (2 * k - 1)
        print(stars.center(width))
    return 0


if __name__ == "__main__":
    sys.exit(main())
