#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The ``*at()`` calls on EdenFS: every ``posix`` function with ``dir_fd``.

Ported from CPython's ``Lib/test/test_posix.py`` (``TestPosixDirFd``), PSF
license, Python 3.12: one test method per CPython test, named after it, with
the CPython class and test in the docstring, calling the ``posix`` module
directly. ``README.md`` in this directory lists every CPython test with its
method and the checks that were left out.

CPython's ``prepare()`` makes a base directory, opens it with O_RDONLY as
``dir_fd`` and uses a name that does not exist in it; ``prepare_file()`` also
creates that file. The tests do the same with ``n0`` as the base directory
and ``n1`` as the name.
"""

from __future__ import annotations

import contextlib
import errno
import posix
import stat
import time
from typing import Generator

from eden.integration.lib import testcase

from .base import PosixTestBase


@testcase.eden_nfs_repo_test
class DirFdTest(PosixTestBase):
    @contextlib.contextmanager
    def prepare(self, base: str = "n0") -> Generator[tuple[int, str, str], None, None]:
        """``TestPosixDirFd.prepare``: ``(dir_fd, name, fullname)``."""
        posix.mkdir(base, 0o755)
        with self.open_fd(base, posix.O_RDONLY) as dir_fd:
            yield (dir_fd, "n1", f"{base}/n1")

    @contextlib.contextmanager
    def prepare_file(
        self, base: str = "n0"
    ) -> Generator[tuple[int, str, str], None, None]:
        """``TestPosixDirFd.prepare_file``: ``prepare`` plus an empty file."""
        with self.prepare(base) as (dir_fd, name, fullname):
            self.create_file(fullname)
            yield (dir_fd, name, fullname)

    def test_access_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_access_dir_fd: faccessat"""
        with self.prepare_file() as (dir_fd, name, fullname):
            self.assertTrue(posix.access(name, posix.R_OK, dir_fd=dir_fd))

    def test_chmod_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_chmod_dir_fd: fchmodat"""
        with self.prepare_file() as (dir_fd, name, fullname):
            posix.chmod(fullname, stat.S_IRUSR)
            posix.chmod(name, stat.S_IRUSR | stat.S_IWUSR, dir_fd=dir_fd)
            st = posix.stat(fullname)
            self.assertEqual(stat.S_IRUSR | stat.S_IWUSR, st.st_mode & stat.S_IRWXU)

    def test_chown_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_chown_dir_fd: fchownat to the owner"""
        with self.prepare_file() as (dir_fd, name, fullname):
            posix.chown(name, posix.getuid(), posix.getgid(), dir_fd=dir_fd)

    def test_stat_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_stat_dir_fd: fstatat equals stat"""
        with self.prepare() as (dir_fd, name, fullname):
            self.create_file(fullname, b"testline\n")
            s1 = posix.stat(fullname)
            s2 = posix.stat(name, dir_fd=dir_fd)
            self.assertEqual(s1, s2)
            s2 = posix.stat(fullname, dir_fd=None)
            self.assertEqual(s1, s2)

    def test_utime_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_utime_dir_fd: utimensat with dir_fd"""
        with self.prepare_file() as (dir_fd, name, fullname):
            now = time.time()
            posix.utime(name, None, dir_fd=dir_fd)
            posix.utime(name, dir_fd=dir_fd)
            posix.utime(name, (int(now), int(now)), dir_fd=dir_fd)
            posix.utime(name, (now, now), dir_fd=dir_fd)
            posix.utime(name, (int(now), int((now - int(now)) * 1e9)), dir_fd=dir_fd)
            posix.utime(
                name, dir_fd=dir_fd, times=(int(now), int((now - int(now)) * 1e9))
            )
            posix.utime(name, follow_symlinks=False, dir_fd=dir_fd)

    def test_link_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_link_dir_fd: linkat

        EdenFS by design: no hard links. ``link`` fails with EPERM over FUSE
        and with errno 524 (``ENOTSUPP``, from ``NFS3ERR_NOTSUPP``) over NFS;
        CPython skips the test on PermissionError.
        """
        with (
            self.prepare_file() as (dir_fd, name, fullname),
            self.prepare("n2") as (dir_fd2, linkname, fulllinkname),
        ):
            with self.assert_errno(errno.EPERM, 524):
                posix.link(name, linkname, src_dir_fd=dir_fd, dst_dir_fd=dir_fd2)
            with self.assert_errno(errno.ENOENT):
                posix.stat(fulllinkname)

    def test_mkdir_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_mkdir_dir_fd: mkdirat"""
        with self.prepare() as (dir_fd, name, fullname):
            posix.mkdir(name, dir_fd=dir_fd)
            self.assertTrue(stat.S_ISDIR(posix.stat(fullname).st_mode))

    def test_mknod_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_mknod_dir_fd: mknodat makes a FIFO"""
        with self.prepare() as (dir_fd, name, fullname):
            mode = stat.S_IFIFO | stat.S_IRUSR | stat.S_IWUSR
            posix.mknod(name, mode, 0, dir_fd=dir_fd)
            self.assertTrue(stat.S_ISFIFO(posix.stat(fullname).st_mode))

    def test_open_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_open_dir_fd: openat and read"""
        with self.prepare() as (dir_fd, name, fullname):
            self.create_file(fullname, b"testline\n")
            fd = posix.open(name, posix.O_RDONLY, dir_fd=dir_fd)
            try:
                self.assertEqual(b"testline\n", posix.read(fd, 9))
            finally:
                posix.close(fd)

    def test_readlink_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_readlink_dir_fd: readlinkat"""
        with self.prepare() as (dir_fd, name, fullname):
            posix.symlink("symlink", fullname)
            self.assertEqual("symlink", posix.readlink(name, dir_fd=dir_fd))

    def test_rename_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_rename_dir_fd: renameat across two dir_fds"""
        with (
            self.prepare_file() as (dir_fd, name, fullname),
            self.prepare("n2") as (dir_fd2, name2, fullname2),
        ):
            posix.rename(name, name2, src_dir_fd=dir_fd, dst_dir_fd=dir_fd2)
            posix.stat(fullname2)
            with self.assert_errno(errno.ENOENT):
                posix.stat(fullname)
            posix.rename(fullname2, fullname)
            posix.stat(fullname)

    def test_symlink_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_symlink_dir_fd: symlinkat"""
        with self.prepare() as (dir_fd, name, fullname):
            posix.symlink("symlink", name, dir_fd=dir_fd)
            self.assertEqual("symlink", posix.readlink(fullname))

    def test_unlink_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_unlink_dir_fd: unlinkat"""
        with self.prepare() as (dir_fd, name, fullname):
            self.create_file(fullname)
            posix.stat(fullname)
            posix.unlink(name, dir_fd=dir_fd)
            with self.assert_errno(errno.ENOENT):
                posix.stat(fullname)

    def test_mkfifo_dir_fd(self) -> None:
        """test_posix.TestPosixDirFd.test_mkfifo_dir_fd: mkfifoat"""
        with self.prepare() as (dir_fd, name, fullname):
            posix.mkfifo(name, stat.S_IRUSR | stat.S_IWUSR, dir_fd=dir_fd)
            self.assertTrue(stat.S_ISFIFO(posix.stat(fullname).st_mode))

    def test_rmdir_dir_fd(self) -> None:
        """EdenFS addition (no CPython test): unlinkat with AT_REMOVEDIR"""
        with self.prepare() as (dir_fd, name, fullname):
            posix.mkdir(name, dir_fd=dir_fd)
            posix.rmdir(name, dir_fd=dir_fd)
            with self.assert_errno(errno.ENOENT):
                posix.stat(fullname)

    def test_stat_dir_fd_nofollow(self) -> None:
        """EdenFS addition (no CPython test): fstatat with AT_SYMLINK_NOFOLLOW"""
        with self.prepare_file() as (dir_fd, name, fullname):
            posix.symlink(name, "n0/n2")
            st = posix.stat("n2", dir_fd=dir_fd, follow_symlinks=False)
            self.assertTrue(stat.S_ISLNK(st.st_mode))
            st = posix.stat("n2", dir_fd=dir_fd)
            self.assertTrue(stat.S_ISREG(st.st_mode))
            self.assertEqual(posix.stat(fullname).st_ino, st.st_ino)
