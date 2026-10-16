#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared setup for the POSIX file system tests.

The test modules next to this file come from two sources: pjdfstest
(https://github.com/pjd/pjdfstest, BSD-2-Clause), a POSIX file system test
suite whose ``tests/*/*.t`` scripts check path-based semantics, and CPython's
``Lib/test/test_posix.py``, ``Lib/test/test_os.py``, ``Lib/test/test_fcntl.py``
and ``Lib/test/test_mmap.py`` (PSF license, 3.12), whose file system tests
check fd-based I/O, the ``*at()`` calls, space, locks, mmap, names, xattrs and
directory iteration. ``README.md`` explains both translations and lists the
cases that were left out.

The tests call the ``posix`` module, the direct binding of the POSIX system
calls, rather than the portable ``os`` layer on top of it.
"""

from __future__ import annotations

import contextlib
import ctypes
import errno
import posix
import socket
from typing import Generator

from eden.integration.lib import testcase


UTIME_OMIT: tuple[int, int] = (0, (1 << 30) - 2)

# ``posix`` binds SEEK_DATA and SEEK_HOLE but leaves these three to ``os``.
SEEK_SET: int = 0
SEEK_CUR: int = 1
SEEK_END: int = 2

_AT_FDCWD = -100


class _Timespec(ctypes.Structure):
    _fields_ = [("tv_sec", ctypes.c_long), ("tv_nsec", ctypes.c_long)]


def utimensat(path: str, atime: tuple[int, int], mtime: tuple[int, int]) -> None:
    """``utimensat(2)`` with ``(seconds, nanoseconds)`` pairs.

    ``posix.utime`` cannot leave one timestamp alone; pass ``UTIME_OMIT`` here.
    """
    times = (_Timespec * 2)()
    times[0].tv_sec, times[0].tv_nsec = atime
    times[1].tv_sec, times[1].tv_nsec = mtime
    libc = ctypes.CDLL(None, use_errno=True)
    if libc.utimensat(_AT_FDCWD, path.encode(), times, 0) != 0:
        err = ctypes.get_errno()
        raise OSError(err, posix.strerror(err), path)


class PosixTestBase(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    """Each test runs in its own empty directory inside a fresh mount.

    The pjdfstest scripts run from a scratch directory with ``umask 0`` and
    name files relative to it, so the tests do the same: the working directory
    of the process is the scratch directory while a test runs.
    """

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        scratch = f"{self.mount}/scratch"
        posix.mkdir(scratch, 0o755)
        previous_cwd = posix.getcwd()
        previous_umask = posix.umask(0)
        posix.chdir(scratch)
        self.addCleanup(posix.chdir, previous_cwd)
        self.addCleanup(posix.umask, previous_umask)

    @contextlib.contextmanager
    def assert_errno(self, *expected: int) -> Generator[None, None, None]:
        """Assert that the block raises ``OSError`` with one of ``expected``."""
        with self.assertRaises(OSError) as context:
            yield
        actual = context.exception.errno
        if actual not in expected:
            self.fail(
                "expected "
                + "|".join(errno.errorcode[e] for e in expected)
                + f", got {errno.errorcode.get(actual or 0, str(actual))}"
            )

    def bind_unix_socket(self, path: str) -> None:
        """``create_file socket``: ``bind(2)`` an AF_UNIX socket at ``path``."""
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.bind(path)

    def create_file(self, path: str | bytes, data: bytes = b"") -> None:
        """CPython's ``create_file``: create or truncate ``path`` with ``data``.

        (``EdenRepoTest.write_file`` takes a path relative to the mount and
        text; this one takes the scratch-relative path and bytes.)
        """
        fd = posix.open(path, posix.O_WRONLY | posix.O_CREAT | posix.O_TRUNC, 0o644)
        try:
            posix.write(fd, data)
        finally:
            posix.close(fd)

    def read_bytes(self, path: str | bytes) -> bytes:
        """The whole content of ``path``."""
        with self.open_fd(path, posix.O_RDONLY) as fd:
            chunks = []
            while chunk := posix.read(fd, 65536):
                chunks.append(chunk)
            return b"".join(chunks)

    @contextlib.contextmanager
    def open_fd(
        self, path: str | bytes, flags: int, mode: int = 0o644
    ) -> Generator[int, None, None]:
        """``posix.open`` with the ``posix.close`` in a ``finally``."""
        fd = posix.open(path, flags, mode)
        try:
            yield fd
        finally:
            posix.close(fd)

    def mkdir_p(self, path: str) -> None:
        """The scripts' ``mkdir -p``."""
        prefix = ""
        for component in path.split("/"):
            prefix = f"{prefix}{component}/"
            posix.mkdir(prefix, 0o755)

    def rm_rf(self, path: str) -> None:
        """The scripts' ``rm -rf``."""
        for entry in posix.scandir(path):
            if entry.is_dir(follow_symlinks=False):
                self.rm_rf(entry.path)
            else:
                posix.unlink(entry.path)
        posix.rmdir(path)

    def name_max(self) -> str:
        """A file name of exactly {NAME_MAX} bytes."""
        return "x" * posix.pathconf(".", "PC_NAME_MAX")

    def path_max(self) -> str:
        """A relative path of exactly {PATH_MAX} - 1 bytes (room for the NUL)."""
        component = "x" * (posix.pathconf(".", "PC_NAME_MAX") // 2) + "/"
        length = posix.pathconf(".", "PC_PATH_MAX") - 1
        path = (component * (length // len(component) + 1))[:length]
        if path.endswith("/"):
            path = path[:-1] + "x"
        return path
