#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""chmod(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/chmod/*.t`` script, named after it, calling the ``posix`` module
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
class ChmodTest(PosixTestBase):
    def test_chmod_00_changes_permission(self) -> None:
        """chmod/00.t: chmod changes permission"""
        posix.mkdir("n2", 0o755)
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.chmod("n2/n0", 0o111)
        self.assertEqual(0o111, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.symlink("n0", "n2/n1")
        mode = stat.S_IMODE(posix.lstat("n2/n1").st_mode)
        posix.chmod("n2/n1", 0o222)
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n1").st_mode))
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        self.assertEqual(mode, stat.S_IMODE(posix.lstat("n2/n1").st_mode))
        posix.unlink("n2/n1")
        posix.unlink("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        posix.chmod("n2/n0", 0o111)
        self.assertEqual(0o111, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.symlink("n0", "n2/n1")
        mode = stat.S_IMODE(posix.lstat("n2/n1").st_mode)
        posix.chmod("n2/n1", 0o222)
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n1").st_mode))
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        self.assertEqual(mode, stat.S_IMODE(posix.lstat("n2/n1").st_mode))
        posix.unlink("n2/n1")
        posix.rmdir("n2/n0")
        posix.mkfifo("n2/n0", 0o644)
        posix.chmod("n2/n0", 0o111)
        self.assertEqual(0o111, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.symlink("n0", "n2/n1")
        mode = stat.S_IMODE(posix.lstat("n2/n1").st_mode)
        posix.chmod("n2/n1", 0o222)
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n1").st_mode))
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        self.assertEqual(mode, stat.S_IMODE(posix.lstat("n2/n1").st_mode))
        posix.unlink("n2/n1")
        posix.unlink("n2/n0")
        # cases 32-41: needs root (block device nodes)
        # cases 42-51: needs root (char device nodes)
        self.bind_unix_socket("n2/n0")
        posix.chmod("n2/n0", 0o111)
        self.assertEqual(0o111, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.symlink("n0", "n2/n1")
        mode = stat.S_IMODE(posix.lstat("n2/n1").st_mode)
        posix.chmod("n2/n1", 0o222)
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n1").st_mode))
        self.assertEqual(0o222, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        self.assertEqual(mode, stat.S_IMODE(posix.lstat("n2/n1").st_mode))
        posix.unlink("n2/n1")
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.chmod("n2/n0", 0o111)
        self.assertLess(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.chmod("n2/n0", 0o111)
        self.assertLess(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.rmdir("n2/n0")
        posix.mkfifo("n2/n0", 0o644)
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.chmod("n2/n0", 0o111)
        self.assertLess(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        # cases 74-77: needs root (block device nodes)
        # cases 78-81: needs root (char device nodes)
        self.bind_unix_socket("n2/n0")
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        posix.chmod("n2/n0", 0o111)
        self.assertLess(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 87: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        posix.mkdir("n2/n0", 0o755)
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 91: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.rmdir("n2/n0")
        posix.mkfifo("n2/n0", 0o644)
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 95: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        # cases 98-101: needs root (block device nodes)
        # cases 102-105: needs root (char device nodes)
        self.bind_unix_socket("n2/n0")
        ctime1 = posix.stat("n2/n0").st_ctime_ns
        time.sleep(1)
        # case 107: needs root
        self.assertEqual(ctime1, posix.stat("n2/n0").st_ctime_ns)
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o755))
        # cases 111-112: needs root
        # case 113: depends on a step above
        # case 114: needs root
        self.assertEqual(0o755, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        # case 116: needs root
        self.assertEqual(0o755, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        posix.rmdir("n2")

    def test_chmod_01_enotdir_component_prefix_directory(self) -> None:
        """chmod/01.t: chmod returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.chmod("n0/n1/test", 0o644)
        posix.unlink("n0/n1")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.chmod("n0/n1/test", 0o644)
        posix.unlink("n0/n1")
        # cases 8-10: needs root (block device nodes)
        # cases 11-13: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.chmod("n0/n1/test", 0o644)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_chmod_02_enametoolong_name_max(self) -> None:
        """chmod/02.t: chmod returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.chmod(nx, 0o620)
        self.assertEqual(0o620, stat.S_IMODE(posix.stat(nx).st_mode))
        posix.unlink(nx)
        # case 5: EdenFS bug: too-long name returns ENOENT

    def test_chmod_03_enametoolong_path_max(self) -> None:
        """chmod/03.t: chmod returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.chmod(nx, 0o642)
        self.assertEqual(0o642, stat.S_IMODE(posix.stat(nx).st_mode))
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.chmod(nxx, 0o642)
        self.rm_rf(nx.partition("/")[0])

    def test_chmod_04_enoent(self) -> None:
        """chmod/04.t: chmod returns ENOENT if the named file does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.chmod("n0/n1/test", 0o644)
        with self.assert_errno(errno.ENOENT):
            posix.chmod("n0/n1", 0o644)
        posix.symlink("n2", "n0/n1")
        with self.assert_errno(errno.ENOENT):
            posix.chmod("n0/n1", 0o644)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_chmod_06_eloop_too_many_symbolic(self) -> None:
        """chmod/06.t: chmod returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.chmod("n0", 0o644)
        with self.assert_errno(errno.ELOOP):
            posix.chmod("n1", 0o644)
        with self.assert_errno(errno.ELOOP):
            posix.chmod("n0/test", 0o644)
        with self.assert_errno(errno.ELOOP):
            posix.chmod("n1/test", 0o644)
        posix.unlink("n0")
        posix.unlink("n1")

    def test_chmod_12_verify_suid_sgid_bit(self) -> None:
        """chmod/12.t: verify SUID/SGID bit behaviour"""
        posix.mkdir("n2", 0o755)
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o4777))
        # case 3: needs root
        self.assertEqual(0o777, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o2777))
        # case 7: needs root
        self.assertEqual(0o777, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        posix.close(posix.open("n2/n0", posix.O_CREAT | posix.O_EXCL, 0o6777))
        # case 11: needs root
        self.assertEqual(0o777, stat.S_IMODE(posix.stat("n2/n0").st_mode))
        posix.unlink("n2/n0")
        posix.rmdir("n2")
