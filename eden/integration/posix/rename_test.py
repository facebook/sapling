#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""rename(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/rename/*.t`` script, named after it, calling the ``posix`` module
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
class RenameTest(PosixTestBase):
    def test_rename_00_changes_name(self) -> None:
        """rename/00.t: rename changes file name"""
        posix.mkdir("n3", 0o755)
        posix.close(posix.open("n3/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.chmod("n3/n0", 0o644)
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        inode = posix.lstat("n3/n0").st_ino
        posix.rename("n3/n0", "n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        st = posix.lstat("n3/n1")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(inode, st.st_ino)
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        # case 8: no hard links on EdenFS
        # cases 9-10: depends on a step above
        posix.rename("n3/n1", "n3/n2")
        # case 12: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        # cases 14-15: depends on a step above
        posix.unlink("n3/n2")
        posix.mkfifo("n3/n0", 0o644)
        posix.chmod("n3/n0", 0o644)
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        inode = posix.lstat("n3/n0").st_ino
        posix.rename("n3/n0", "n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        st = posix.lstat("n3/n1")
        self.assertTrue(stat.S_ISFIFO(st.st_mode))
        self.assertEqual(inode, st.st_ino)
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        # case 23: no hard links on EdenFS
        # cases 24-25: depends on a step above
        posix.rename("n3/n1", "n3/n2")
        # case 27: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        # cases 29-30: depends on a step above
        posix.unlink("n3/n2")
        # cases 32-46: needs root (block device nodes)
        # cases 47-61: needs root (char device nodes)
        self.bind_unix_socket("n3/n0")
        posix.chmod("n3/n0", 0o644)
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISSOCK(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        inode = posix.lstat("n3/n0").st_ino
        posix.rename("n3/n0", "n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        st = posix.lstat("n3/n1")
        self.assertTrue(stat.S_ISSOCK(st.st_mode))
        self.assertEqual(inode, st.st_ino)
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        self.assertEqual(1, st.st_nlink)
        # case 68: no hard links on EdenFS
        # cases 69-70: depends on a step above
        posix.rename("n3/n1", "n3/n2")
        # case 72: depends on a step above
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        # cases 74-75: depends on a step above
        posix.unlink("n3/n2")
        posix.mkdir("n3/n0", 0o755)
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(0o755, stat.S_IMODE(st.st_mode))
        inode = posix.lstat("n3/n0").st_ino
        posix.rename("n3/n0", "n3/n1")
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n0")
        st = posix.lstat("n3/n1")
        self.assertTrue(stat.S_ISDIR(st.st_mode))
        self.assertEqual(inode, st.st_ino)
        self.assertEqual(0o755, stat.S_IMODE(st.st_mode))
        posix.rmdir("n3/n1")
        posix.close(posix.open("n3/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        rinode = posix.lstat("n3/n0").st_ino
        st = posix.lstat("n3/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        posix.symlink("n0", "n3/n1")
        sinode = posix.lstat("n3/n1").st_ino
        st = posix.stat("n3/n1")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(rinode, st.st_ino)
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        st = posix.lstat("n3/n1")
        self.assertTrue(stat.S_ISLNK(st.st_mode))
        self.assertEqual(sinode, st.st_ino)
        posix.rename("n3/n1", "n3/n2")
        st = posix.stat("n3/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(rinode, st.st_ino)
        self.assertEqual(0o644, stat.S_IMODE(st.st_mode))
        with self.assert_errno(errno.ENOENT):
            posix.lstat("n3/n1")
        st = posix.lstat("n3/n2")
        self.assertTrue(stat.S_ISLNK(st.st_mode))
        self.assertEqual(sinode, st.st_ino)
        posix.unlink("n3/n0")
        posix.unlink("n3/n2")
        posix.close(posix.open("n3/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.lstat("n3/n0").st_ctime_ns
        time.sleep(1)
        # case 95: needs root
        self.assertEqual(ctime1, posix.lstat("n3/n0").st_ctime_ns)
        posix.unlink("n3/n0")
        posix.mkdir("n3/n0", 0o755)
        ctime1 = posix.lstat("n3/n0").st_ctime_ns
        time.sleep(1)
        # case 99: needs root
        self.assertEqual(ctime1, posix.lstat("n3/n0").st_ctime_ns)
        posix.rmdir("n3/n0")
        posix.mkfifo("n3/n0", 0o644)
        ctime1 = posix.lstat("n3/n0").st_ctime_ns
        time.sleep(1)
        # case 103: needs root
        self.assertEqual(ctime1, posix.lstat("n3/n0").st_ctime_ns)
        posix.unlink("n3/n0")
        # cases 106-109: needs root (block device nodes)
        # cases 110-113: needs root (char device nodes)
        self.bind_unix_socket("n3/n0")
        ctime1 = posix.lstat("n3/n0").st_ctime_ns
        time.sleep(1)
        # case 115: needs root
        self.assertEqual(ctime1, posix.lstat("n3/n0").st_ctime_ns)
        posix.unlink("n3/n0")
        posix.symlink("test", "n3/n0")
        ctime1 = posix.lstat("n3/n0").st_ctime_ns
        time.sleep(1)
        # case 119: needs root
        self.assertEqual(ctime1, posix.lstat("n3/n0").st_ctime_ns)
        posix.unlink("n3/n0")
        posix.rmdir("n3")

    def test_rename_01_enametoolong_name_max(self) -> None:
        """rename/01.t: rename returns ENAMETOOLONG if a component of either pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        nxx = nx + "x"
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.rename(nx, "n0")
        posix.rename("n0", nx)
        posix.unlink(nx)
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.rename("n0", nxx)
        posix.unlink("n0")
        # case 8: EdenFS bug: too-long name returns ENOENT

    def test_rename_02_enametoolong_path_max(self) -> None:
        """rename/02.t: rename returns ENAMETOOLONG if an entire length of either path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.rename("n0", nx)
        posix.rename(nx, "n0")
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.rename("n0", nxx)
        posix.unlink("n0")
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.rename(nxx, "n0")
        self.rm_rf(nx.partition("/")[0])

    def test_rename_03_enoent_component_from_prefix(self) -> None:
        """rename/03.t: rename returns ENOENT if a component of the 'from' path does not exist, or a path prefix of 'to' does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.rename("n0/n1/test", "n2")
        posix.close(posix.open("n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOENT):
            posix.rename("n2", "n0/n1/test")
        posix.unlink("n2")
        posix.rmdir("n0")

    def test_rename_11_eloop_too_many_symbolic(self) -> None:
        """rename/11.t: rename returns ELOOP if too many symbolic links were encountered in translating one of the pathnames"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.rename("n0/test", "n2")
        with self.assert_errno(errno.ELOOP):
            posix.rename("n1/test", "n2")
        posix.close(posix.open("n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ELOOP):
            posix.rename("n2", "n0/test")
        with self.assert_errno(errno.ELOOP):
            posix.rename("n2", "n1/test")
        posix.unlink("n0")
        posix.unlink("n1")
        posix.unlink("n2")

    def test_rename_12_enotdir_component_either_prefix(self) -> None:
        """rename/12.t: rename returns ENOTDIR if a component of either path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0/n1/test", "n0/n2")
        posix.close(posix.open("n0/n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n0/n1")
        posix.unlink("n0/n2")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0/n1/test", "n0/n2")
        posix.mkfifo("n0/n2", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n0/n1")
        posix.unlink("n0/n2")
        # cases 14-19: needs root (block device nodes)
        # cases 20-25: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0/n1/test", "n0/n2")
        self.bind_unix_socket("n0/n2")
        with self.assert_errno(errno.ENOTDIR):
            posix.link("n0/n2", "n0/n1/test", follow_symlinks=False)
        posix.unlink("n0/n1")
        posix.unlink("n0/n2")
        posix.rmdir("n0")

    def test_rename_13_enotdir_from_directory_but(self) -> None:
        """rename/13.t: rename returns ENOTDIR when the 'from' argument is a directory, but 'to' is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0", "n1")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISREG(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        posix.mkfifo("n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0", "n1")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISFIFO(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        # cases 12-16: needs root (block device nodes)
        # cases 17-21: needs root (char device nodes)
        self.bind_unix_socket("n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0", "n1")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISSOCK(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        posix.symlink("test", "n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.rename("n0", "n1")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISLNK(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        posix.rmdir("n0")

    def test_rename_14_eisdir_directory_but_from(self) -> None:
        """rename/14.t: rename returns EISDIR when the 'to' argument is a directory, but 'from' is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EISDIR):
            posix.rename("n1", "n0")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISREG(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        posix.mkfifo("n1", 0o644)
        with self.assert_errno(errno.EISDIR):
            posix.rename("n1", "n0")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISFIFO(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        # cases 12-16: needs root (block device nodes)
        # cases 17-21: needs root (char device nodes)
        self.bind_unix_socket("n1")
        with self.assert_errno(errno.EISDIR):
            posix.rename("n1", "n0")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISSOCK(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        posix.symlink("test", "n1")
        with self.assert_errno(errno.EISDIR):
            posix.rename("n1", "n0")
        self.assertTrue(stat.S_ISDIR(posix.lstat("n0").st_mode))
        self.assertTrue(stat.S_ISLNK(posix.lstat("n1").st_mode))
        posix.unlink("n1")
        posix.rmdir("n0")

    def test_rename_18_einval_from_parent_directory(self) -> None:
        """rename/18.t: rename returns EINVAL when the 'from' argument is a parent directory of 'to'"""
        posix.mkdir("n0", 0o755)
        posix.mkdir("n0/n1", 0o755)
        with self.assert_errno(errno.EINVAL):
            posix.rename("n0", "n0/n1")
        with self.assert_errno(errno.EINVAL):
            posix.rename("n0", "n0/n1/n2")
        posix.rmdir("n0/n1")
        posix.rmdir("n0")

    def test_rename_19_einval_ebusy_attempt_made_rename(self) -> None:
        """rename/19.t: rename returns EINVAL/EBUSY when an attempt is made to rename '.' or '..'"""
        posix.mkdir("n0", 0o755)
        posix.mkdir("n0/n1", 0o755)
        with self.assert_errno(errno.EINVAL, errno.EBUSY):
            posix.rename("n0/n1/.", "n2")
        with self.assert_errno(errno.EINVAL, errno.EBUSY):
            posix.rename("n0/n1/..", "n2")
        posix.rmdir("n0/n1")
        posix.rmdir("n0")

    def test_rename_20_eexist_enotempty_directory_empty(self) -> None:
        """rename/20.t: rename returns EEXIST or ENOTEMPTY if the 'to' argument is a directory and is not empty"""
        posix.mkdir("n0", 0o755)
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n2", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rename("n0", "n1")
        posix.unlink("n1/n2")
        posix.mkdir("n1/n2", 0o755)
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rename("n0", "n1")
        posix.rmdir("n1/n2")
        posix.mkfifo("n1/n2", 0o644)
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rename("n0", "n1")
        posix.unlink("n1/n2")
        # cases 12-14: needs root (block device nodes)
        # cases 15-17: needs root (char device nodes)
        self.bind_unix_socket("n1/n2")
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rename("n0", "n1")
        posix.unlink("n1/n2")
        posix.symlink("test", "n1/n2")
        with self.assert_errno(errno.EEXIST, errno.ENOTEMPTY):
            posix.rename("n0", "n1")
        posix.unlink("n1/n2")
        posix.rmdir("n1")
        posix.rmdir("n0")

    def test_rename_23_succeeds_multiply_linked(self) -> None:
        """rename/23.t: rename succeeds when to is multiply linked"""
        posix.mkdir("parent", 0o755)
        posix.close(posix.open("parent/src", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.close(posix.open("parent/dst", posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 4: no hard links on EdenFS
        posix.rename("parent/src", "parent/dst")
        # cases 6-7: depends on a step above
        posix.unlink("parent/dst")
        # case 9: depends on a step above
        posix.mkfifo("parent/src", 0o644)
        posix.mkfifo("parent/dst", 0o644)
        # case 12: no hard links on EdenFS
        posix.rename("parent/src", "parent/dst")
        # cases 14-15: depends on a step above
        posix.unlink("parent/dst")
        # case 17: depends on a step above
        # cases 18-25: needs root (block device nodes)
        # cases 26-33: needs root (char device nodes)
        self.bind_unix_socket("parent/src")
        self.bind_unix_socket("parent/dst")
        # case 36: no hard links on EdenFS
        posix.rename("parent/src", "parent/dst")
        # cases 38-39: depends on a step above
        posix.unlink("parent/dst")
        # case 41: depends on a step above
        posix.rmdir("parent")

    def test_rename_24_directory_updates_link(self) -> None:
        """rename/24.t: rename of a directory updates its .. link"""
        posix.mkdir("src_parent", 0o755)
        posix.mkdir("dst_parent", 0o755)
        posix.mkdir("src_parent/src", 0o755)
        self.assertEqual(3, posix.lstat("src_parent").st_nlink)
        self.assertEqual(2, posix.lstat("dst_parent").st_nlink)
        dotdot_inode = posix.lstat("src_parent").st_ino
        self.assertEqual(dotdot_inode, posix.lstat("src_parent/src/..").st_ino)
        posix.rename("src_parent/src", "dst_parent/dst")
        self.assertEqual(2, posix.lstat("src_parent").st_nlink)
        self.assertEqual(3, posix.lstat("dst_parent").st_nlink)
        dotdot_inode = posix.lstat("dst_parent").st_ino
        self.assertEqual(dotdot_inode, posix.lstat("dst_parent/dst/..").st_ino)
        posix.rmdir("dst_parent/dst")
        posix.rmdir("dst_parent")
        posix.rmdir("src_parent")
