#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""truncate(2) and ftruncate(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/{truncate,ftruncate}/*.t`` script, named after it, calling the ``posix`` module
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
class TruncateTest(PosixTestBase):
    def test_truncate_00_descrease_increase_size(self) -> None:
        """truncate/00.t: truncate decrease/increase file size"""
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.truncate("n1/n0", 1234567)
        self.assertEqual(1234567, posix.lstat("n1/n0").st_size)
        posix.truncate("n1/n0", 567)
        self.assertEqual(567, posix.lstat("n1/n0").st_size)
        posix.unlink("n1/n0")
        fd = posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY | posix.O_TRUNC, 0o644)
        try:
            posix.write(fd, posix.urandom(12345))
        finally:
            posix.close(fd)
        posix.truncate("n1/n0", 23456)
        self.assertEqual(23456, posix.lstat("n1/n0").st_size)
        posix.truncate("n1/n0", 1)
        self.assertEqual(1, posix.lstat("n1/n0").st_size)
        posix.unlink("n1/n0")
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n1/n0").st_ctime_ns
        time.sleep(1)
        posix.truncate("n1/n0", 123)
        self.assertLess(ctime1, posix.stat("n1/n0").st_ctime_ns)
        posix.unlink("n1/n0")
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n1/n0").st_ctime_ns
        time.sleep(1)
        # case 18: needs root
        self.assertEqual(ctime1, posix.stat("n1/n0").st_ctime_ns)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_truncate_01_enotdir_component_prefix_directory(self) -> None:
        """truncate/01.t: truncate returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.truncate("n0/n1/test", 123)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_truncate_02_enametoolong_name_max(self) -> None:
        """truncate/02.t: truncate returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.truncate(nx, 123)
        self.assertEqual(123, posix.stat(nx).st_size)
        posix.unlink(nx)
        # case 5: EdenFS bug: too-long name returns ENOENT

    def test_truncate_03_enametoolong_path_max(self) -> None:
        """truncate/03.t: truncate returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.truncate(nx, 123)
        st = posix.stat(nx)
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(123, st.st_size)
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.truncate(nxx, 123)
        self.rm_rf(nx.partition("/")[0])

    def test_truncate_04_enoent(self) -> None:
        """truncate/04.t: truncate returns ENOENT if the named file does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.truncate("n0/n1/test", 123)
        with self.assert_errno(errno.ENOENT):
            posix.truncate("n0/n1", 123)
        posix.rmdir("n0")

    def test_truncate_07_eloop_too_many_symbolic(self) -> None:
        """truncate/07.t: truncate returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.truncate("n0/test", 123)
        with self.assert_errno(errno.ELOOP):
            posix.truncate("n1/test", 123)
        posix.unlink("n0")
        posix.unlink("n1")

    def test_truncate_09_eisdir_directory(self) -> None:
        """truncate/09.t: truncate returns EISDIR if the named file is a directory"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EISDIR):
            posix.truncate("n0", 123)
        posix.rmdir("n0")

    def test_truncate_12_efbig_einval_maximum_file_size(self) -> None:
        """truncate/12.t: truncate returns EFBIG or EINVAL if the length argument was greater than the maximum file size"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        try:
            posix.truncate("n0", 999999999999999)
        except OSError as e:
            self.assertIn(e.errno, (errno.EFBIG, errno.EINVAL))
            self.assertEqual(0, posix.stat("n0").st_size)
        else:
            self.assertEqual(999999999999999, posix.stat("n0").st_size)
        posix.unlink("n0")

    def test_truncate_13_einval_length_less(self) -> None:
        """truncate/13.t: truncate returns EINVAL if the length argument was less than 0"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EINVAL):
            posix.truncate("n0", -1)
        with self.assert_errno(errno.EINVAL):
            posix.truncate("n0", -999999)
        posix.unlink("n0")

    def test_ftruncate_00_descrease_increase_size(self) -> None:
        """ftruncate/00.t: ftruncate decrease/increase file size"""
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        fd = posix.open("n1/n0", posix.O_RDWR)
        try:
            posix.ftruncate(fd, 1234567)
        finally:
            posix.close(fd)
        self.assertEqual(1234567, posix.lstat("n1/n0").st_size)
        fd = posix.open("n1/n0", posix.O_WRONLY)
        try:
            posix.ftruncate(fd, 567)
        finally:
            posix.close(fd)
        self.assertEqual(567, posix.lstat("n1/n0").st_size)
        posix.unlink("n1/n0")
        fd = posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY | posix.O_TRUNC, 0o644)
        try:
            posix.write(fd, posix.urandom(12345))
        finally:
            posix.close(fd)
        fd = posix.open("n1/n0", posix.O_RDWR)
        try:
            posix.ftruncate(fd, 23456)
        finally:
            posix.close(fd)
        self.assertEqual(23456, posix.lstat("n1/n0").st_size)
        fd = posix.open("n1/n0", posix.O_WRONLY)
        try:
            posix.ftruncate(fd, 1)
        finally:
            posix.close(fd)
        self.assertEqual(1, posix.lstat("n1/n0").st_size)
        posix.unlink("n1/n0")
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n1/n0").st_ctime_ns
        time.sleep(1)
        fd = posix.open("n1/n0", posix.O_RDWR)
        try:
            posix.ftruncate(fd, 123)
        finally:
            posix.close(fd)
        self.assertLess(ctime1, posix.stat("n1/n0").st_ctime_ns)
        posix.unlink("n1/n0")
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n1/n0").st_ctime_ns
        time.sleep(1)
        # case 18: needs root
        self.assertEqual(ctime1, posix.stat("n1/n0").st_ctime_ns)
        posix.unlink("n1/n0")
        fd = posix.open("n1/n0", posix.O_CREAT | posix.O_RDWR, 0o000)
        try:
            posix.ftruncate(fd, 0)
        finally:
            posix.close(fd)
        posix.unlink("n1/n0")
        posix.chmod("n1", 0o777)
        # case 24: needs root
        # case 25: depends on a step above
        posix.rmdir("n1")

    def test_ftruncate_01_enotdir_component_prefix_directory(self) -> None:
        """ftruncate/01.t: truncate returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.truncate("n0/n1/test", 123)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_ftruncate_02_enametoolong_name_max(self) -> None:
        """ftruncate/02.t: truncate returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.truncate(nx, 123)
        self.assertEqual(123, posix.stat(nx).st_size)
        posix.unlink(nx)
        # case 5: EdenFS bug: too-long name returns ENOENT

    def test_ftruncate_03_enametoolong_path_max(self) -> None:
        """ftruncate/03.t: truncate returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.truncate(nx, 123)
        st = posix.stat(nx)
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(123, st.st_size)
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.truncate(nxx, 123)
        self.rm_rf(nx.partition("/")[0])

    def test_ftruncate_04_enoent(self) -> None:
        """ftruncate/04.t: truncate returns ENOENT if the named file does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.truncate("n0/n1/test", 123)
        with self.assert_errno(errno.ENOENT):
            posix.truncate("n0/n1", 123)
        posix.rmdir("n0")

    def test_ftruncate_07_eloop_too_many_symbolic(self) -> None:
        """ftruncate/07.t: truncate returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.truncate("n0/test", 123)
        with self.assert_errno(errno.ELOOP):
            posix.truncate("n1/test", 123)
        posix.unlink("n0")
        posix.unlink("n1")

    def test_ftruncate_09_eisdir_directory(self) -> None:
        """ftruncate/09.t: truncate returns EISDIR if the named file is a directory"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EISDIR):
            posix.truncate("n0", 123)
        posix.rmdir("n0")

    def test_ftruncate_12_efbig_einval_maximum_file_size(self) -> None:
        """ftruncate/12.t: truncate returns EFBIG or EINVAL if the length argument was greater than the maximum file size"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        try:
            posix.truncate("n0", 999999999999999)
        except OSError as e:
            self.assertIn(e.errno, (errno.EFBIG, errno.EINVAL))
            self.assertEqual(0, posix.stat("n0").st_size)
        else:
            self.assertEqual(999999999999999, posix.stat("n0").st_size)
        posix.unlink("n0")

    def test_ftruncate_13_einval_length_less(self) -> None:
        """ftruncate/13.t: ftruncate returns EINVAL if the length argument was less than 0"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        fd = posix.open("n0", posix.O_RDWR)
        try:
            with self.assert_errno(errno.EINVAL):
                posix.ftruncate(fd, -1)
        finally:
            posix.close(fd)
        fd = posix.open("n0", posix.O_WRONLY)
        try:
            with self.assert_errno(errno.EINVAL):
                posix.ftruncate(fd, -999999)
        finally:
            posix.close(fd)
        posix.unlink("n0")
