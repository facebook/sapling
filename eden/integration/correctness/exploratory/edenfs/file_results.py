# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Backend results shared by the EdenFS and reference sides of a differential pair.

Both sides report one syscall or Sapling process the same way: a failed call
keeps its errno, a read reports the content digest, an empty read fails, and
a failed process keeps its exit status and output. Using one set of helpers
keeps the two observations comparable.
"""

from __future__ import annotations

import errno
import os
import stat
import subprocess
from collections.abc import Mapping
from typing import Any

from eden.integration.correctness.exploratory.core.checker import content_digest
from eden.integration.correctness.exploratory.core.history import FAIL, OK
from eden.integration.correctness.exploratory.core.scheduler import BackendResult

READ_CHUNK = 1 << 20
FILE_TYPES = {stat.S_IFREG: "file", stat.S_IFDIR: "directory", stat.S_IFLNK: "symlink"}


def os_failure(operation: str, error: OSError) -> BackendResult:
    """Return one failed syscall with its errno kept as a fact."""
    return BackendResult(
        FAIL, {"reason": f"{operation}: {error}", "errno": error.errno}
    )


def no_descriptor(operation: str) -> BackendResult:
    """A descriptor operation whose `open` failed or never ran: EBADF, as the kernel reports."""
    return BackendResult(
        FAIL,
        {
            "reason": f"{operation}: no descriptor (its open failed)",
            "errno": errno.EBADF,
        },
    )


def close_failure(fd: int, facts: dict[str, Any]) -> BackendResult | None:
    """Close a written descriptor; if that fails, fail with ``close_errno`` added to ``facts``.

    ``facts`` keeps its ``errno`` and reason when the write already failed;
    otherwise the close's own errno and reason are recorded. Returns None
    when the close succeeds.
    """
    try:
        os.close(fd)
    except OSError as error:
        if "errno" not in facts:
            facts = {**facts, **os_failure("close", error).facts}
        return BackendResult(FAIL, {**facts, "close_errno": error.errno})
    return None


def read_all(fd: int) -> bytes:
    """Return the file's bytes from offset 0 without moving the descriptor.

    The next offset is a running total, so each chunk is measured once.
    """
    chunks: list[bytes] = []
    offset = 0
    while chunk := os.pread(fd, READ_CHUNK, offset):
        chunks.append(chunk)
        offset += len(chunk)
    return b"".join(chunks)


def read_result(data: bytes) -> BackendResult:
    """Return a whole-file read: its digest facts, or a failure when it is empty."""
    if not data:
        return BackendResult(FAIL, {"reason": "empty read"})
    return BackendResult(OK, content_digest(data))


def write_image(path: str, payload: bytes) -> BackendResult:
    """Write one image with a single pwrite at offset 0.

    A failed open, pwrite, or close keeps its errno as a fact, and a failed
    close also records ``close_errno``: FUSE may report a deferred error at
    close.
    """
    try:
        fd = os.open(path, os.O_WRONLY)
    except OSError as error:
        return os_failure("open", error)
    try:
        written = os.pwrite(fd, payload, 0)
    except OSError as error:
        failed = os_failure("pwrite", error)
        return close_failure(fd, failed.facts) or failed
    closed = close_failure(fd, {"bytes_written": written})
    if closed is not None:
        return closed
    if written != len(payload):
        return BackendResult(FAIL, {"reason": f"short write {written}/{len(payload)}"})
    return BackendResult(OK, {"bytes_written": written})


def read_path(path: str) -> BackendResult:
    """Read one whole file by path, reporting its digest facts."""
    try:
        fd = os.open(path, os.O_RDONLY)
    except OSError as error:
        return os_failure("open", error)
    try:
        return read_result(read_all(fd))
    except OSError as error:
        return os_failure("read", error)
    finally:
        os.close(fd)


def failed_process(
    operation: str, result: subprocess.CompletedProcess[str]
) -> BackendResult:
    """Return one raw target failure with the process's actual output."""
    output = result.stderr.strip() or result.stdout.strip()
    detail = output or f"process exited {result.returncode}"
    return BackendResult(
        FAIL,
        {
            "reason": f"{operation}: {detail}",
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        },
    )


def parse_diff_stat_paths(text: str) -> list[str]:
    """Return the file column of ``sl diff --stat`` output.

    Data lines look like `` tree/churn/c0 | 1 +-``; the trailing
    summary (``1 file changed, ...``) carries no pipe and is skipped.
    """
    paths: list[str] = []
    for line in text.splitlines():
        head, sep, _rest = line.partition("|")
        if not sep:
            continue
        path = head.strip()
        if path:
            paths.append(path)
    return paths


def parse_status(text: str) -> dict[str, str]:
    """Parse ``sl status --print0`` into a path-to-flag observation."""
    status: dict[str, str] = {}
    for entry in text.split("\0"):
        if not entry:
            continue
        path = entry[2:]
        if path != "default.profraw":
            status[path] = entry[0]
    return status


def status_entries(status: Mapping[str, str]) -> list[dict[str, str]]:
    """Return a path-to-code status map as the result schema's entry list, by path."""
    return [{"path": path, "code": status[path]} for path in sorted(status)]
