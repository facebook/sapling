#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Observe whether FUSE invalidation removes a bind mount over a FUSE directory.

Run:
    make

The test mounts a minimal FUSE filesystem with /dir, then bind-mounts a real
directory over that path. It invalidates either:
  - the covered child directory inode,
  - the parent/root directory inode,
  - the parent/name entry as a control case,
  - or the global FUSE epoch.

For each invalidation, it also controls what a future lookup("dir") would
return: the original inode or a different inode. The output reports whether
the bind mount was lost and whether FUSE saw another lookup for /dir.
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
FUSE_DIR_NAME = "dir"

G, R, Y, B, N = "\033[32m", "\033[31m", "\033[33m", "\033[1m", "\033[0m"


class FuseMountError(RuntimeError):
    pass


class BindMountError(RuntimeError):
    pass


class FuseDaemon:
    def __init__(self, mnt: Path) -> None:
        self.mnt = mnt
        self.proc = subprocess.Popen(
            [str(TESTFS), "-f", str(mnt)],
            stdin=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        self._q: queue.Queue[str] = queue.Queue()
        threading.Thread(target=self._reader, daemon=True).start()
        for _ in range(50):
            if (mnt / FUSE_DIR_NAME).is_dir():
                break
            time.sleep(0.1)
        else:
            self.stop()
            raise FuseMountError("FUSE mount timeout")
        time.sleep(0.2)
        self.drain()

    def _reader(self) -> None:
        assert self.proc.stderr is not None
        for line in self.proc.stderr:
            line = line.rstrip()
            self._q.put(line)
            if VERBOSE:
                print(f"  {Y}{line}{N}", file=sys.stderr)

    def cmd(self, command: str) -> None:
        assert self.proc.stdin is not None
        self.proc.stdin.write(command + "\n")
        self.proc.stdin.flush()
        time.sleep(0.15)

    def drain(self) -> list[str]:
        lines = []
        while not self._q.empty():
            try:
                lines.append(self._q.get_nowait())
            except queue.Empty:
                break
        return lines

    def stop(self) -> None:
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


def run_checked(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, capture_output=True, text=True, timeout=5)


def unmount(path: Path) -> None:
    run_checked(["umount", str(path)])


def visible_source(path: Path) -> str:
    entries = sorted(p.name for p in path.iterdir())
    if "source.txt" in entries:
        return "bind"
    if "fuse.txt" in entries:
        return "fuse"
    return ",".join(entries) if entries else "empty"


def run_case(invalidation: str, dir_inode: str) -> tuple[bool, bool, list[str]]:
    with tempfile.TemporaryDirectory(prefix="fuse_bind_") as td:
        base = Path(td)
        mnt = base / "mnt"
        src = base / "src"
        mnt.mkdir()
        src.mkdir()
        (src / "source.txt").write_text("bind source\n")

        fs = FuseDaemon(mnt)
        try:
            covered_path = mnt / FUSE_DIR_NAME
            ret = run_checked(["mount", "--bind", str(src), str(covered_path)])
            if ret.returncode != 0:
                raise BindMountError(ret.stderr.strip() or ret.stdout.strip())

            try:
                before = visible_source(covered_path)
                fs.drain()
                fs.cmd("different" if dir_inode == "changed" else dir_inode)
                fs.cmd(invalidation)
                logs = fs.drain()

                after = visible_source(covered_path)
                time.sleep(0.2)
                logs.extend(fs.drain())
                got_dir_lookup = any(
                    f"[LOOKUP] parent=1 name={FUSE_DIR_NAME}" in line for line in logs
                )
                lost_bind = before == "bind" and after != "bind"
                return lost_bind, got_dir_lookup, logs
            finally:
                unmount(covered_path)
        finally:
            fs.stop()


def main() -> None:
    if not TESTFS.exists():
        sys.exit(f"Build first: cd {DIR} && make")

    print(f"\n{B}FUSE bind mount invalidation experiment{N} ({os.uname().release})\n")
    print(f"  {'Test setup':<57s} | {'Test observation':<29s}")
    print(
        f"  {'Invalidation':<44s}  {'Dir ino':<10s}  | "
        f"{'Lost bind':<9s}  {'Got dir lookup':<14s}"
    )
    print(f"  {'-' * 44}  {'-' * 10}  | {'-' * 9}  {'-' * 14}")

    cases = [
        ("inval_child", "same"),
        ("inval_child", "changed"),
        ("inval_parent", "same"),
        ("inval_parent", "changed"),
        ("inval_entry", "same"),
        ("inval_entry", "changed"),
        ("inc_epoch", "same"),
        ("inc_epoch", "changed"),
    ]
    labels = {
        "inval_child": "FUSE_NOTIFY_INVAL_INODE(dir)",
        "inval_parent": "FUSE_NOTIFY_INVAL_INODE(parent)",
        "inval_entry": 'FUSE_NOTIFY_INVAL_ENTRY(parent, "dir")',
        "inc_epoch": "FUSE_NOTIFY_INC_EPOCH",
    }
    for invalidation, dir_inode in cases:
        try:
            lost_bind, got_dir_lookup, logs = run_case(invalidation, dir_inode)
        except FuseMountError as err:
            print(f"\n{R}SKIP{N}: {err}")
            print("This experiment needs a working unprivileged FUSE mount.")
            return
        except BindMountError as err:
            print(f"\n{R}SKIP{N}: bind mount failed: {err}")
            print("This experiment needs permission to run `mount --bind`.")
            return

        lost_bind_text = "yes" if lost_bind else "no"
        lost_bind_color = R if lost_bind else G
        got_dir_lookup_text = "yes" if got_dir_lookup else "no"
        print(
            f"  {labels[invalidation]:<44s}  {dir_inode:<10s}  | "
            f"{lost_bind_color}{lost_bind_text:<9s}{N}  {got_dir_lookup_text:<14s}"
        )
        if VERBOSE:
            for line in logs:
                print(f"      {line}")

    print()


if __name__ == "__main__":
    main()
