#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""utimensat(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/utimensat/*.t`` script, named after it, calling the ``posix`` module
directly. Comments such as ``# case 17: needs root`` stand for the script's
TAP cases that are left out; ``README.md`` in this directory explains the
three reasons and lists the cases.
"""

from __future__ import annotations

import posix
import time

from eden.integration.lib import testcase

from .base import PosixTestBase, UTIME_OMIT, utimensat


@testcase.eden_nfs_repo_test
class UtimensatTest(PosixTestBase):
    def test_utimensat_00_changes_timestamps_any_type(self) -> None:
        """utimensat/00.t: utimensat changes timestamps on any type of file"""
        posix.mkdir("n1", 0o755)
        date1 = 1900000000
        date2 = 1950000000
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.unlink("n1/n0")
        posix.mkdir("n1/n0", 0o755)
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.rmdir("n1/n0")
        posix.mkfifo("n1/n0", 0o644)
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.unlink("n1/n0")
        # cases 17-21: needs root (block device nodes)
        # cases 22-26: needs root (char device nodes)
        self.bind_unix_socket("n1/n0")
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_utimensat_01_utime_now_sets_timestamps_to_now(self) -> None:
        """utimensat/01.t: utimensat with UTIME_NOW will set the will set typestamps to now"""
        time_margin = 300
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        old_mtime = posix.lstat("n1/n0").st_mtime
        old_atime = posix.lstat("n1/n0").st_atime
        time.sleep(1)
        posix.utime("n1/n0")
        delta_mtime = posix.lstat("n1/n0").st_mtime - old_mtime
        delta_atime = posix.lstat("n1/n0").st_atime - old_atime
        self.assertGreater(delta_mtime, 0)
        self.assertLess(delta_mtime, time_margin)
        self.assertGreater(delta_atime, 0)
        self.assertLess(delta_atime, time_margin)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_utimensat_02_utime_omit_leaves_time_unchanged(self) -> None:
        """utimensat/02.t: utimensat with UTIME_OMIT will leave the time unchanged"""
        date1 = 1900000000
        date2 = 1950000000
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        orig_atime = posix.lstat("n1/n0").st_atime_ns
        orig_mtime = posix.lstat("n1/n0").st_mtime_ns
        utimensat("n1/n0", (date1, 0), UTIME_OMIT)
        if self.use_nfs():
            # EdenFS ignores an NFS SETATTR that changes only atime.
            self.assertEqual(orig_atime, posix.lstat("n1/n0").st_atime_ns)
        else:
            self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(orig_mtime, posix.lstat("n1/n0").st_mtime_ns)
        atime = posix.lstat("n1/n0").st_atime_ns
        utimensat("n1/n0", UTIME_OMIT, (date2, 0))
        self.assertEqual(atime, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_utimensat_04_can_mtime_atime_vice(self) -> None:
        """utimensat/04.t: utimensat can set mtime < atime or vice versa"""
        date1 = 100000000
        date2 = 200000000
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.utime("n1/n0", ns=(date2 * 10**9, date1 * 10**9))
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_utimensat_05_can_follow_symlinks(self) -> None:
        """utimensat/05.t: utimensat can follow symlinks"""
        date1 = 1900000000
        date2 = 1950000000
        date3 = 1960000000
        date4 = 1970000000
        date5 = 1980000000
        date6 = 1990000000
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.symlink("n0", "n1/n2")
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        posix.utime("n1/n2", ns=(date3 * 10**9, date4 * 10**9), follow_symlinks=False)
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        self.assertEqual(date3 * 10**9, posix.lstat("n1/n2").st_atime_ns)
        self.assertEqual(date4 * 10**9, posix.lstat("n1/n2").st_mtime_ns)
        posix.utime("n1/n2", ns=(date5 * 10**9, date6 * 10**9))
        self.assertEqual(date5 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        self.assertEqual(date6 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        self.assertNotEqual(date5 * 10**9, posix.lstat("n1/n2").st_atime_ns)
        self.assertEqual(date4 * 10**9, posix.lstat("n1/n2").st_mtime_ns)
        posix.unlink("n1/n0")
        posix.unlink("n1/n2")
        posix.rmdir("n1")

    def test_utimensat_08_subsecond_precision(self) -> None:
        """utimensat/08.t: utimensat can set timestamps with subsecond precision"""
        date1 = 100000000
        date1_ns = 100000000
        date2 = 200000000
        date2_ns = 200000000
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.utime("n1/n0", ns=(date1 * 10**9 + date1_ns, date2 * 10**9 + date2_ns))
        self.assertEqual(date1_ns, posix.lstat("n1/n0").st_atime_ns % 10**9)
        self.assertEqual(date2_ns, posix.lstat("n1/n0").st_mtime_ns % 10**9)
        # case 5: no st_birthtime on Linux
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_utimensat_09_y2038_compliant(self) -> None:
        """utimensat/09.t: utimensat is y2038 compliant"""
        date1 = 2147483648  # 2^31
        date2 = 4294967296  # 2^32
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        posix.utime("n1/n0", ns=(date1 * 10**9, date2 * 10**9))
        self.assertEqual(date1 * 10**9, posix.lstat("n1/n0").st_atime_ns)
        if self.use_nfs():
            # nfstime3.seconds is 32 bits (RFC 1813); 2^32 is clamped.
            self.assertEqual((date2 - 1) * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        else:
            self.assertEqual(date2 * 10**9, posix.lstat("n1/n0").st_mtime_ns)
        posix.unlink("n1/n0")
        posix.rmdir("n1")
