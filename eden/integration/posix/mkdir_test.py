#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""mkdir(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/mkdir/*.t`` script, named after it, calling the ``posix`` module
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
class MkdirTest(PosixTestBase):
    def test_mkdir_00_creates_directories(self) -> None:
        """mkdir/00.t: mkdir creates directories"""
        posix.mkdir("n1", 0o755)
        posix.mkdir("n1/n0", 0o755)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o755, stat.S_IMODE(st.st_mode))
        posix.rmdir("n1/n0")
        posix.mkdir("n1/n0", 0o151)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o151, stat.S_IMODE(st.st_mode))
        posix.rmdir("n1/n0")
        posix.umask(0o077)
        posix.mkdir("n1/n0", 0o151)
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o100, stat.S_IMODE(st.st_mode))
        posix.rmdir("n1/n0")
        posix.umask(0o070)
        posix.mkdir("n1/n0", 0o345)
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o305, stat.S_IMODE(st.st_mode))
        posix.rmdir("n1/n0")
        posix.umask(0o501)
        posix.mkdir("n1/n0", 0o345)
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o244, stat.S_IMODE(st.st_mode))
        posix.rmdir("n1/n0")
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
        posix.mkdir("n1/n0", 0o755)
        self.assertLess(time0, posix.stat("n1/n0").st_atime_ns)
        self.assertLess(time0, posix.stat("n1/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n1/n0").st_ctime_ns)
        self.assertLess(time0, posix.stat("n1").st_mtime_ns)
        self.assertLess(time0, posix.stat("n1").st_ctime_ns)
        posix.rmdir("n1/n0")
        posix.rmdir("n1")

    def test_mkdir_01_enotdir_component_prefix_directory(self) -> None:
        """mkdir/01.t: mkdir returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.mkdir("n0/n1/test", 0o755)
        posix.unlink("n0/n1")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.mkdir("n0/n1/test", 0o755)
        posix.unlink("n0/n1")
        # cases 8-10: needs root (block device nodes)
        # cases 11-13: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.mkdir("n0/n1/test", 0o755)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_mkdir_02_enametoolong_name_max(self) -> None:
        """mkdir/02.t: mkdir returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        nxx = nx + "x"
        posix.mkdir(nx, 0o755)
        posix.rmdir(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mkdir(nxx, 0o755)

    def test_mkdir_03_enametoolong_path_max(self) -> None:
        """mkdir/03.t: mkdir returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.mkdir(nx, 0o755)
        posix.rmdir(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.mkdir(nxx, 0o755)
        self.rm_rf(nx.partition("/")[0])

    def test_mkdir_04_enoent_component_prefix(self) -> None:
        """mkdir/04.t: mkdir returns ENOENT if a component of the path prefix does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.mkdir("n0/n1/test", 0o755)
        posix.rmdir("n0")

    def test_mkdir_07_eloop_too_many_symbolic(self) -> None:
        """mkdir/07.t: mkdir returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.mkdir("n0/test", 0o755)
        with self.assert_errno(errno.ELOOP):
            posix.mkdir("n1/test", 0o755)
        posix.unlink("n0")
        posix.unlink("n1")

    def test_mkdir_10_eexist(self) -> None:
        """mkdir/10.t: mkdir returns EEXIST if the named file exists"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST):
            posix.mkdir("n0", 0o755)
        posix.unlink("n0")
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EEXIST):
            posix.mkdir("n0", 0o755)
        posix.rmdir("n0")
        posix.mkfifo("n0", 0o644)
        with self.assert_errno(errno.EEXIST):
            posix.mkdir("n0", 0o755)
        posix.unlink("n0")
        # cases 10-12: needs root (block device nodes)
        # cases 13-15: needs root (char device nodes)
        self.bind_unix_socket("n0")
        with self.assert_errno(errno.EEXIST):
            posix.mkdir("n0", 0o755)
        posix.unlink("n0")
        posix.symlink("test", "n0")
        with self.assert_errno(errno.EEXIST):
            posix.mkdir("n0", 0o755)
        posix.unlink("n0")
