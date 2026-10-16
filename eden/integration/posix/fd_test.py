#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""File descriptors on EdenFS: dup, fstat, fchmod, fchown, utime, open flags
and metadata reached through a descriptor.

Ported from CPython's ``Lib/test/test_posix.py`` (``PosixTester``) and
``Lib/test/test_os.py`` (``FileTests``, ``StatAttributeTests``,
``FDInheritanceTests``, ``UtimeTests``), PSF license, Python 3.12: one test
method per CPython test, named after it, with the CPython class and test in
the docstring, calling the ``posix`` module directly. The ``UtimeTests``
methods live in their own class because ``PosixTester`` has tests of the same
names. ``README.md`` in this directory lists every CPython test with its
method and the checks that were left out.
"""

from __future__ import annotations

import errno
import mmap
import posix
import stat
import time
from typing import Callable

from eden.integration.lib import testcase

from .base import PosixTestBase, SEEK_CUR, SEEK_SET


@testcase.eden_nfs_repo_test
class FdTest(PosixTestBase):
    def test_dup(self) -> None:
        """test_posix.PosixTester.test_dup: dup of an open file"""
        self.create_file("n0", b"abc")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            fd2 = posix.dup(fd)
            try:
                self.assertIsInstance(fd2, int)
                self.assertEqual(b"abc", posix.read(fd2, 3))
                self.assertEqual(3, posix.lseek(fd, 0, SEEK_CUR))
            finally:
                posix.close(fd2)

    def test_dup2(self) -> None:
        """test_posix.PosixTester.test_dup2: dup2 over a second open file"""
        self.create_file("n0", b"abc")
        with (
            self.open_fd("n0", posix.O_RDONLY) as fd1,
            self.open_fd("n0", posix.O_RDONLY) as fd2,
        ):
            posix.read(fd1, 1)
            self.assertEqual(fd2, posix.dup2(fd1, fd2))
            self.assertEqual(b"bc", posix.read(fd2, 2))

    def test_fstat(self) -> None:
        """test_posix.PosixTester.test_fstat: fstat and stat of a descriptor"""
        self.create_file("n0", b"abc")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            self.assertTrue(posix.fstat(fd))
            self.assertTrue(posix.stat(fd))
            self.assertEqual(posix.stat("n0"), posix.fstat(fd))

    def test_stat(self) -> None:
        """test_posix.PosixTester.test_stat: stat with a str and a bytes path"""
        self.create_file("n0")
        self.assertTrue(posix.stat("n0"))
        self.assertTrue(posix.stat(b"n0"))
        self.assertEqual(posix.stat("n0"), posix.stat(b"n0"))

    def test_stat_attributes(self) -> None:
        """test_os.StatAttributeTests.test_stat_attributes: the stat fields agree

        The checks of the Python ``stat_result`` type (indexing, assignment,
        constructor) are left out; the file system values stay.
        """
        self.check_stat_attributes("n0")

    def test_stat_attributes_bytes(self) -> None:
        """test_os.StatAttributeTests.test_stat_attributes_bytes: same with a bytes path"""
        self.check_stat_attributes(b"n0")

    def check_stat_attributes(self, fname: str | bytes) -> None:
        self.create_file(fname, b"ABC")
        result = posix.stat(fname)
        self.assertEqual(3, result[stat.ST_SIZE])
        self.assertEqual(3, result.st_size)
        for name in dir(stat):
            if name[:3] == "ST_":
                attr = name.lower()
                value = getattr(result, attr)
                if name.endswith("TIME"):
                    value = int(value)
                self.assertEqual(result[getattr(stat, name)], value)
        for name in ("st_atime", "st_mtime", "st_ctime"):
            floaty = int(getattr(result, name) * 100000)
            nanosecondy = getattr(result, name + "_ns") // 10000
            self.assertAlmostEqual(floaty, nanosecondy, delta=2)
        self.assertEqual(1, result.st_blocks)
        self.assertGreater(result.st_blksize, 0)

    def test_access(self) -> None:
        """test_os.FileTests.test_access: a new file is writable"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_RDWR, 0o644))
        self.assertTrue(posix.access("n0", posix.W_OK))

    def test_access_r_ok(self) -> None:
        """test_posix.PosixTester.test_access: a new file is readable"""
        self.create_file("n0")
        self.assertTrue(posix.access("n0", posix.R_OK))

    def test_closerange(self) -> None:
        """test_os.FileTests.test_closerange: closerange over an open and a closed fd"""
        first = posix.open("n0", posix.O_CREAT | posix.O_RDWR, 0o644)
        second = posix.dup(first)
        try:
            retries = 0
            while second != first + 1:
                posix.close(first)
                retries += 1
                if retries > 10:
                    self.skipTest("couldn't allocate two consecutive fds")
                first, second = second, posix.dup(second)
        finally:
            posix.close(second)
        posix.closerange(first, first + 2)
        with self.assert_errno(errno.EBADF):
            posix.write(first, b"a")

    def test_replace(self) -> None:
        """test_os.FileTests.test_replace: replace overwrites the target"""
        self.create_file("n0", b"1")
        self.create_file("n1", b"2")
        posix.replace("n0", "n1")
        with self.assert_errno(errno.ENOENT):
            posix.stat("n0")
        self.assertEqual(b"1", self.read_bytes("n1"))

    def test_chdir(self) -> None:
        """test_posix.PosixTester.test_chdir: chdir to '.' works, to a file does not"""
        self.create_file("n0")
        posix.chdir(".")
        with self.assert_errno(errno.ENOTDIR):
            posix.chdir("n0")

    def test_oscloexec(self) -> None:
        """test_posix.PosixTester.test_oscloexec: O_CLOEXEC makes a non-inheritable fd"""
        self.create_file("n0")
        with self.open_fd("n0", posix.O_RDONLY | posix.O_CLOEXEC) as fd:
            self.assertFalse(posix.get_inheritable(fd))

    def test_get_set_inheritable_o_path(self) -> None:
        """test_os.FDInheritanceTests.test_get_set_inheritable_o_path: O_PATH fd flags"""
        self.create_file("n0")
        with self.open_fd("n0", posix.O_PATH) as fd:
            self.assertFalse(posix.get_inheritable(fd))
            posix.set_inheritable(fd, True)
            self.assertTrue(posix.get_inheritable(fd))
            posix.set_inheritable(fd, False)
            self.assertFalse(posix.get_inheritable(fd))

    def _test_all_chown_common(
        self,
        chown_func: Callable[..., None],
        first_param: str | int,
        stat_func: Callable[..., posix.stat_result],
    ) -> None:
        """``PosixTester._test_all_chown_common`` for an unprivileged user."""

        def check_stat(uid: int, gid: int) -> None:
            st = stat_func(first_param)
            self.assertEqual(uid, st.st_uid)
            self.assertEqual(gid, st.st_gid)

        uid = posix.getuid()
        gid = posix.getgid()
        chown_func(first_param, uid, gid)
        check_stat(uid, gid)
        chown_func(first_param, -1, gid)
        check_stat(uid, gid)
        chown_func(first_param, uid, -1)
        check_stat(uid, gid)
        # needs root: the large uid/gid part runs only as root
        if self.use_nfs():
            # EdenFS by design: the NFS server does not check who may chown;
            # an unprivileged user can give a file to root.
            chown_func(first_param, 0, 0)
            check_stat(0, 0)
            chown_func(first_param, uid, gid)
            check_stat(uid, gid)
            return
        with self.assert_errno(errno.EPERM):
            chown_func(first_param, 0, 0)
        check_stat(uid, gid)
        with self.assert_errno(errno.EPERM):
            chown_func(first_param, 0, -1)
        check_stat(uid, gid)
        if 0 not in posix.getgroups():
            with self.assert_errno(errno.EPERM):
                chown_func(first_param, -1, 0)
            check_stat(uid, gid)

    def test_chown(self) -> None:
        """test_posix.PosixTester.test_chown: chown of a missing and of an own file"""
        with self.assert_errno(errno.ENOENT):
            posix.chown("n0", -1, -1)
        self.create_file("n0")
        self._test_all_chown_common(posix.chown, "n0", posix.stat)

    def test_fchown(self) -> None:
        """test_posix.PosixTester.test_fchown: fchown through a descriptor"""
        with self.open_fd("n0", posix.O_WRONLY | posix.O_CREAT | posix.O_TRUNC) as fd:
            self._test_all_chown_common(posix.fchown, fd, posix.fstat)

    def test_lchown(self) -> None:
        """test_posix.PosixTester.test_lchown: lchown of a symlink"""
        posix.symlink("dummy", "n0")
        self._test_all_chown_common(posix.lchown, "n0", posix.lstat)

    def check_chmod(
        self, chmod_func: Callable[..., None], target: str, **kwargs: bool
    ) -> None:
        """``PosixTester.check_chmod``: clear and set the write bits."""
        mode = posix.stat(target).st_mode
        try:
            new_mode = mode & ~(stat.S_IWOTH | stat.S_IWGRP | stat.S_IWUSR)
            chmod_func(target, new_mode, **kwargs)
            self.assertEqual(new_mode, posix.stat(target).st_mode)
            if stat.S_ISREG(mode):
                with self.assert_errno(errno.EACCES):
                    posix.open(target, posix.O_RDWR)
            new_mode = mode | (stat.S_IWOTH | stat.S_IWGRP | stat.S_IWUSR)
            chmod_func(target, new_mode, **kwargs)
            self.assertEqual(new_mode, posix.stat(target).st_mode)
            if stat.S_ISREG(mode):
                posix.close(posix.open(target, posix.O_RDWR))
        finally:
            posix.chmod(target, mode)

    def test_chmod_file(self) -> None:
        """test_posix.PosixTester.test_chmod_file: chmod of a regular file"""
        self.create_file("n0")
        self.check_chmod(posix.chmod, "n0")

    def test_chmod_dir(self) -> None:
        """test_posix.PosixTester.test_chmod_dir: chmod of a directory"""
        posix.mkdir("n0", 0o777)
        self.check_chmod(posix.chmod, "n0")

    def check_chmod_link(
        self, chmod_func: Callable[..., None], target: str, link: str, **kwargs: bool
    ) -> None:
        """``PosixTester.check_chmod_link``: chmod through a symlink changes the target."""
        target_mode = posix.stat(target).st_mode
        link_mode = posix.lstat(link).st_mode
        try:
            new_mode = target_mode & ~(stat.S_IWOTH | stat.S_IWGRP | stat.S_IWUSR)
            chmod_func(link, new_mode, **kwargs)
            self.assertEqual(new_mode, posix.stat(target).st_mode)
            self.assertEqual(link_mode, posix.lstat(link).st_mode)
            new_mode = target_mode | (stat.S_IWOTH | stat.S_IWGRP | stat.S_IWUSR)
            chmod_func(link, new_mode, **kwargs)
            self.assertEqual(new_mode, posix.stat(target).st_mode)
            self.assertEqual(link_mode, posix.lstat(link).st_mode)
        finally:
            posix.chmod(target, target_mode)

    def test_chmod_file_symlink(self) -> None:
        """test_posix.PosixTester.test_chmod_file_symlink: chmod follows a file symlink"""
        self.create_file("n0")
        posix.symlink("n0", "n0-link")
        self.check_chmod_link(posix.chmod, "n0", "n0-link")
        self.check_chmod_link(posix.chmod, "n0", "n0-link", follow_symlinks=True)

    def test_chmod_dir_symlink(self) -> None:
        """test_posix.PosixTester.test_chmod_dir_symlink: chmod follows a dir symlink"""
        posix.mkdir("n0", 0o777)
        posix.symlink("n0", "n0-link", target_is_directory=True)
        self.check_chmod_link(posix.chmod, "n0", "n0-link")
        self.check_chmod_link(posix.chmod, "n0", "n0-link", follow_symlinks=True)

    def test_utime(self) -> None:
        """test_posix.PosixTester.test_utime: utime with None, ints and floats"""
        self.create_file("n0")
        now = time.time()
        posix.utime("n0", None)
        posix.utime("n0", (int(now), int(now)))
        self.assertEqual(int(now), posix.stat("n0").st_mtime)
        posix.utime("n0", (now, now))
        self.assertAlmostEqual(now, posix.stat("n0").st_mtime, delta=1e-6)

    def test_utime_with_fd(self) -> None:
        """test_posix.PosixTester.test_utime_with_fd: futimens through a descriptor"""
        self.create_file("n0")
        now = time.time()
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            posix.utime(fd)
            posix.utime(fd, None)
            posix.utime(fd, (int(now), int(now)))
            posix.utime(fd, (now, now))
            posix.utime(fd, (int(now), int((now - int(now)) * 1e9)))
            self.assertEqual(int(now), posix.fstat(fd).st_atime)
            posix.utime(fd, ns=(int(now), int((now - int(now)) * 1e9)))
            st = posix.fstat(fd)
            self.assertEqual(int(now), st.st_atime_ns)
            if self.use_nfs():
                # EdenFS by design (NFS): a time within the first second of
                # 1970 is stored as 0.
                self.assertEqual(0, st.st_mtime_ns)
            else:
                self.assertEqual(int((now - int(now)) * 1e9), st.st_mtime_ns)

    def test_utime_nofollow_symlinks(self) -> None:
        """test_posix.PosixTester.test_utime_nofollow_symlinks: utimensat without following"""
        self.create_file("n0")
        now = time.time()
        posix.utime("n0", None, follow_symlinks=False)
        posix.utime("n0", (int(now), int(now)), follow_symlinks=False)
        posix.utime("n0", (now, now), follow_symlinks=False)
        posix.utime("n0", follow_symlinks=False)
        # The kernel fills UTIME_NOW from its coarse clock, which can be a
        # few milliseconds behind time.time().
        self.assertAlmostEqual(now, posix.lstat("n0").st_mtime, delta=0.050)

    def test_fchmod(self) -> None:
        """EdenFS addition (no CPython test): fchmod and fstat through a descriptor"""
        self.create_file("n0")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            posix.chmod(fd, 0o600)
            self.assertEqual(0o600, stat.S_IMODE(posix.fstat(fd).st_mode))
            self.assertEqual(0o600, stat.S_IMODE(posix.stat("n0").st_mode))

    def test_fd_after_rename(self) -> None:
        """EdenFS addition (no CPython test): a descriptor follows the inode across a rename"""
        self.create_file("n0", b"abc")
        with self.open_fd("n0", posix.O_RDWR) as fd:
            ino = posix.fstat(fd).st_ino
            posix.rename("n0", "n1")
            posix.chmod(fd, 0o600)
            posix.utime(fd, ns=(10**9, 2 * 10**9))
            posix.write(fd, b"def")
            st = posix.fstat(fd)
            self.assertEqual(ino, st.st_ino)
            self.assertEqual(0o600, stat.S_IMODE(st.st_mode))
            self.assertEqual(10**9, st.st_atime_ns)
            self.assertEqual(posix.stat("n1"), st)
        self.assertEqual(b"def", self.read_bytes("n1"))

    def test_open_o_directory(self) -> None:
        """EdenFS addition (no CPython test): O_DIRECTORY opens directories only"""
        self.create_file("n0")
        posix.mkdir("n1", 0o755)
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0", posix.O_RDONLY | posix.O_DIRECTORY)
        with self.open_fd("n1", posix.O_RDONLY | posix.O_DIRECTORY) as fd:
            self.assertTrue(stat.S_ISDIR(posix.fstat(fd).st_mode))

    def test_open_o_nofollow(self) -> None:
        """EdenFS addition (no CPython test): O_NOFOLLOW is ELOOP; with O_PATH it opens the link"""
        self.create_file("n0")
        posix.symlink("n0", "n1")
        with self.assert_errno(errno.ELOOP):
            posix.open("n1", posix.O_RDONLY | posix.O_NOFOLLOW)
        with self.open_fd("n1", posix.O_PATH | posix.O_NOFOLLOW) as fd:
            self.assertTrue(stat.S_ISLNK(posix.fstat(fd).st_mode))
            with self.assert_errno(errno.EBADF):
                posix.read(fd, 1)

    def test_open_o_tmpfile(self) -> None:
        """EdenFS addition (no CPython test): O_TMPFILE

        EdenFS by design: O_TMPFILE fails with EOPNOTSUPP (the Linux-only
        anonymous file; FUSE has no operation for it, and the NFS client has
        no support either).
        """
        with self.assert_errno(errno.EOPNOTSUPP):
            posix.open(".", posix.O_TMPFILE | posix.O_RDWR, 0o600)

    def test_open_o_direct(self) -> None:
        """EdenFS addition (no CPython test): O_DIRECT reads back an aligned write"""
        buf = mmap.mmap(-1, 4096)
        self.addCleanup(buf.close)
        buf[:] = b"d" * 4096
        out = mmap.mmap(-1, 4096)
        self.addCleanup(out.close)
        with self.open_fd("n0", posix.O_CREAT | posix.O_RDWR | posix.O_DIRECT) as fd:
            self.assertEqual(4096, posix.write(fd, buf))
            posix.lseek(fd, 0, SEEK_SET)
            self.assertEqual(4096, posix.readv(fd, [out]))
        self.assertEqual(b"d" * 4096, out[:])
        self.assertEqual(b"d" * 4096, self.read_bytes("n0"))


@testcase.eden_nfs_repo_test
class UtimeTest(PosixTestBase):
    """``test_os.UtimeTests``: ``n0`` is the directory, ``n0/f1`` the file."""

    ATIME_NS: int = 1002003000
    MTIME_NS: int = 4005006000

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        posix.mkdir("n0", 0o777)
        self.create_file("n0/f1")

    def _test_utime(
        self, set_time: Callable[[str, tuple[int, int]], None], filename: str = "n0/f1"
    ) -> None:
        """``UtimeTests._test_utime`` for a file system with nanoseconds."""
        set_time(filename, (self.ATIME_NS, self.MTIME_NS))
        st = posix.stat(filename)
        self.assertAlmostEqual(self.ATIME_NS * 1e-9, st.st_atime, delta=1e-6)
        self.assertAlmostEqual(self.MTIME_NS * 1e-9, st.st_mtime, delta=1e-6)
        self.assertEqual(self.ATIME_NS, st.st_atime_ns)
        self.assertEqual(self.MTIME_NS, st.st_mtime_ns)

    @staticmethod
    def ns_to_sec(ns: int) -> float:
        return (ns * 1e-9) + 0.5e-9

    def test_utime(self) -> None:
        """test_os.UtimeTests.test_utime: the ns keyword"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            posix.utime(filename, ns=ns)

        self._test_utime(set_time)

    def test_utime_by_indexed(self) -> None:
        """test_os.UtimeTests.test_utime_by_indexed: float seconds as the second argument"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            atime_ns, mtime_ns = ns
            posix.utime(filename, (self.ns_to_sec(atime_ns), self.ns_to_sec(mtime_ns)))

        self._test_utime(set_time)

    def test_utime_by_times(self) -> None:
        """test_os.UtimeTests.test_utime_by_times: the times keyword"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            atime_ns, mtime_ns = ns
            posix.utime(
                filename, times=(self.ns_to_sec(atime_ns), self.ns_to_sec(mtime_ns))
            )

        self._test_utime(set_time)

    def test_utime_nofollow_symlinks(self) -> None:
        """test_os.UtimeTests.test_utime_nofollow_symlinks: follow_symlinks=False"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            posix.utime(filename, ns=ns, follow_symlinks=False)

        self._test_utime(set_time)

    def test_utime_fd(self) -> None:
        """test_os.UtimeTests.test_utime_fd: futimens"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            with self.open_fd(filename, posix.O_WRONLY) as fd:
                posix.utime(fd, ns=ns)

        self._test_utime(set_time)

    def test_utime_dir_fd(self) -> None:
        """test_os.UtimeTests.test_utime_dir_fd: utimensat with dir_fd"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            dirname, name = filename.rsplit("/", 1)
            with self.open_fd(dirname, posix.O_RDONLY) as dir_fd:
                posix.utime(name, dir_fd=dir_fd, ns=ns)

        self._test_utime(set_time)

    def test_utime_directory(self) -> None:
        """test_os.UtimeTests.test_utime_directory: utime on a directory"""

        def set_time(filename: str, ns: tuple[int, int]) -> None:
            posix.utime(filename, ns=ns)

        self._test_utime(set_time, filename="n0")

    def _test_utime_current(self, set_time: Callable[[], None]) -> None:
        current = time.time()
        set_time()
        st = posix.stat("n0/f1")
        self.assertAlmostEqual(current, st.st_mtime, delta=0.050)

    def test_utime_current(self) -> None:
        """test_os.UtimeTests.test_utime_current: utime without times"""
        self._test_utime_current(lambda: posix.utime("n0/f1"))

    def test_utime_current_old(self) -> None:
        """test_os.UtimeTests.test_utime_current_old: utime with None"""
        self._test_utime_current(lambda: posix.utime("n0/f1", None))

    def test_large_time(self) -> None:
        """test_os.UtimeTests.test_large_time: a timestamp in 2128

        CPython runs this only on NTFS. EdenFS by design: over NFS the
        server stores 32-bit seconds and clamps larger values to 2^32 - 1.
        """
        large = 5000000000
        posix.utime("n0/f1", (large, large))
        if self.use_nfs():
            self.assertEqual((1 << 32) - 1, posix.stat("n0/f1").st_mtime)
        else:
            self.assertEqual(large, posix.stat("n0/f1").st_mtime)
