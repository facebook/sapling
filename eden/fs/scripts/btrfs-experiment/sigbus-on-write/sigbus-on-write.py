#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Show when an mmap write to a full btrfs filesystem turns into SIGBUS.

Models the stores MappedDiskVector makes into an already established table: a
page that was mapped and written back long ago is written again, on a
filesystem that has since run out of data space. The page under test is an
ordinary entry page rather than the header, because nothing in the failing path
is specific to the header -- `++header().entryCount` is only where production
crashed, since it is the one store that skips populateForWrite().

Five cases on five throwaway filesystems, differing in the file's COW attribute
and in where the pre-fault sits relative to the store. Each case prints its
event ordering before it runs. Needs root for mkfs/mount; everything lives in a
loop image under /var/tmp and is removed on exit.
"""

import ctypes
import errno
import mmap
import os
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
from typing import NamedTuple


IMAGE_SIZE = 256 * 1024 * 1024
MAPPING_SIZE = 1024 * 1024
FILL_CHUNK_SIZE = 1024 * 1024
MADV_POPULATE_WRITE = 23
MOUNT_OPTIONS = "loop,compress-force=zstd:3,ssd,discard,space_cache=v2"

# An arbitrary page in the middle of the mapping: neither the header nor the
# last page, so the store is a plain overwrite with no growth involved.
TARGET_PAGE = 128
TARGET_OFFSET = TARGET_PAGE * mmap.PAGESIZE

# btrfs_page_mkwrite() only grew a NOCOW fallback in v6.17, commit 6599716de2d6
# "btrfs: fix -ENOSPC mmap write failure on NOCOW files/extents". Before that it
# fails the fault as soon as btrfs_check_data_free_space() fails, without ever
# calling btrfs_check_nocow_lock(), so `chattr +C` changes nothing for an mmap
# store. write(2) and direct IO have had the fallback for far longer.
NOCOW_MMAP_FIX = (6, 17)
NOCOW_MMAP_FIX_COMMIT = "6599716de2d6"
NOCOW_MMAP_FIX_SUBJECT = "btrfs: fix -ENOSPC mmap write failure on NOCOW files/extents"

EXIT_SURVIVED = 0
EXIT_MADVISE_REFUSED = 20
EXIT_WRITEBACK_FAILED = 21

SIGBUS = "SIGBUS"
SURVIVED = "survived"
MADVISE_REFUSED = "madvise EFAULT"
WRITEBACK_FAILED = "msync failed"

# Where the pre-fault happens relative to the store under test.
NO_PREFAULT = "none"
# Before the filesystem fills, and undone by writeback afterwards. This is the
# populate/store race made deterministic: production loses the writable PTE to
# the periodic writeback thread instead, in a window of a few hundred ns.
STALE_PREFAULT = "stale"
# Immediately before the store, which is what populateEntryForWrite() does.
FRESH_PREFAULT = "fresh"


class Case(NamedTuple):
    name: str
    nocow: bool
    prefault: str


CASES = (
    Case("COW, no pre-fault", False, NO_PREFAULT),
    Case("COW, pre-fault immediately before the store", False, FRESH_PREFAULT),
    Case("COW, pre-fault undone by writeback", False, STALE_PREFAULT),
    Case("NODATACOW, no pre-fault", True, NO_PREFAULT),
    Case("NODATACOW, pre-fault undone by writeback", True, STALE_PREFAULT),
)


def say(text: str) -> None:
    # SIGBUS kills the child mid-line; unflushed progress output would be lost
    # and the run would look like it never filled the filesystem.
    print(text, flush=True)


def timeline(case: Case) -> list[str]:
    """The case's events in order, printed before the case runs."""
    steps = [
        "create the table file"
        + (", chattr +C while it is still empty" if case.nocow else ""),
        "fallocate 1 MiB, mmap, MADV_POPULATE_WRITE the whole mapping (open time)",
        "store to every page",
        "msync: the extents become regular and the PTEs are write-protected "
        "again (steady state)",
    ]
    if case.prefault == STALE_PREFAULT:
        steps.append(
            f"MADV_POPULATE_WRITE page {TARGET_PAGE}, then msync it back out, "
            f"so the pre-fault is already stale"
        )
    steps.append("fill the filesystem until ENOSPC  <-- the disk runs out here")
    if case.prefault == FRESH_PREFAULT:
        steps.append(f"MADV_POPULATE_WRITE page {TARGET_PAGE}")
    steps.append(f"store to page {TARGET_PAGE}  <-- the write under test")
    return steps


def madvise_populate_write(address: int, length: int) -> int:
    """Pre-faults the range for writing. Returns 0, or the errno on failure."""
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.madvise(
        ctypes.c_void_p(address), ctypes.c_size_t(length), MADV_POPULATE_WRITE
    ):
        return ctypes.get_errno()
    return 0


def fill(mountpoint: str) -> None:
    filler_path = os.path.join(mountpoint, "filler")
    fd = os.open(filler_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    # Incompressible, so compress-force cannot buy the kernel extra room.
    chunk = os.urandom(FILL_CHUNK_SIZE)
    written = 0
    try:
        while True:
            written += os.write(fd, chunk)
    except OSError as error:
        if error.errno != errno.ENOSPC:
            raise
    # The delalloc reservations only become real extents here; without the
    # fsync the filesystem still has room by the time we fault.
    try:
        os.fsync(fd)
    except OSError as error:
        if error.errno != errno.ENOSPC:
            raise
    os.close(fd)
    say(f"  filled {written} bytes, then ENOSPC")


def report_space(mountpoint: str) -> None:
    output = subprocess.run(
        ["btrfs", "filesystem", "usage", "-b", mountpoint],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    for line in output.splitlines():
        line = line.strip()
        if line.startswith(("Data,", "Metadata,", "Device unallocated")):
            say(f"  {line}")


def child(mountpoint: str, case: Case) -> int:
    table_path = os.path.join(mountpoint, "table")
    fd = os.open(table_path, os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o600)
    if case.nocow:
        # NODATACOW is only accepted while the file is still empty.
        subprocess.run(["chattr", "+C", table_path], check=True)
        attributes = subprocess.run(
            ["lsattr", "-d", table_path],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.split()[0]
        say(f"  attributes: {attributes}")

    os.posix_fallocate(fd, 0, MAPPING_SIZE)
    mapping = mmap.mmap(fd, MAPPING_SIZE, flags=mmap.MAP_SHARED)
    address = ctypes.addressof(ctypes.c_char.from_buffer(mapping))

    # What MappedDiskVector does once at open time.
    error = madvise_populate_write(address, MAPPING_SIZE)
    if error:
        raise OSError(error, os.strerror(error))
    for offset in range(0, MAPPING_SIZE, mmap.PAGESIZE):
        mapping[offset] = ord("a")
    # Writeback turns the fallocate'd PREALLOC extents into regular ones and
    # write-protects the PTEs again. Both matter: the store below has to fault
    # a second time, and without NODATACOW that fault can no longer take the
    # nocow shortcut. This is the steady state the production table is in.
    mapping.flush()

    if case.prefault == STALE_PREFAULT:
        error = madvise_populate_write(address + TARGET_OFFSET, mmap.PAGESIZE)
        if error:
            raise OSError(error, os.strerror(error))
        mapping.flush(TARGET_OFFSET, mmap.PAGESIZE)
        say(f"  page {TARGET_PAGE} was pre-faulted, then written back again")

    fill(mountpoint)
    report_space(mountpoint)

    if case.prefault == FRESH_PREFAULT:
        error = madvise_populate_write(address + TARGET_OFFSET, mmap.PAGESIZE)
        if error:
            # Never ENOSPC: vm_fault_t cannot carry an errno, so the kernel
            # collapses the failure into VM_FAULT_SIGBUS and madvise() reports
            # EFAULT. The point is that it is a return value, not a signal.
            say(f"  RESULT: madvise refused, errno={error} ({os.strerror(error)})")
            return EXIT_MADVISE_REFUSED

    mapping[TARGET_OFFSET] = ord("b")
    # Not faulting is only half of surviving: the write still has to reach the
    # disk, and that failure would arrive later as an errno on msync rather
    # than as a signal. Checking it here keeps "survived" meaning "no error".
    try:
        mapping.flush(TARGET_OFFSET, mmap.PAGESIZE)
    except OSError as error:
        say(f"  RESULT: store completed, but writeback failed: {error}")
        return EXIT_WRITEBACK_FAILED
    say("  RESULT: store completed and reached the disk")
    return EXIT_SURVIVED


def run_case(image: str, mountpoint: str, case: Case) -> str:
    with open(image, "wb") as image_file:
        image_file.truncate(IMAGE_SIZE)
    subprocess.run(["mkfs.btrfs", "-q", "-f", image], check=True)
    subprocess.run(["mount", "-o", MOUNT_OPTIONS, image, mountpoint], check=True)

    try:
        sys.stdout.flush()
        # The store may die from SIGBUS, so it runs in a child.
        pid = os.fork()
        if pid == 0:
            try:
                os._exit(child(mountpoint, case))
            except Exception as error:
                print(f"  child failed: {error}", file=sys.stderr, flush=True)
                os._exit(2)
        _, status = os.waitpid(pid, 0)
    finally:
        subprocess.run(["umount", mountpoint], check=True)

    if os.WIFSIGNALED(status) and os.WTERMSIG(status) == signal.SIGBUS:
        return SIGBUS
    if os.WIFEXITED(status) and os.WEXITSTATUS(status) == EXIT_SURVIVED:
        return SURVIVED
    if os.WIFEXITED(status) and os.WEXITSTATUS(status) == EXIT_MADVISE_REFUSED:
        return MADVISE_REFUSED
    if os.WIFEXITED(status) and os.WEXITSTATUS(status) == EXIT_WRITEBACK_FAILED:
        return WRITEBACK_FAILED
    return f"unexpected wait status {status}"


def expected(case: Case, nocow_works: bool) -> str:
    if case.nocow and nocow_works:
        return SURVIVED
    return MADVISE_REFUSED if case.prefault == FRESH_PREFAULT else SIGBUS


def conclusion(nocow_works: bool) -> str:
    return "\n".join(
        [
            "== notes ==",
            "Not covered here, all of them data ENOSPC's neighbours rather than",
            "variations of it:",
            "- Metadata ENOSPC. NODATACOW only trims that reservation (by ~40%, via the",
            "  NODATASUM it implies), so cases 4 and 5 say nothing about it. Left out",
            "  because it is hard to construct: the real filesystems that get there",
            "  spend weeks drifting into it, and at this image size the filler needs a",
            "  reservation as large as the write under test, so metadata never gets",
            "  close to full. In production it also looks different -- a transaction",
            "  abort that forces the whole filesystem read-only, not a single SIGBUS.",
            "- The first write to a snapshot- or reflink-shared extent, which has to COW",
            "  even under NODATACOW. These images are freshly mkfs'd and never",
            "  snapshotted.",
            "- Checksum and IO errors, which reach the same VM_FAULT_SIGBUS by another",
            "  route and would need error injection to trigger.",
        ]
    )


def kernel_version() -> tuple[int, int]:
    release = platform.release()
    match = re.match(r"(\d+)\.(\d+)", release)
    if match is None:
        raise RuntimeError(f"cannot parse kernel version: {release}")
    major, minor = match.groups()
    return int(major), int(minor)


def main() -> int:
    if os.geteuid() != 0:
        print(f"Run as root: sudo {sys.argv[0]}", file=sys.stderr)
        return 2

    for command in ("btrfs", "chattr", "lsattr", "mkfs.btrfs", "mount", "umount"):
        if shutil.which(command) is None:
            print(f"Missing required command: {command}", file=sys.stderr)
            return 2

    fix = "{}.{}".format(*NOCOW_MMAP_FIX)
    nocow_works = kernel_version() >= NOCOW_MMAP_FIX
    print(f"kernel {platform.release()}")
    print(
        f"{'contains' if nocow_works else 'might not contain'} v{fix} "
        f'{NOCOW_MMAP_FIX_COMMIT} "{NOCOW_MMAP_FIX_SUBJECT}",'
    )

    temp_dir = tempfile.mkdtemp(prefix="btrfs-sigbus-", dir="/var/tmp")
    image = os.path.join(temp_dir, "fs.img")
    mountpoint = os.path.join(temp_dir, "mnt")
    os.mkdir(mountpoint)

    results = []
    try:
        for number, case in enumerate(CASES, start=1):
            print(f"\n== case {number}: {case.name} ==")
            for step, text in enumerate(timeline(case), start=1):
                print(f"  {step}. {text}")
            results.append(
                (case, expected(case, nocow_works), run_case(image, mountpoint, case))
            )
    finally:
        shutil.rmtree(temp_dir)

    print("\n== summary ==")
    for number, (case, want, got) in enumerate(results, start=1):
        verdict = "ok" if want == got else "UNEXPECTED"
        print(f"{number}. {case.name:<44} expected {want:<15} got {got:<15} {verdict}")

    print()
    print(conclusion(nocow_works))

    return 0 if all(want == got for _, want, got in results) else 1


if __name__ == "__main__":
    sys.exit(main())
