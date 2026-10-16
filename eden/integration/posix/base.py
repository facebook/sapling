#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared setup for the tests ported from pjdfstest.

pjdfstest (https://github.com/pjd/pjdfstest, BSD-2-Clause) is a POSIX file
system test suite. The test modules next to this file are translations of
its ``tests/*/*.t`` scripts; ``README.md`` explains the translation and lists
the cases that were left out.

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
