#!/usr/bin/env python3
"""Print a center-aligned tree of '*' of height n.

Usage: python3 tree.py <n>
Level k (1-indexed) holds 2k-1 stars; each line is centered.
"""

import sys


def main():
    if len(sys.argv) != 2:
        print("Usage: python3 tree.py <n>", file=sys.stderr)
        return 2
    try:
        n = int(sys.argv[1])
    except ValueError:
        print("Error: argument must be an integer", file=sys.stderr)
        return 2
    if n < 1:
        print("Error: height must be at least 1", file=sys.stderr)
        return 2

    max_width = 2 * n - 1
    for k in range(1, n + 1):
        width = 2 * k - 1
        padding = (max_width - width) // 2
        print(" " * padding + "*" * width)
    return 0


if __name__ == "__main__":
    sys.exit(main())
