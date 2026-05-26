#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Verify that FUSE_NOTIFY_INC_EPOCH invalidates the readdir page cache.

Requires a patched kernel with INC_EPOCH support (proto_minor >= 44).

Run test:
    make

The test mounts a minimal FUSE filesystem with:
  - FUSE_CAP_NO_OPENDIR_SUPPORT, FUSE_CAP_READDIRPLUS
  - no AUTO_INVAL_DATA
  - infinite TTL on all caches (entry, attr, readdir)

It verifies that after INC_EPOCH, subsequent listdir() calls see the
updated directory contents, even though no INVAL_INODE or INVAL_ENTRY was
sent. It checks both:
  - listdir(".") with cwd inside the changed directory
  - listdir("dir") with cwd at the FUSE root, so fuse_root/dir is cached

Expected output (patched kernel):
    cwd=. baseline (no invalidation)       STALE   ['a.txt']
    cwd=. inc_epoch                        FRESH   ['a.txt', 'b.txt']
    cwd=root baseline (no invalidation)    STALE   ['a.txt']
    cwd=root inc_epoch                     FRESH   ['a.txt', 'b.txt']

On stock kernel (no INC_EPOCH fix), both show STALE.
"""

# @noautodeps

import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

DIR = Path(__file__).parent
TESTFS = DIR / "testfs"
VERBOSE = "-v" in sys.argv

G, R, Y, B, N = "\033[32m", "\033[31m", "\033[33m", "\033[1m", "\033[0m"


class FuseDaemon:
    def __init__(self, mnt):
        self.mnt = mnt
        self.proc = subprocess.Popen(
            [str(TESTFS), "-f", str(mnt)],
            stdin=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._q = queue.Queue()
        threading.Thread(target=self._reader, daemon=True).start()
        # Wait for mount
        for _ in range(50):
            if (mnt / "dir").is_dir():
                break
            time.sleep(0.1)
        else:
            self.stop()
            raise RuntimeError("mount timeout")
        time.sleep(0.2)
        self.drain()

    def _reader(self):
        for line in self.proc.stderr:
            s = line.rstrip()
            self._q.put(s)
            if VERBOSE:
                print(f"  {Y}{s}{N}", file=sys.stderr)

    def cmd(self, s):
        self.proc.stdin.write(s + "\n")
        self.proc.stdin.flush()
        time.sleep(0.1)

    def drain(self):
        lines = []
        while not self._q.empty():
            try:
                lines.append(self._q.get_nowait())
            except queue.Empty:
                break
        return lines

    def stop(self):
        for prog in ("fusermount3", "fusermount"):
            try:
                subprocess.run(
                    [prog, "-u", str(self.mnt)], capture_output=True, timeout=5
                )
                break
            except FileNotFoundError:
                continue
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def run_case(fs, name, invalidate, cwd, path):
    """Run one test case. Returns (readdir_fresh, files)."""
    # Reset to clean state: only a.txt visible, flush all caches
    fs.cmd("reset")
    # We need to invalidate the kernel cache from the previous test.
    # Use inc_epoch itself for cleanup (harmless if unsupported).
    fs.cmd("inc_epoch")
    time.sleep(0.3)
    fs.drain()

    saved = os.getcwd()
    try:
        os.chdir(cwd)

        # Prime the path lookup and readdir cache. For the cwd=root case, this
        # specifically caches fuse_root/dir before INC_EPOCH.
        os.listdir(path)
        time.sleep(0.2)
        fs.drain()

        # Verify cache is working: second listdir should NOT hit FUSE
        os.listdir(path)
        time.sleep(0.2)
        logs = fs.drain()
        if any("[READDIRPLUS]" in l for l in logs):
            print(f"  {R}SKIP{N} {name}: readdir cache not working")
            return None

        # Make b.txt visible server-side
        fs.cmd("add")
        fs.drain()

        # Invalidate (or not, for baseline)
        if invalidate:
            fs.cmd("inc_epoch")
        time.sleep(0.2)
        fs.drain()

        files = sorted(os.listdir(path))
        time.sleep(0.2)
        fs.drain()
        fresh = "b.txt" in files
        return fresh, files
    finally:
        os.chdir(saved)


def main():
    if not TESTFS.exists():
        sys.exit(f"Build first: cd {DIR} && make")

    mnt = Path(tempfile.mkdtemp(prefix="fuse_epoch_"))
    fs = FuseDaemon(mnt)
    release = os.uname().release

    print(f"\n{B}INC_EPOCH readdir cache invalidation test{N} ({release})\n")
    print(f"  {'Test':<35s}  Result   Files")
    print(f"  {'─' * 35}  ───────  ─────────────────")

    try:
        cases = [
            ("cwd=. baseline (no invalidation)", False, fs.mnt / "dir", "."),
            ("cwd=. inc_epoch", True, fs.mnt / "dir", "."),
            ("cwd=root baseline (no invalidation)", False, fs.mnt, "dir"),
            ("cwd=root inc_epoch", True, fs.mnt, "dir"),
        ]
        for name, inval, cwd, path in cases:
            r = run_case(fs, name, inval, cwd, path)
            if r is None:
                continue
            fresh, files = r
            tag = f"{G}FRESH{N}" if fresh else f"{R}STALE{N}"
            print(f"  {name:<35s}  {tag}    {files}")

        print()
    finally:
        fs.stop()
        try:
            mnt.rmdir()
        except OSError:
            pass


if __name__ == "__main__":
    main()
