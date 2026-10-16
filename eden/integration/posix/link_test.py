#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""link(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/link/*.t`` script, named after it, calling the ``posix`` module
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
class LinkTest(PosixTestBase):
    def test_link_00_creates_hardlinks(self) -> None:
        """link/00.t: link creates hardlinks"""
        posix.mkdir("n3", 0o755)
        posix.close(posix.open("n3/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        # case 4: no hard links on EdenFS
        # cases 5-6: depends on a step above
        # case 7: no hard links on EdenFS
        # cases 8-11: depends on a step above
        # case 12: needs root
        # cases 13-15: depends on a step above
        posix.unlink("n3/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        # cases 18-20: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        # case 22: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n2")
        # case 24: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n2")
        posix.mkfifo("n3/n0", 0o644)
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        # case 30: no hard links on EdenFS
        # cases 31-32: depends on a step above
        # case 33: no hard links on EdenFS
        # cases 34-37: depends on a step above
        # case 38: needs root
        # cases 39-41: depends on a step above
        posix.unlink("n3/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        # cases 44-46: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        # case 48: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n2")
        # case 50: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n2")
        # cases 54-79: needs root (block device nodes)
        # cases 80-105: needs root (char device nodes)
        self.bind_unix_socket("n3/n0")
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISSOCK(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        # case 108: no hard links on EdenFS
        # cases 109-110: depends on a step above
        # case 111: no hard links on EdenFS
        # cases 112-115: depends on a step above
        # case 116: needs root
        # cases 117-119: depends on a step above
        posix.unlink("n3/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        # cases 122-124: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        # case 126: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n2")
        # case 128: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n2")
        posix.close(posix.open("n3/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 133: no hard links on EdenFS
        # cases 134-136: depends on a step above
        posix.unlink("n3/n0")
        # case 138: depends on a step above
        posix.mkfifo("n3/n0", 0o644)
        # case 140: no hard links on EdenFS
        # cases 141-143: depends on a step above
        posix.unlink("n3/n0")
        # case 145: depends on a step above
        # cases 146-152: needs root (block device nodes)
        # cases 153-159: needs root (char device nodes)
        self.bind_unix_socket("n3/n0")
        # case 161: no hard links on EdenFS
        # cases 162-164: depends on a step above
        posix.unlink("n3/n0")
        # case 166: depends on a step above
        posix.close(posix.open("n3/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 168: needs root
        ctime1 = posix.stat("n3/n0").st_ctime_ns
        dctime1 = posix.stat("n3").st_ctime_ns
        time.sleep(1)
        # case 169: needs root
        self.assertEqual(ctime1, posix.stat("n3/n0").st_ctime_ns)
        self.assertEqual(dctime1, posix.stat("n3").st_ctime_ns)
        self.assertEqual(dctime1, posix.stat("n3").st_mtime_ns)
        posix.unlink("n3/n0")
        posix.mkfifo("n3/n0", 0o644)
        # case 175: needs root
        ctime1 = posix.stat("n3/n0").st_ctime_ns
        dctime1 = posix.stat("n3").st_ctime_ns
        time.sleep(1)
        # case 176: needs root
        self.assertEqual(ctime1, posix.stat("n3/n0").st_ctime_ns)
        self.assertEqual(dctime1, posix.stat("n3").st_ctime_ns)
        self.assertEqual(dctime1, posix.stat("n3").st_mtime_ns)
        posix.unlink("n3/n0")
        # cases 181-187: needs root (block device nodes)
        # cases 188-194: needs root (char device nodes)
        self.bind_unix_socket("n3/n0")
        # case 196: needs root
        ctime1 = posix.stat("n3/n0").st_ctime_ns
        dctime1 = posix.stat("n3").st_ctime_ns
        time.sleep(1)
        # case 197: needs root
        self.assertEqual(ctime1, posix.stat("n3/n0").st_ctime_ns)
        self.assertEqual(dctime1, posix.stat("n3").st_ctime_ns)
        self.assertEqual(dctime1, posix.stat("n3").st_mtime_ns)
        posix.unlink("n3/n0")
        posix.rmdir("n3")

    def test_link_01_enotdir_component_either_prefix(self) -> None:
        """link/01.t: link returns ENOTDIR if a component of either path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n1/test", "n0/n2", follow_symlinks=False)
        posix.close(posix.open("n0/n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n0/n1")
        posix.unlink("n0/n2")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n1/test", "n0/n2", follow_symlinks=False)
        posix.mkfifo("n0/n2", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n0/n1")
        posix.unlink("n0/n2")
        # cases 14-19: needs root (block device nodes)
        # cases 20-25: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n1/test", "n0/n2", follow_symlinks=False)
        self.bind_unix_socket("n0/n2")
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n0/n1")
        posix.unlink("n0/n2")
        posix.rmdir("n0")

    def test_link_02_enametoolong_name_max(self) -> None:
        """link/02.t: link returns ENAMETOOLONG if a component of either pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        nxx = nx + "x"
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 2: no hard links on EdenFS
        posix.unlink(nx)
        # case 4: no hard links on EdenFS
        # cases 5-6: depends on a step above
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.link("n0", nxx, follow_symlinks=False)
        posix.unlink("n0")
        # case 10: EdenFS bug: too-long name returns ENOENT

    def test_link_03_enametoolong_path_max(self) -> None:
        """link/03.t: link returns ENAMETOOLONG if an entire length of either path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 2: no hard links on EdenFS
        # cases 3-4: depends on a step above
        posix.unlink(nx)
        # case 6: no hard links on EdenFS
        # cases 7-9: depends on a step above
        # case 10: EdenFS bug: too-long name returns ENOENT
        # cases 11-12: depends on a step above
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.link(nxx, "n0", follow_symlinks=False)
        self.rm_rf(nx.partition("/")[0])

    def test_link_04_enoent_component_either_prefix(self) -> None:
        """link/04.t: link returns ENOENT if a component of either path prefix does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.link("n0/n1/test", "n2", follow_symlinks=False)
        posix.close(posix.open("n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOENT):
            posix.link("n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n2")
        posix.rmdir("n0")

    def test_link_08_eloop_too_many_symbolic(self) -> None:
        """link/08.t: link returns ELOOP if too many symbolic links were encountered in translating one of the pathnames"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.link("n0/test", "n2", follow_symlinks=False)
        with self.assert_errno(errno.ELOOP):
            posix.link("n1/test", "n2", follow_symlinks=False)
        posix.close(posix.open("n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ELOOP):
            posix.link("n2", "n0/test", follow_symlinks=False)
        with self.assert_errno(errno.ELOOP):
            posix.link("n2", "n1/test", follow_symlinks=False)
        posix.unlink("n0")
        posix.unlink("n1")
        posix.unlink("n2")

    def test_link_09_enoent_source(self) -> None:
        """link/09.t: link returns ENOENT if the source file does not exist"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 2: no hard links on EdenFS
        posix.unlink("n0")
        # case 4: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.link("n0", "n1", follow_symlinks=False)

    def test_link_10_eexist_destination(self) -> None:
        """link/10.t: link returns EEXIST if the destination file does exist"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.close(posix.open("n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST):
            posix.link("n0", "n1", follow_symlinks=False)
        posix.unlink("n1")
        posix.mkdir("n1", 0o755)
        with self.assert_errno(errno.EEXIST):
            posix.link("n0", "n1", follow_symlinks=False)
        posix.rmdir("n1")
        posix.mkfifo("n1", 0o644)
        with self.assert_errno(errno.EEXIST):
            posix.link("n0", "n1", follow_symlinks=False)
        posix.unlink("n1")
        # cases 11-13: needs root (block device nodes)
        # cases 14-16: needs root (char device nodes)
        self.bind_unix_socket("n1")
        with self.assert_errno(errno.EEXIST):
            posix.link("n0", "n1", follow_symlinks=False)
        posix.unlink("n1")
        posix.symlink("test", "n1")
        with self.assert_errno(errno.EEXIST):
            posix.link("n0", "n1", follow_symlinks=False)
        posix.unlink("n1")
        posix.unlink("n0")

    def test_link_11_eperm_source_directory(self) -> None:
        """link/11.t: link returns EPERM if the source file is a directory"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EPERM):
            posix.link("n0", "n1", follow_symlinks=False)
        posix.rmdir("n0")
        posix.mkdir("n0", 0o755)
        # cases 5-8: needs root
        posix.rmdir("n0")
