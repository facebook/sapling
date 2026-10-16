#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Advisory locks on EdenFS: lockf, fcntl F_SETLK/F_SETLKW/F_GETLK and flock,
plus fcntl F_NOTIFY on a directory.

Ported from CPython's ``Lib/test/test_posix.py`` (``PosixTester``) and
``Lib/test/test_fcntl.py`` (``TestFcntl``), PSF license, Python 3.12: one
test method per CPython test, named after it, with the CPython class and test
in the docstring. ``lockf`` is in ``posix``; ``fcntl(2)`` and ``flock(2)`` are
only bound by the ``fcntl`` module, which the tests use for those two calls.
A second process is a ``posix.fork`` child in place of CPython's
``multiprocessing`` worker. ``README.md`` in this directory lists every
CPython test with its method and the checks that were left out.
"""

from __future__ import annotations

import errno
import fcntl
import posix
import struct
from typing import Callable

from eden.integration.lib import testcase

from .base import PosixTestBase, SEEK_SET


# struct flock on Linux x86-64 and aarch64: l_type, l_whence, l_start, l_len,
# l_pid (TestFcntl.get_lockdata uses 'hhqqhh', which leaves l_pid as a short).
FLOCK_FORMAT: str = "hhqqi"


def lockdata(l_type: int, start: int = 0, length: int = 0) -> bytes:
    return struct.pack(FLOCK_FORMAT, l_type, SEEK_SET, start, length, 0)


@testcase.eden_nfs_repo_test
class LockTest(PosixTestBase):
    def in_child(self, fn: Callable[[], int]) -> int:
        """Run ``fn`` in a forked child and return its exit code."""
        pid = posix.fork()
        if pid == 0:
            code = 99
            try:
                code = fn()
            finally:
                posix._exit(code)
        _, status = posix.waitpid(pid, 0)
        return posix.waitstatus_to_exitcode(status)

    def test_lockf(self) -> None:
        """test_posix.PosixTester.test_lockf: lockf F_LOCK and F_ULOCK on a section"""
        with self.open_fd("n0", posix.O_WRONLY | posix.O_CREAT) as fd:
            posix.write(fd, b"test")
            posix.lseek(fd, 0, SEEK_SET)
            posix.lockf(fd, posix.F_LOCK, 4)
            posix.lockf(fd, posix.F_ULOCK, 4)

    def test_fcntl_fileno(self) -> None:
        """test_fcntl.TestFcntl.test_fcntl_fileno: F_SETFL O_NONBLOCK and F_SETLKW"""
        with self.open_fd("n0", posix.O_WRONLY | posix.O_CREAT | posix.O_TRUNC) as fd:
            self.assertEqual(0, fcntl.fcntl(fd, fcntl.F_SETFL, posix.O_NONBLOCK))
            self.assertTrue(fcntl.fcntl(fd, fcntl.F_GETFL) & posix.O_NONBLOCK)
            rv = fcntl.fcntl(fd, fcntl.F_SETLKW, lockdata(fcntl.F_WRLCK))
            self.assertEqual(lockdata(fcntl.F_WRLCK), rv)

    def test_fcntl_64_bit(self) -> None:
        """test_fcntl.TestFcntl.test_fcntl_64_bit: F_NOTIFY with DN_MULTISHOT on a directory

        The flag does not fit in a C int; the kernel's dnotify accepts it on
        a directory of any file system.
        """
        with self.open_fd(".", posix.O_RDONLY) as fd:
            self.assertEqual(0, fcntl.fcntl(fd, fcntl.F_NOTIFY, fcntl.DN_MULTISHOT))

    def test_flock(self) -> None:
        """test_fcntl.TestFcntl.test_flock: shared, non-blocking and exclusive flock"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            fcntl.flock(fd, fcntl.LOCK_SH)
            fcntl.flock(fd, fcntl.LOCK_UN)
            fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
            fcntl.flock(fd, fcntl.LOCK_UN)
            fcntl.flock(fd, fcntl.LOCK_EX)
            fcntl.flock(fd, fcntl.LOCK_UN)

    def test_lockf_exclusive(self) -> None:
        """test_fcntl.TestFcntl.test_lockf_exclusive: a second process cannot lock"""

        def try_lockf_on_other_process_fail() -> int:
            fd = posix.open("n0", posix.O_RDWR)
            try:
                fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as e:
                return 0 if e.errno in (errno.EAGAIN, errno.EACCES) else 98
            return 1

        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(0, self.in_child(try_lockf_on_other_process_fail))
            fcntl.lockf(fd, fcntl.LOCK_UN)

    def test_lockf_share(self) -> None:
        """test_fcntl.TestFcntl.test_lockf_share: a second process shares the lock"""

        def try_lockf_on_other_process() -> int:
            fd = posix.open("n0", posix.O_RDWR)
            fcntl.lockf(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
            fcntl.lockf(fd, fcntl.LOCK_UN)
            return 0

        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            fcntl.lockf(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
            self.assertEqual(0, self.in_child(try_lockf_on_other_process))
            fcntl.lockf(fd, fcntl.LOCK_UN)

    def test_f_getlk(self) -> None:
        """EdenFS addition (no CPython test): F_GETLK reports the other process's lock"""

        def getlk_in_child() -> int:
            fd = posix.open("n0", posix.O_RDWR)
            got = struct.unpack(
                FLOCK_FORMAT, fcntl.fcntl(fd, fcntl.F_GETLK, lockdata(fcntl.F_WRLCK))
            )
            l_type, _, l_start, l_len, l_pid = got
            if (l_type, l_start, l_len, l_pid) != (
                fcntl.F_WRLCK,
                0,
                4,
                posix.getppid(),
            ):
                return 1
            try:
                fcntl.fcntl(fd, fcntl.F_SETLK, lockdata(fcntl.F_RDLCK, 0, 4))
                return 2
            except OSError as e:
                if e.errno not in (errno.EAGAIN, errno.EACCES):
                    return 3
            fcntl.fcntl(fd, fcntl.F_SETLK, lockdata(fcntl.F_WRLCK, 4, 0))
            got = struct.unpack(
                FLOCK_FORMAT,
                fcntl.fcntl(fd, fcntl.F_GETLK, lockdata(fcntl.F_WRLCK, 5, 1)),
            )
            return 0 if got[0] == fcntl.F_UNLCK else 4

        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT | posix.O_TRUNC) as fd:
            posix.write(fd, b"0123456789")
            fcntl.fcntl(fd, fcntl.F_SETLK, lockdata(fcntl.F_WRLCK, 0, 4))
            self.assertEqual(0, self.in_child(getlk_in_child))
            got = struct.unpack(
                FLOCK_FORMAT, fcntl.fcntl(fd, fcntl.F_GETLK, lockdata(fcntl.F_WRLCK))
            )
            self.assertEqual(fcntl.F_UNLCK, got[0])

    def test_flock_second_descriptor(self) -> None:
        """EdenFS addition (no CPython test): flock is per open file description

        A second open of the same file in the same process cannot take the
        exclusive flock; a second lockf (per process) can.
        """
        self.create_file("n0", b"0123456789")
        with (
            self.open_fd("n0", posix.O_RDWR) as fd1,
            self.open_fd("n0", posix.O_RDWR) as fd2,
        ):
            fcntl.flock(fd1, fcntl.LOCK_EX)
            with self.assert_errno(errno.EAGAIN, errno.EWOULDBLOCK):
                fcntl.flock(fd2, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.flock(fd1, fcntl.LOCK_UN)
            fcntl.flock(fd2, fcntl.LOCK_EX | fcntl.LOCK_NB)
            fcntl.flock(fd2, fcntl.LOCK_UN)
            posix.lockf(fd1, posix.F_LOCK, 4)
            posix.lockf(fd2, posix.F_TLOCK, 4)
            posix.lockf(fd1, posix.F_ULOCK, 4)
