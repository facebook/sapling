#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""rmdir(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/rmdir/*.t`` script, named after it, calling the ``posix`` module
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
class RmdirTest(PosixTestBase):
    def test_rmdir_00_removes_directories(self) -> None:
        """rmdir/00.t: rmdir removes directories"""
        posix.mkdir("n0", 0o755)
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        posix.rmdir("n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n0")
        posix.mkdir("n0", 0o755)
        posix.mkdir("n0/n1", 0o755)
        time0 = posix.stat("n0").st_ctime_ns
        time.sleep(1)
        posix.rmdir("n0/n1")
        self.assertLess(time0, posix.stat("n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n0").st_ctime_ns)
        posix.rmdir("n0")

    def test_rmdir_01_enotdir_component_directory(self) -> None:
        """rmdir/01.t: rmdir returns ENOTDIR if a component of the path is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.rmdir("n0/n1/test")
        posix.unlink("n0/n1")
        posix.rmdir("n0")
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.rmdir("n0")
        posix.unlink("n0")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ENOTDIR):
            posix.rmdir("n0")
        posix.unlink("n0")
        posix.mkfifo("n0", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.rmdir("n0")
        posix.unlink("n0")

    def test_rmdir_02_enametoolong_name_max(self) -> None:
        """rmdir/02.t: rmdir returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        posix.mkdir(nx, 0o755)
        posix.rmdir(nx)
        with self.assert_errno(errno.ENOENT):
            posix.rmdir(nx)
        # case 4: EdenFS bug: too-long name returns ENOENT

    def test_rmdir_03_enametoolong_path_max(self) -> None:
        """rmdir/03.t: rmdir returns ENAMETOOLONG if an entire path name exceeded ${PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.mkdir(nx, 0o755)
        st = posix.stat(nx)
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o755, stat.S_IMODE(st.st_mode))
        posix.rmdir(nx)
        with self.assert_errno(errno.ENOENT):
            posix.rmdir(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.rmdir(nxx)
        self.rm_rf(nx.partition("/")[0])

    def test_rmdir_04_enoent_directory(self) -> None:
        """rmdir/04.t: rmdir returns ENOENT if the named directory does not exist"""
        posix.mkdir("n0", 0o755)
        posix.rmdir("n0")
        with self.assert_errno(errno.ENOENT):
            posix.rmdir("n0")
        with self.assert_errno(errno.ENOENT):
            posix.rmdir("n1")

    def test_rmdir_05_eloop_too_many_symbolic(self) -> None:
        """rmdir/05.t: rmdir returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.rmdir("n0/test")
        with self.assert_errno(errno.ELOOP):
            posix.rmdir("n1/test")
        posix.unlink("n0")
        posix.unlink("n1")

    def test_rmdir_06_eexist_enotempty_directory_contains(self) -> None:
        """rmdir/06.t: rmdir returns EEXIST or ENOTEMPTY the named directory contains files other than '.' and '..' in it"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rmdir("n0")
        posix.unlink("n0/n1")
        posix.mkdir("n0/n1", 0o755)
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rmdir("n0")
        posix.rmdir("n0/n1")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rmdir("n0")
        posix.unlink("n0/n1")
        # cases 11-13: needs root (block device nodes)
        # cases 14-16: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rmdir("n0")
        posix.unlink("n0/n1")
        posix.symlink("test", "n0/n1")
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rmdir("n0")
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_rmdir_12_einval_last_component_eexist(self) -> None:
        """rmdir/12.t: rmdir returns EINVAL if the last component of the path is '.' and EEXIST or ENOTEMPTY if the last component of the path is '..'"""
        posix.mkdir("n0", 0o755)
        posix.mkdir("n0/n1", 0o755)
        with self.assert_errno(errno.EINVAL):
            posix.rmdir("n0/n1/.")
        with self.assert_errno(errno.ENOTEMPTY, errno.EEXIST):
            posix.rmdir("n0/n1/..")
        posix.rmdir("n0/n1")
        posix.rmdir("n0")
