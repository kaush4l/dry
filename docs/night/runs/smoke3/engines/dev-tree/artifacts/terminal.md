kind: terminal
root: /Volumes/Backup/dry-build/night-run/smoke3/proj/tree
session: 1 run exit:0 1795494045 ls -la
session: 2 run exit:0 1795494056 python3 tree.py 3 | cat -e; echo '---'; python3 tree.py 1 | cat -e; echo '---'; python3 tree.py | cat -e; echo '---'; for a in 0 -2 abc '3 1 2'; do python3 tree.py $a 2>/dev/null | wc -c; python3 tree.py $a 2>&1 >/dev/null; echo "exit $?"; done
session: 3 run exit:0 1795494074 python3 tree.py 3 | cat -e; echo "exit ${PIPESTATUS[0]}"; echo '---'; python3 tree.py 1 | cat -e; echo '---'; python3 tree.py | cat -e; echo '---'; python3 tree.py 7 | cat -e; echo '---'; python3 tree.py | wc -l; for a in 0 -2 abc '3 1 2'; do echo "== $a"; printf 'stdout bytes: '; python3 tree.py $a 2>/dev/null | wc -c; python3 tree.py $a 2>&1 >/dev/null; echo "exit $?"; done
session: 4 run exit:0 1795494083 python3 tree.py 3 | cat -e; echo "exit ${PIPESTATUS[0]}"; echo '---'; python3 tree.py 1 | cat -e; echo '---'; python3 tree.py | cat -e; echo "exit ${PIPESTATUS[0]}"
