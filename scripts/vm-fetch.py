#!/usr/bin/env python3
"""Fetch what the browser VM needs from Alpine's own mirror
(dl-cdn.alpinelinux.org) into vendor-dl/ (git-ignored; docs/container.md):

  vendor-dl/alpine/       the x86 minirootfs (sha256 checked against
                          Alpine's .sha256) and the linux-virt package
  vendor-dl/apk-mirror/   alpine/v3.24/{main,community}/x86/: the signed
                          APKINDEX files as published and the packages
                          PACKAGES need, so `apk add` in the VM can fetch
                          them from the page's own origin

Then scripts/vm-image.py builds the kernel + initramfs. Nothing downloaded
is run here: files are saved and hashed. apk in the VM checks the index
signature and each package's hash itself. Run with `python3 -I`.
"""

import hashlib
import io
import os
import sys
import tarfile
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "vendor-dl")
CDN = "https://dl-cdn.alpinelinux.org/alpine"
BRANCH = "v3.24"
RELEASE = "3.24.4"
ARCH = "x86"
MINIROOT = f"alpine-minirootfs-{RELEASE}-{ARCH}.tar.gz"
# What the container route's checks install (python3; tcc + musl-dev for a
# small C build). gcc is ~70 MB with its dependencies: left out.
PACKAGES = ["python3", "python3-pycache-pyc0", "tcc", "tcc-libs-static", "tcc-dev", "musl-dev"]


def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "dry-vm-fetch"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def save(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def index(raw, repo):
    text = tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz").extractfile("APKINDEX").read().decode()
    pkgs = {}
    for block in text.split("\n\n"):
        d = {}
        for line in block.splitlines():
            if len(line) > 2 and line[1] == ":":
                d[line[0]] = line[2:]
        if "P" in d:
            d["repo"] = repo
            pkgs[d["P"]] = d
    return pkgs


def closure(pkgs, want, have):
    provides = {}
    for name, d in pkgs.items():
        provides.setdefault(name, name)
        for p in d.get("p", "").split():
            provides.setdefault(p.split("=")[0], name)
    seen, stack = set(), list(want)
    while stack:
        dep = stack.pop()
        if dep.startswith("!"):
            continue
        for op in "<>=~":
            dep = dep.split(op)[0]
        name = provides.get(dep)
        if not name or name in seen or name in have:
            continue
        seen.add(name)
        stack += pkgs[name].get("D", "").split()
    return seen


def main():
    rel = f"{CDN}/{BRANCH}/releases/{ARCH}"
    mini = get(f"{rel}/{MINIROOT}")
    want_sha = get(f"{rel}/{MINIROOT}.sha256").decode().split()[0]
    if hashlib.sha256(mini).hexdigest() != want_sha:
        sys.exit(f"{MINIROOT}: sha256 does not match Alpine's")
    save(os.path.join(OUT, "alpine", MINIROOT), mini)
    installed = set()
    db = tarfile.open(fileobj=io.BytesIO(mini), mode="r:gz").extractfile("./lib/apk/db/installed").read().decode()
    for line in db.splitlines():
        if line.startswith("P:"):
            installed.add(line[2:])

    pkgs = {}
    for repo in ["community", "main"]:
        raw = get(f"{CDN}/{BRANCH}/{repo}/{ARCH}/APKINDEX.tar.gz")
        save(os.path.join(OUT, "apk-mirror", "alpine", BRANCH, repo, ARCH, "APKINDEX.tar.gz"), raw)
        pkgs.update(index(raw, repo))  # main last: it wins a name both have

    kernel = pkgs["linux-virt"]
    name = f"linux-virt-{kernel['V']}.apk"
    data = get(f"{CDN}/{BRANCH}/main/{ARCH}/{name}")
    save(os.path.join(OUT, "alpine", name), data)
    print(f"{name} {len(data)} B", file=sys.stderr)

    total = 0
    for p in sorted(closure(pkgs, PACKAGES, installed)):
        d = pkgs[p]
        file = f"{p}-{d['V']}.apk"
        data = get(f"{CDN}/{BRANCH}/{d['repo']}/{ARCH}/{file}")
        save(os.path.join(OUT, "apk-mirror", "alpine", BRANCH, d["repo"], ARCH, file), data)
        total += len(data)
        print(f"  {d['repo']}/{file} {len(data)} B", file=sys.stderr)
    print(f"mirror: {total} B", file=sys.stderr)


if __name__ == "__main__":
    main()
