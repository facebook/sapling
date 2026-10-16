#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Holes and space on EdenFS: SEEK_DATA/SEEK_HOLE, posix_fallocate,
posix_fadvise, truncate, st_blocks, statvfs and fstatvfs.

Ported from CPython's ``Lib/test/test_posix.py`` (``PosixTester``) and
``Lib/test/test_os.py`` (``StatAttributeTests``), PSF license, Python 3.12:
one test method per CPython test, named after it, with the CPython class and
test in the docstring, calling the ``posix`` module directly. Comments such
as ``# EdenFS bug: ...`` mark checks that are left out; ``README.md`` in this
directory explains the three reasons and lists the cases.
"""

from __future__ import annotations

import ctypes
import errno
import posix
import stat

from eden.integration.lib import testcase

from .base import PosixTestBase, SEEK_CUR


FALLOC_FL_KEEP_SIZE: int = 1


@testcase.eden_nfs_repo_test
class SpaceTest(PosixTestBase):
    def test_fs_holes(self) -> None:
        """test_posix.PosixTester.test_fs_holes: SEEK_DATA and SEEK_HOLE are consistent

        A file system that does not report holes answers SEEK_DATA with the
        current position and SEEK_HOLE with the end of the file. EdenFS has
        no FUSE_LSEEK handler, so the kernel gives these answers itself.
        """
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"hello")
            size = posix.lseek(fd, 0, SEEK_CUR)
            for i in range(size):
                self.assertEqual(i, posix.lseek(fd, i, posix.SEEK_DATA))
                self.assertLessEqual(size, posix.lseek(fd, i, posix.SEEK_HOLE))
            with self.assert_errno(errno.ENXIO):
                posix.lseek(fd, size, posix.SEEK_DATA)
            with self.assert_errno(errno.ENXIO):
                posix.lseek(fd, size, posix.SEEK_HOLE)

    def test_posix_fallocate(self) -> None:
        """test_posix.PosixTester.test_posix_fallocate: posix_fallocate succeeds"""
        with self.open_fd("n0", posix.O_WRONLY | posix.O_CREAT) as fd:
            posix.posix_fallocate(fd, 0, 10)
            if self.use_nfs():
                # The NFS client has no fallocate; glibc extends the file.
                self.assertEqual(10, posix.fstat(fd).st_size)
            # EdenFS bug: over FUSE posix_fallocate returns 0 but st_size
            # stays 0 (POSIX: the size becomes offset + len). EdenFS extends
            # the overlay file but keeps its cached size.

    def test_fallocate_keep_size(self) -> None:
        """EdenFS addition (no CPython test): fallocate modes other than 0

        EdenFS by design: only mode 0 (what posix_fallocate uses) is
        implemented; FALLOC_FL_KEEP_SIZE fails with EOPNOTSUPP. The NFS client
        has no fallocate at all, so the error is the same there.
        """
        libc = ctypes.CDLL(None, use_errno=True)
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            rv = libc.fallocate(
                fd, FALLOC_FL_KEEP_SIZE, ctypes.c_long(0), ctypes.c_long(100)
            )
            self.assertEqual(-1, rv)
            self.assertEqual(errno.EOPNOTSUPP, ctypes.get_errno())
            self.assertEqual(0, posix.fstat(fd).st_size)
            # EdenFS bug: the handler answers ENOSYS, which makes the kernel
            # stop sending FUSE_FALLOCATE for the life of the mount; from
            # here on glibc emulates every posix_fallocate on this mount with
            # one write per block, which the handler exists to avoid.
            posix.posix_fallocate(fd, 0, 10)
            self.assertEqual(10, posix.fstat(fd).st_size)

    def test_posix_fadvise(self) -> None:
        """test_posix.PosixTester.test_posix_fadvise: POSIX_FADV_WILLNEED succeeds"""
        self.create_file("n0")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            posix.posix_fadvise(fd, 0, 0, posix.POSIX_FADV_WILLNEED)

    def test_ftruncate(self) -> None:
        """test_posix.PosixTester.test_ftruncate: ftruncate an open file to 0"""
        with self.open_fd("n0", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.write(fd, b"test")
            posix.ftruncate(fd, 0)
            self.assertEqual(0, posix.fstat(fd).st_size)

    def test_truncate(self) -> None:
        """test_posix.PosixTester.test_truncate: truncate a path to 0"""
        self.create_file("n0", b"test")
        posix.truncate("n0", 0)
        self.assertEqual(0, posix.stat("n0").st_size)

    def test_statvfs(self) -> None:
        """test_posix.PosixTester.test_statvfs: statvfs of the current directory"""
        self.assertTrue(posix.statvfs("."))

    def test_fstatvfs(self) -> None:
        """test_posix.PosixTester.test_fstatvfs: fstatvfs and statvfs(fd)"""
        self.create_file("n0")
        with self.open_fd("n0", posix.O_RDONLY) as fd:
            self.assertTrue(posix.fstatvfs(fd))
            self.assertTrue(posix.statvfs(fd))

    def test_statvfs_attributes(self) -> None:
        """test_os.StatAttributeTests.test_statvfs_attributes: the statvfs fields

        The checks of the Python ``statvfs_result`` type (assignment,
        constructor) are left out; the file system values stay.
        """
        self.create_file("n0", b"ABC")
        result = posix.statvfs("n0")
        self.assertEqual(result[3], result.f_bfree)
        members = (
            "bsize",
            "frsize",
            "blocks",
            "bfree",
            "bavail",
            "files",
            "ffree",
            "favail",
            "flag",
            "namemax",
        )
        for value, member in enumerate(members):
            self.assertEqual(result[value], getattr(result, "f_" + member))
        self.assertIsInstance(result.f_fsid, int)
        self.assertEqual(10, len(result))
        self.assertGreater(result.f_bsize, 0)
        self.assertGreater(result.f_frsize, 0)
        self.assertEqual(posix.pathconf("n0", "PC_NAME_MAX"), result.f_namemax)

    def test_st_blocks(self) -> None:
        """EdenFS addition (no CPython test): st_blocks follows st_size

        EdenFS by design: st_blocks is st_size rounded up to 512-byte units;
        holes made by ftruncate or pwrite past the end count as allocated.
        """
        self.create_file("n0", b"hello")
        st = posix.stat("n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(1, st.st_blocks)
        with self.open_fd("n1", posix.O_RDWR | posix.O_CREAT) as fd:
            posix.ftruncate(fd, 1 << 20)
            posix.pwrite(fd, b"x", 0)
            st = posix.fstat(fd)
        self.assertEqual(1 << 20, st.st_size)
        self.assertEqual((1 << 20) // 512, st.st_blocks)
