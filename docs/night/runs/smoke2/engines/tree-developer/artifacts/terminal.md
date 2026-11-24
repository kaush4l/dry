kind: terminal
root: /Volumes/Backup/dry-build/night-run/smoke2/proj/tree
session: 1 run exit:0 1795494014 mkdir -p /Volumes/Backup/dry-build/night-run/smoke2/proj/tree && cd /Volumes/Backup/dry-build/night-run/smoke2/proj/tree && ls -la
session: 2 run exit:2 1795494014 cd /Volumes/Backup/dry-build/night-run/smoke2/proj/tree && python3 tree.py 3 && echo '---' && python3 tree.py 5
session: 3 run exit:0 1795494018 cd /Volumes/Backup/dry-build/night-run/smoke2/proj/tree && python3 tree.py 3 && echo '---' && python3 tree.py 5
session: 4 run exit:0 1795494033 ls -la /Volumes/Backup/dry-build/night-run/smoke2/proj/tree/tree.py && cd /Volumes/Backup/dry-build/night-run/smoke2/proj/tree && python3 tree.py 3 && echo '---' && python3 tree.py 5
session: 5 run exit:0 1795494068 cd /Volumes/Backup/dry-build/night-run/smoke2/proj/tree && echo '$ python3 tree.py 3' && python3 tree.py 3 && echo '$ python3 tree.py 5' && python3 tree.py 5
