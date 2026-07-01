#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Verify that FUSE_NOTIFY_INVAL_ENTRY invalidates a negative dentry.

Run test:
    make

Expected good kernel:
    - first stat populates a negative dentry for /dir/file
    - daemon makes /dir/file exist and sends FUSE_NOTIFY_INVAL_ENTRY
    - second stat reaches LOOKUP again and succeeds

Broken kernel:
    - notify returns ENOENT
    - second stat fails without a new LOOKUP
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
        expected = f"[cmd] {s}"
        if s == "add":
            expected = "[cmd] file visible"
        elif s == "inval":
            expected = "[cmd] inval parent=2 name=file -> "

        self.proc.stdin.write(s + "\n")
        self.proc.stdin.flush()
        return self._drain_until(lambda line: expected in line)

    def _drain_until(self, predicate, timeout=5.0):
        deadline = time.monotonic() + timeout
        lines = []
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("timed out waiting for daemon log")
            try:
                line = self._q.get(timeout=remaining)
            except queue.Empty as ex:
                raise TimeoutError("timed out waiting for daemon log") from ex
            lines.append(line)
            if predicate(line):
                return lines

    def drain(self):
        lines = []
        while not self._q.empty():
            try:
                lines.append(self._q.get_nowait())
            except queue.Empty:
                break
        return lines

    def stop(self):
        if self.proc.poll() is None:
            try:
                self.proc.stdin.write("quit\n")
                self.proc.stdin.flush()
            except OSError:
                pass
        unmounted = False
        unmount_error = None
        for prog in ("fusermount3", "fusermount"):
            try:
                result = subprocess.run(
                    [prog, "-u", str(self.mnt)], capture_output=True, timeout=5
                )
                if result.returncode == 0:
                    unmounted = True
                    break
                unmount_error = result.stderr.decode(errors="replace").strip()
            except FileNotFoundError:
                continue
        if not unmounted and unmount_error:
            print(
                f"warning: failed to unmount {self.mnt}: {unmount_error}",
                file=sys.stderr,
            )
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def lookup_file_logs(lines):
    return [
        line for line in lines if "[LOOKUP]" in line and "parent=2 name=file" in line
    ]


def main():
    if not TESTFS.exists():
        sys.exit(f"Build first: cd {DIR} && make")

    mnt = Path(tempfile.mkdtemp(prefix="fuse_neg_inval_"))
    fs = None

    try:
        fs = FuseDaemon(mnt)
        path = mnt / "dir" / "file"
        release = os.uname().release

        print(f"\n{B}FUSE negative dentry invalidation test{N} ({release})\n")

        r1 = subprocess.run(["stat", str(path)], capture_output=True, text=True)
        first_logs = fs.cmd("sync")
        first_lookup = len(lookup_file_logs(first_logs))

        fs.cmd("add")
        inval_logs = fs.cmd("inval")
        notify_ok = any("inval parent=2 name=file -> ok" in line for line in inval_logs)
        notify_enoent = any(
            "inval parent=2 name=file -> No such file or directory" in line
            for line in inval_logs
        )

        r2 = subprocess.run(["stat", str(path)], capture_output=True, text=True)
        second_logs = fs.cmd("sync")
        second_lookup = len(lookup_file_logs(second_logs))

        print(f"  first stat rc:       {r1.returncode}")
        print(f"  first file lookups:  {first_lookup}")
        print(
            f"  notify result:       {'ok' if notify_ok else 'ENOENT' if notify_enoent else 'other'}"
        )
        print(f"  second stat rc:      {r2.returncode}")
        print(f"  second file lookups: {second_lookup}")

        if (
            r1.returncode != 0
            and first_lookup
            and notify_ok
            and r2.returncode == 0
            and second_lookup
        ):
            print(f"\n  {G}PASS{N}: notify invalidated the negative dentry")
            return

        print(f"\n  {R}FAIL{N}: negative dentry remained stale or notify failed")
        if not VERBOSE:
            print("  rerun with -v to show daemon logs")
        sys.exit(1)
    finally:
        if fs is not None:
            fs.stop()
        try:
            mnt.rmdir()
        except OSError:
            pass


if __name__ == "__main__":
    main()
