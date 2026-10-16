#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""mknod(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/mknod/*.t`` script, named after it, calling the ``posix`` module
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
class MknodTest(PosixTestBase):
    def test_mknod_00_creates_fifo(self) -> None:
        """mknod/00.t: mknod creates fifo files"""
        posix.mkdir("n1", 0o755)
        posix.mknod("n1/n0", stat.S_IFIFO | 0o755)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o755, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.mknod("n1/n0", stat.S_IFIFO | 0o151)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o151, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.umask(0o077)
        posix.mknod("n1/n0", stat.S_IFIFO | 0o151)
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o100, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.umask(0o070)
        posix.mknod("n1/n0", stat.S_IFIFO | 0o345)
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o305, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.umask(0o501)
        posix.mknod("n1/n0", stat.S_IFIFO | 0o345)
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o244, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        # cases 17-18: needs root
        # cases 19-20: depends on a step above
        # case 21: needs root
        # cases 22-23: depends on a step above
        posix.chmod("n1", 0o777)
        # case 25: needs root
        # cases 26-27: depends on a step above
        # case 28: needs root
        time0 = posix.stat("n1").st_ctime_ns
        time.sleep(1)
        posix.mknod("n1/n0", stat.S_IFIFO | 0o755)
        self.assertLess(time0, posix.stat("n1/n0").st_atime_ns)
        self.assertLess(time0, posix.stat("n1/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n1/n0").st_ctime_ns)
        self.assertLess(time0, posix.stat("n1").st_mtime_ns)
        self.assertLess(time0, posix.stat("n1").st_ctime_ns)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_mknod_01_enotdir_component_prefix_directory(self) -> None:
        """mknod/01.t: mknod returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFBLK | 0o644, posix.makedev(1, 2))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFCHR | 0o644, posix.makedev(1, 2))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFIFO | 0o644)
        posix.unlink("n0/n1")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFBLK | 0o644, posix.makedev(1, 2))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFCHR | 0o644, posix.makedev(1, 2))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFIFO | 0o644)
        posix.unlink("n0/n1")
        # cases 12-16: needs root (block device nodes)
        # cases 17-21: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFBLK | 0o644, posix.makedev(1, 2))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFCHR | 0o644, posix.makedev(1, 2))
        with self.assert_errno(errno.ENOTDIR):
            posix.mknod("n0/n1/test", stat.S_IFIFO | 0o644)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_mknod_02_enametoolong_name_max(self) -> None:
        """mknod/02.t: mknod returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        nxx = nx + "x"
        posix.mknod(nx, stat.S_IFIFO | 0o644)
        st = posix.stat(nx)
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mknod(nxx, stat.S_IFIFO | 0o644)
        # case 5: needs root
        # cases 6-7: depends on a step above
        # cases 8-9: needs root
        # cases 10-11: depends on a step above
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mknod(nxx, stat.S_IFCHR | 0o644, posix.makedev(0, 0))

    def test_mknod_03_enametoolong_path_max(self) -> None:
        """mknod/03.t: mknod returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.mknod(nx, stat.S_IFIFO | 0o644)
        self.assertTrue(stat.S_ISFIFO(posix.stat(nx).st_mode))
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mknod(nxx, stat.S_IFIFO | 0o644)
        # case 5: needs root
        # cases 6-7: depends on a step above
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mknod(nxx, stat.S_IFBLK | 0o644, posix.makedev(1, 2))
        # case 9: needs root
        # cases 10-11: depends on a step above
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mknod(nxx, stat.S_IFCHR | 0o644, posix.makedev(1, 2))
        self.rm_rf(nx.partition("/")[0])

    def test_mknod_04_enoent_component_prefix(self) -> None:
        """mknod/04.t: mknod returns ENOENT if a component of the path prefix does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.mknod("n0/n1/test", stat.S_IFIFO | 0o644)
        posix.rmdir("n0")

    def test_mknod_07_eloop_too_many_symbolic(self) -> None:
        """mknod/07.t: mknod returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.mknod("n0/test", stat.S_IFIFO | 0o644)
        with self.assert_errno(errno.ELOOP):
            posix.mknod("n1/test", stat.S_IFIFO | 0o644)
        posix.unlink("n0")
        posix.unlink("n1")

    def test_mknod_08_eexist(self) -> None:
        """mknod/08.t: mknod returns EEXIST if the named file exists"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFBLK | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFCHR | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFIFO | 0o644)
        posix.unlink("n0")
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFBLK | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFCHR | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFIFO | 0o644)
        posix.rmdir("n0")
        posix.mkfifo("n0", 0o644)
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFBLK | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFCHR | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFIFO | 0o644)
        posix.unlink("n0")
        # cases 16-20: needs root (block device nodes)
        # cases 21-25: needs root (char device nodes)
        self.bind_unix_socket("n0")
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFBLK | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFCHR | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFIFO | 0o644)
        posix.unlink("n0")
        posix.symlink("test", "n0")
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFBLK | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFCHR | 0o644, posix.makedev(0, 0))
        with self.assert_errno(errno.EEXIST):
            posix.mknod("n0", stat.S_IFIFO | 0o644)
        posix.unlink("n0")
