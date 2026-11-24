import sys

def main():
    n = 5
    if len(sys.argv) > 1:
        try:
            n = int(sys.argv[1])
        except ValueError:
            print('Usage: python3 star_tree.py [n]', file=sys.stderr)
            sys.exit(1)
    if n < 1:
        print('Usage: python3 star_tree.py [n]  (n >= 1)', file=sys.stderr)
        sys.exit(1)
    lines = ['*' * (2 * i - 1) for i in range(1, n + 1)]
    width = max(len(line) for line in lines)
    for line in lines:
        print(line.center(width))

if __name__ == '__main__':
    main()
