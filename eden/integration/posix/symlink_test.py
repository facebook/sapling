#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""symlink(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/symlink/*.t`` script, named after it, calling the ``posix`` module
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
class SymlinkTest(PosixTestBase):
    def test_symlink_00_creates_symbolic_links(self) -> None:
        """symlink/00.t: symlink creates symbolic links"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        st = posix.lstat("n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        posix.symlink("n0", "n1")
        self.assertTrue(stat.S_ISLNK(posix.lstat("n1").st_mode))
        st = posix.stat("n1")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        posix.unlink("n0")
        with self.assert_errno(errno.ENOENT):
            posix.stat("n1")
        posix.unlink("n1")
        posix.mkdir("n0", 0o755)
        time0 = posix.stat("n0").st_ctime_ns
        time.sleep(1)
        posix.symlink("test", "n0/n1")
        self.assertLess(time0, posix.stat("n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n0").st_ctime_ns)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_symlink_01_enotdir_component_name_prefix(self) -> None:
        """symlink/01.t: symlink returns ENOTDIR if a component of the name2 path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.symlink("test", "n0/n1/test")
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_symlink_02_enametoolong_name_max(self) -> None:
        """symlink/02.t: symlink returns ENAMETOOLONG if a component of the name2 pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        nxx = nx + "x"
        posix.symlink(nx, "n0")
        posix.unlink("n0")
        posix.symlink("n0", nx)
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.symlink("n0", nxx)
        posix.symlink(nxx, "n0")
        posix.unlink("n0")

    def test_symlink_03_enametoolong_path_max(self) -> None:
        """symlink/03.t: symlink returns ENAMETOOLONG if an entire length of either path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.symlink(nx, "n0")
        posix.unlink("n0")
        posix.symlink("n0", nx)
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.symlink("n0", nxx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.symlink(nxx, "n0")
        self.rm_rf(nx.partition("/")[0])

    def test_symlink_04_enoent_component_name_prefix(self) -> None:
        """symlink/04.t: symlink returns ENOENT if a component of the name2 path prefix does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.symlink("test", "n0/n1/test")
        posix.rmdir("n0")

    def test_symlink_07_eloop_too_many_symbolic(self) -> None:
        """symlink/07.t: symlink returns ELOOP if too many symbolic links were encountered in translating the name2 path name"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.symlink("test", "n0/test")
        with self.assert_errno(errno.ELOOP):
            posix.symlink("test", "n1/test")
        posix.unlink("n0")
        posix.unlink("n1")

    def test_symlink_08_eexist_name_already(self) -> None:
        """symlink/08.t: symlink returns EEXIST if the name2 argument already exists"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST):
            posix.symlink("test", "n0")
        posix.unlink("n0")
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EEXIST):
            posix.symlink("test", "n0")
        posix.rmdir("n0")
        posix.mkfifo("n0", 0o644)
        with self.assert_errno(errno.EEXIST):
            posix.symlink("test", "n0")
        posix.unlink("n0")
        # cases 10-12: needs root (block device nodes)
        # cases 13-15: needs root (char device nodes)
        self.bind_unix_socket("n0")
        with self.assert_errno(errno.EEXIST):
            posix.symlink("test", "n0")
        posix.unlink("n0")
        posix.symlink("test", "n0")
        with self.assert_errno(errno.EEXIST):
            posix.symlink("test", "n0")
        posix.unlink("n0")
