import sys


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    width = 2 * n - 1
    for level in range(1, n + 1):
        stars = 2 * level - 1
        print(('*' * stars).center(width))


if __name__ == '__main__':
    main()
