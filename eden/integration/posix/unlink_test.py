#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""unlink(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/unlink/*.t`` script, named after it, calling the ``posix`` module
directly. Comments such as ``# case 17: needs root`` stand for the script's
TAP cases that are left out; ``README.md`` in this directory explains the
three reasons and lists the cases.
"""

from __future__ import annotations

import errno
import posix
import stat
import time

from eden.integration.lib import testcase

from .base import PosixTestBase


@testcase.eden_nfs_repo_test
class UnlinkTest(PosixTestBase):
    def test_unlink_00_removes_regular_symbolic_links(self) -> None:
        """unlink/00.t: unlink removes regular files, symbolic links, fifos and sockets"""
        posix.mkdir("n2", 0o755)
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        self.assertTrue(stat.S_ISREG(posix.lstat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n2/n0")
        posix.symlink("n1", "n2/n0")
        self.assertTrue(stat.S_ISLNK(posix.lstat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n2/n0")
        posix.mkfifo("n2/n0", 0o644)
        self.assertTrue(stat.S_ISFIFO(posix.lstat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n2/n0")
        # case 14: needs root
        # cases 15-16: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n2/n0")
        # case 18: needs root
        # cases 19-20: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n2/n0")
        self.bind_unix_socket("n2/n0")
        self.assertTrue(stat.S_ISSOCK(posix.lstat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 27: no hard links on EdenFS
        # cases 28-29: depends on a step above
        posix.unlink("n2/n0")
        posix.mkfifo("n2/n0", 0o644)
        # case 32: no hard links on EdenFS
        # cases 33-34: depends on a step above
        posix.unlink("n2/n0")
        # case 36: needs root
        # case 37: no hard links on EdenFS
        # cases 38-40: depends on a step above
        # case 41: needs root
        # case 42: no hard links on EdenFS
        # cases 43-45: depends on a step above
        self.bind_unix_socket("n2/n0")
        # case 47: no hard links on EdenFS
        # cases 48-49: depends on a step above
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 52: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        posix.mkfifo("n2/n0", 0o644)
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 56: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        # cases 59-60: needs root
        # cases 61-62: depends on a step above
        # cases 63-64: needs root
        # cases 65-66: depends on a step above
        self.bind_unix_socket("n2/n0")
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 68: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        posix.close(posix.open("n2/n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        time0 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.unlink("n2/n0/n1")
        self.assertLess(time0, posix.stat("n2/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n2/n0").st_ctime_ns)
        posix.rmdir("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        posix.mkfifo("n2/n0/n1", 0o644)
        time0 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.unlink("n2/n0/n1")
        self.assertLess(time0, posix.stat("n2/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n2/n0").st_ctime_ns)
        posix.rmdir("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        # case 84: needs root
        # cases 85-87: depends on a step above
        posix.rmdir("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        # case 90: needs root
        # cases 91-93: depends on a step above
        posix.rmdir("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        self.bind_unix_socket("n2/n0/n1")
        time0 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.unlink("n2/n0/n1")
        self.assertLess(time0, posix.stat("n2/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n2/n0").st_ctime_ns)
        posix.rmdir("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        posix.symlink("test", "n2/n0/n1")
        time0 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.unlink("n2/n0/n1")
        self.assertLess(time0, posix.stat("n2/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n2/n0").st_ctime_ns)
        posix.rmdir("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 108: no hard links on EdenFS
        # cases 109-110: depends on a step above
        posix.unlink("n2/n0")
        posix.rmdir("n2")

    def test_unlink_01_enotdir_component_prefix_directory(self) -> None:
        """unlink/01.t: unlink returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.unlink("n0/n1/test")
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_unlink_02_enametoolong_name_max(self) -> None:
        """unlink/02.t: unlink returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.unlink(nx)
        with self.assert_errno(errno.ENOENT):
            posix.unlink(nx)
        # case 4: EdenFS bug: too-long name returns ENOENT

    def test_unlink_03_enametoolong_path_max(self) -> None:
        """unlink/03.t: unlink returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.unlink(nx)
        with self.assert_errno(errno.ENOENT):
            posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.unlink(nxx)
        self.rm_rf(nx.partition("/")[0])

    def test_unlink_04_enoent(self) -> None:
        """unlink/04.t: unlink returns ENOENT if the named file does not exist"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.unlink("n0")
        with self.assert_errno(errno.ENOENT):
            posix.unlink("n0")
        with self.assert_errno(errno.ENOENT):
            posix.unlink("n1")

    def test_unlink_07_eloop_too_many_symbolic(self) -> None:
        """unlink/07.t: unlink returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.unlink("n0/test")
        with self.assert_errno(errno.ELOOP):
            posix.unlink("n1/test")
        posix.unlink("n0")
        posix.unlink("n1")

    def test_unlink_14_open_file_not_freed(self) -> None:
        """unlink/14.t: An open file will not be immediately freed by unlink"""
        posix.mkdir("n2", 0o755)
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        fd = posix.open("n2/n0", posix.O_WRONLY)
        try:
            posix.write(fd, b"Hello, World!")
        finally:
            posix.close(fd)
        # case 4: EdenFS bug: nlink stays 1 after unlink
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        fd = posix.open("n2/n0", posix.O_RDWR)
        try:
            posix.write(fd, b"Hello,_World!")
            posix.unlink("n2/n0")
            self.assertEqual(b"Hello,_World!", posix.pread(fd, 13, 0))
        finally:
            posix.close(fd)
        posix.rmdir("n2")
