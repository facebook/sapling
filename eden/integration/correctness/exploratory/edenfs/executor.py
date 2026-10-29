# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The POSIX and Sapling half of every action both differential sides run.

``SharedExecutor`` issues one shared action's syscalls or Sapling processes
on one checkout, named by ``path`` (relative to its root) and served by
``run_sl`` (that side's Sapling process runner). ``LiveBackend`` runs it on
the EdenFS mount and adds only Thrift channels, red controls, and faults;
``ReferenceBackend`` runs it on the plain checkout.
"""

from __future__ import annotations

import logging
import os
import stat
import subprocess
import threading
import time
from collections.abc import Callable
from typing import Protocol

from eden.integration.correctness.exploratory.core.history import FAIL, OK
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.scheduler import BackendResult
from eden.integration.correctness.exploratory.edenfs.actions import (
    CHECKOUT_CONTINUE,
    CHECKOUT_FORCE,
    differential_applicability,
    DIFFERENTIAL_OBSERVATIONS,
    EDEN_ONLY_CHANNEL,
)
from eden.integration.correctness.exploratory.edenfs.file_results import (
    failed_process,
    FILE_TYPES,
    no_descriptor,
    os_failure,
    parse_diff_stat_paths,
    parse_status,
    read_all,
    read_path,
    read_result,
    status_entries,
    write_image,
)
from eden.integration.correctness.exploratory.edenfs.sapling_processes import CLOSED

logger: logging.Logger = logging.getLogger(__name__)


class SlRunner(Protocol):
    """One side's Sapling process runner.

    ``primary`` marks an action's first process; ``started`` is when the
    action's first process started, so a side can bound them together.
    """

    def __call__(
        self, action: Action, args: list[str], *, primary: bool, started: float
    ) -> subprocess.CompletedProcess[str]: ...


def serves(spec: RunSpec, action: Action) -> bool:
    """Return whether the shared executor runs this action on either side.

    Every differential kind is shared except on a Thrift channel; the
    checkout root's listing is shared too, though only the live side runs it.
    """
    return (
        action.kind in DIFFERENTIAL_OBSERVATIONS
        and differential_applicability(spec, action).reason != EDEN_ONLY_CHANNEL
    )


class SharedExecutor:
    """Run one shared action with one syscall or Sapling process sequence, no retries."""

    def __init__(
        self,
        path: Callable[[str], str],
        payload: Callable[[Action], bytes],
        node: Callable[[int], str],
        run_sl: SlRunner,
        failure: Callable[[str, str], Exception],
    ) -> None:
        self._path = path
        self._payload = payload
        self._node = node
        self._run_sl = run_sl
        self._failure = failure
        self._lock = threading.Lock()
        # Descriptors held by completed `open` actions, keyed by that action's id.
        self.fds: dict[str, int] = {}
        self._closed = False

    def __call__(self, action: Action) -> BackendResult:  # noqa: C901
        """Run one action ``serves`` admits; an OSError is the action's failed outcome."""
        kind = action.kind
        if kind == "write":
            return self._write(action)
        if kind == "read":
            return read_path(self._path(action.payload["path"]))
        if kind == "getattr":
            return self._getattr(action.payload["path"])
        if kind == "open":
            return self._open(action)
        if kind == "read_fd":
            return self._read_fd(action)
        if kind == "close":
            return self._close(action)
        if kind in ("rename", "unlink", "mkdir", "chmod"):
            return self._path_operation(action)
        if kind == "readdir":
            return self._readdir(action.payload["path"])
        if kind == "status":
            return self._status(action)
        if kind == "checkout":
            return self._checkout(action)
        if kind in ("sl_rm", "sl_mv"):
            return self._dirstate_change(action)
        raise ValueError(f"the shared executor does not run {kind!r}")

    def close(self) -> None:
        """Close every held descriptor and refuse new ones."""
        with self._lock:
            self._closed = True
            held = tuple(self.fds.values())
            self.fds.clear()
        for fd in held:
            try:
                os.close(fd)
            except OSError:
                logger.warning("closing held fd %d failed", fd, exc_info=True)

    def _write(self, action: Action) -> BackendResult:
        """Write one 64-byte image with a single pwrite at offset 0.

        A failed open, pwrite, or close keeps its errno as a fact, and a
        failed close also records ``close_errno``: FUSE may report a
        deferred error at close.
        """
        return write_image(self._path(action.payload["path"]), self._payload(action))

    def _getattr(self, path: str) -> BackendResult:
        """Stat the path.

        ``mtime_history`` is ``st_mtime`` moved onto the monotonic clock the
        history is stamped with (CLOCK_MONOTONIC is host-wide), so the
        checker can compare it with action invoke times.
        """
        try:
            status = os.stat(self._path(path))
        except OSError as error:
            return os_failure("stat", error)
        return BackendResult(
            OK,
            {
                "size": status.st_size,
                "mtime": status.st_mtime,
                "mtime_history": status.st_mtime - (time.time() - time.monotonic()),
                "mtime_ns": status.st_mtime_ns,
                "mode": stat.S_IMODE(status.st_mode),
                "fuse_type": FILE_TYPES.get(stat.S_IFMT(status.st_mode), "other"),
            },
        )

    def _open(self, action: Action) -> BackendResult:
        """Open the path read-only and hold its descriptor for read_fd and close."""
        try:
            fd = os.open(self._path(action.payload["path"]), os.O_RDONLY)
        except OSError as error:
            return os_failure("open", error)
        with self._lock:
            if self._closed:
                os.close(fd)
                raise self._failure(CLOSED, action.id)
            self.fds[action.id] = fd
        return BackendResult(OK, {"fd": fd})

    def _held(self, action: Action, pop: bool) -> int | None:
        with self._lock:
            if pop:
                return self.fds.pop(action.payload["fd"], None)
            return self.fds.get(action.payload["fd"])

    def _read_fd(self, action: Action) -> BackendResult:
        """Read the whole file through a held descriptor; its path may be gone.

        A descriptor whose open failed holds nothing: the read fails EBADF.
        """
        fd = self._held(action, pop=False)
        if fd is None:
            return no_descriptor("read")
        try:
            return read_result(read_all(fd))
        except OSError as error:
            return os_failure("read", error)

    def _close(self, action: Action) -> BackendResult:
        """Close a held descriptor; validation orders this after its reads.

        A descriptor whose open failed holds nothing: the close fails EBADF.
        """
        fd = self._held(action, pop=True)
        if fd is None:
            return no_descriptor("close")
        try:
            os.close(fd)
        except OSError as error:
            return os_failure("close", error)
        return BackendResult(OK, {"fd": fd})

    def _path_operation(self, action: Action) -> BackendResult:
        """Issue one rename(2), chmod(2), unlink(2), or mkdir(2)."""
        payload = action.payload
        try:
            if action.kind == "rename":
                os.rename(
                    self._path(payload["source"]), self._path(payload["destination"])
                )
                return BackendResult(OK, dict(payload))
            if action.kind == "chmod":
                os.chmod(self._path(payload["path"]), payload["mode"])
                return BackendResult(
                    OK, {"path": payload["path"], "mode": payload["mode"]}
                )
            call = os.unlink if action.kind == "unlink" else os.mkdir
            call(self._path(payload["path"]))
        except OSError as error:
            return os_failure(action.kind, error)
        return BackendResult(OK, {"path": payload["path"]})

    def _readdir(self, path: str) -> BackendResult:
        """List one directory once."""
        try:
            names = os.listdir(self._path(path))
        except OSError as error:
            return os_failure("readdir", error)
        return BackendResult(OK, {"channel": "fuse", "entries": sorted(names)})

    def _status(self, action: Action) -> BackendResult:
        """Report one sl status plus its ``sl diff --stat`` file set for the two Sapling observers (status and diff).

        A nonzero exit of `sl status` fails the probe with its `returncode`,
        `stdout`, and `stderr`, which the contracts read. A failed `sl diff`
        fails it with a `reason` alone (its output in the text): no exit
        status, so the contracts leave it untyped (`_guard_refusal`).
        """
        args = ["status", "--print0"]
        revision = action.payload.get("revision")
        if revision is not None:
            args.extend(["--rev", self._node(revision)])
        started = time.monotonic()
        status = self._run_sl(action, args, primary=True, started=started)
        if status.returncode != 0:
            return failed_process("status", status)
        diff = self._run_sl(
            action, ["diff", "--stat", "tree"], primary=False, started=started
        )
        if diff.returncode != 0:
            return BackendResult(
                FAIL, {"reason": failed_process("diff", diff).facts["reason"]}
            )
        return BackendResult(
            OK,
            {
                "status": status_entries(parse_status(status.stdout)),
                "diff_paths": parse_diff_stat_paths(diff.stdout),
            },
        )

    def _checkout(self, action: Action) -> BackendResult:
        """Move to the target commit with one `sl goto`.

        `mode: continue` runs `sl goto --continue`, which resumes the
        destination `.hg/updatestate` names. A nonzero exit is the
        action's FAIL data.
        """
        revision = action.payload["revision"]
        node = self._node(revision)
        via = action.payload["via"]
        mode = action.payload["mode"]
        target = "--continue" if mode == CHECKOUT_CONTINUE else node
        # `goto --clean` is Sapling's forced checkout (`edenfs_force_checkout`).
        args = [via, "--clean", target] if mode == CHECKOUT_FORCE else [via, target]
        result = self._run_sl(action, args, primary=True, started=time.monotonic())
        if result.returncode != 0:
            return failed_process(via, result)
        return BackendResult(OK, {"revision": revision, "node": node, "via": via})

    def _dirstate_change(self, action: Action) -> BackendResult:
        """Run one ``sl rm --force`` or copy-tracking ``sl mv`` and report its paths."""
        payload = action.payload
        if action.kind == "sl_rm":
            args = ["rm", "--force", payload["path"]]
        else:
            args = ["mv", payload["source"], payload["destination"]]
        result = self._run_sl(action, args, primary=True, started=time.monotonic())
        if result.returncode != 0:
            return failed_process(args[0], result)
        return BackendResult(OK, dict(payload))
