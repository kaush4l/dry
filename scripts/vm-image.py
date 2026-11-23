#!/usr/bin/env python3
"""Build the browser VM's Alpine image (docs/container.md).

Reads two official Alpine files that scripts/vm-fetch.py put in
vendor-dl/alpine/ and writes vendor-dl/out/:

  vmlinuz-virt          the kernel, as linux-virt ships it
  alpine-dry.cpio.gz    an initramfs: the minirootfs, the few kernel
                        modules the VM needs (virtio net, 9p), /init

Nothing from the downloads is executed: tar members are read as data and
written into a cpio (newc) archive. Run with `python3 -I`.
"""

import glob
import gzip
import io
import os
import stat
import sys
import tarfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "vendor-dl", "alpine")
OUT = os.path.join(ROOT, "vendor-dl", "out")
MINIROOT = "alpine-minirootfs-3.24.4-x86.tar.gz"
# Loaded in this order by /init (insmod: no modprobe or depmod needed).
MODULES = [
    "net/core/failover.ko.gz",
    "drivers/net/net_failover.ko.gz",
    "drivers/net/virtio_net.ko.gz",
    "fs/netfs/netfs.ko.gz",
    "net/9p/9pnet.ko.gz",
    "net/9p/9pnet_virtio.ko.gz",
    "fs/9p/9p.ko.gz",
]

# The VM's first process. The page talks to this shell on the serial port:
# no echo, no prompt, so what comes back is only what commands print
# (dry_core::container: the exec markers).
INIT = r"""#!/bin/sh
export PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME=/root TERM=dumb
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev 2>/dev/null
mkdir -p /dev/pts /dev/shm /tmp /run /mnt/host
mount -t devpts devpts /dev/pts
mount -t tmpfs tmpfs /tmp
mount -t tmpfs tmpfs /run
hostname dry
for m in failover net_failover virtio_net netfs 9pnet 9pnet_virtio 9p; do
  insmod /lib/modules/dry/$m.ko 2>/dev/null
done
ip link set lo up
# v86's fetch network: the guest is 192.168.86.100, the router and its DNS
# 192.168.86.1 (static; udhcpc needs AF_PACKET, a module not loaded here).
if ip link set eth0 up 2>/dev/null; then
  ip addr add 192.168.86.100/24 dev eth0
  ip route add default via 192.168.86.1
  echo "nameserver 192.168.86.1" > /etc/resolv.conf
else
  echo "dry: no network card"
fi
# Alpine's tcc looks for crt1.o in a Debian-style folder.
mkdir -p /usr/lib && ln -s /usr/lib /usr/lib/i386-linux-gnu 2>/dev/null
mount -t 9p -o trans=virtio,version=9p2000.L host9p /mnt/host 2>/dev/null || echo "dry: no 9p mount"
cd /root
echo "dry: ready $(uname -r) $(cat /etc/alpine-release)"
exec setsid -c /bin/sh -c 'stty -echo; PS1= PS2= exec /bin/sh' </dev/ttyS0 >/dev/ttyS0 2>&1
"""


class Cpio:
    def __init__(self):
        self.buf = io.BytesIO()
        self.ino = 1
        self.names = set()

    def add(self, name, mode, data=b"", rdev=(0, 0), mtime=0):
        while name.startswith("./") or name.startswith("/"):
            name = name[1:] if name.startswith("/") else name[2:]
        name = name.rstrip("/") or "."
        if name in self.names:
            return
        self.names.add(name)
        nb = name.encode() + b"\0"
        fields = [self.ino, mode, 0, 0, 1, mtime, len(data), 0, 0, rdev[0], rdev[1], len(nb), 0]
        self.ino += 1
        self.buf.write(b"070701" + b"".join(b"%08X" % f for f in fields))
        self.buf.write(nb)
        self._pad()
        self.buf.write(data)
        self._pad()

    def _pad(self):
        n = self.buf.tell() % 4
        if n:
            self.buf.write(b"\0" * (4 - n))

    def finish(self):
        self.names.discard("TRAILER!!!")
        self.add("TRAILER!!!", 0)
        return self.buf.getvalue()


def add_tar(cpio, tf):
    members = tf.getmembers()
    by_name = {m.name: m for m in members}
    for m in members:
        perm = m.mode & 0o7777
        if m.isdir():
            cpio.add(m.name, stat.S_IFDIR | perm, mtime=int(m.mtime))
        elif m.issym():
            cpio.add(m.name, stat.S_IFLNK | 0o777, m.linkname.encode(), mtime=int(m.mtime))
        elif m.islnk():
            target = by_name.get(m.linkname)
            data = tf.extractfile(target).read() if target else b""
            cpio.add(m.name, stat.S_IFREG | perm, data, mtime=int(m.mtime))
        elif m.isreg():
            cpio.add(m.name, stat.S_IFREG | perm, tf.extractfile(m).read(), mtime=int(m.mtime))


def main():
    os.makedirs(OUT, exist_ok=True)
    cpio = Cpio()
    # Device nodes the kernel needs before devtmpfs is mounted.
    for d in ["dev", "proc", "sys", "tmp", "run", "root", "mnt", "lib", "lib/modules", "lib/modules/dry"]:
        cpio.add(d, stat.S_IFDIR | 0o755)
    cpio.add("dev/console", stat.S_IFCHR | 0o600, rdev=(5, 1))
    cpio.add("dev/ttyS0", stat.S_IFCHR | 0o660, rdev=(4, 64))
    cpio.add("dev/null", stat.S_IFCHR | 0o666, rdev=(1, 3))
    cpio.add("init", stat.S_IFREG | 0o755, INIT.encode())

    apks = sorted(glob.glob(os.path.join(SRC, "linux-virt-*.apk")))
    if not apks:
        sys.exit("no linux-virt package in vendor-dl/alpine: run scripts/vm-fetch.py first")
    # An .apk is gzip members of tar segments, one after another.
    with tarfile.open(apks[-1], "r:gz", ignore_zeros=True) as tf:
        kver = next(n.split("/")[2] for n in tf.getnames() if n.startswith("lib/modules/") and n.count("/") >= 3)
        for name in MODULES:
            m = tf.getmember(f"lib/modules/{kver}/kernel/{name}")
            base = os.path.basename(name)[: -len(".ko.gz")]
            cpio.add(f"lib/modules/dry/{base}.ko", stat.S_IFREG | 0o644, gzip.decompress(tf.extractfile(m).read()))
        kernel = tf.extractfile(tf.getmember("boot/vmlinuz-virt")).read()

    with tarfile.open(os.path.join(SRC, MINIROOT), "r:gz") as tf:
        add_tar(cpio, tf)

    raw = cpio.finish()
    with open(os.path.join(OUT, "vmlinuz-virt"), "wb") as f:
        f.write(kernel)
    with open(os.path.join(OUT, "alpine-dry.cpio.gz"), "wb") as f:
        f.write(gzip.compress(raw, 9, mtime=0))
    print(f"kernel {len(kernel)} B, initramfs {len(raw)} B raw -> {os.path.getsize(os.path.join(OUT, 'alpine-dry.cpio.gz'))} B gz", file=sys.stderr)


if __name__ == "__main__":
    main()
