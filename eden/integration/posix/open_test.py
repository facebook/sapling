#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""open(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/open/*.t`` script, named after it, calling the ``posix`` module
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
class OpenTest(PosixTestBase):
    def test_open_00_opens_eventually_creates(self) -> None:
        """open/00.t: open opens (and eventually creates) a file"""
        posix.mkdir("n1", 0o755)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o755))
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o755, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o151))
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o151, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.umask(0o077)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o151))
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o100, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.umask(0o070)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o345))
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o305, stat.S_IMODE(st.st_mode))
        posix.unlink("n1/n0")
        posix.umask(0o501)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o345))
        posix.umask(0)
        st = posix.lstat("n1/n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
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
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o644))
        self.assertLess(time0, posix.stat("n1/n0").st_atime_ns)
        self.assertLess(time0, posix.stat("n1/n0").st_mtime_ns)
        self.assertLess(time0, posix.stat("n1/n0").st_ctime_ns)
        self.assertLess(time0, posix.stat("n1").st_mtime_ns)
        self.assertLess(time0, posix.stat("n1").st_ctime_ns)
        posix.unlink("n1/n0")
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        dmtime = posix.stat("n1").st_mtime_ns
        dctime = posix.stat("n1").st_ctime_ns
        time.sleep(1)
        posix.close(posix.open("n1/n0", posix.O_CREAT | posix.O_RDONLY, 0o644))
        self.assertEqual(dmtime, posix.stat("n1").st_mtime_ns)
        self.assertEqual(dctime, posix.stat("n1").st_ctime_ns)
        posix.unlink("n1/n0")
        fd = posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY | posix.O_TRUNC, 0o644)
        try:
            posix.write(fd, b"test\n")
        finally:
            posix.close(fd)
        self.assertEqual(5, posix.stat("n1/n0").st_size)
        ctime1 = posix.stat("n1/n0").st_ctime_ns
        time.sleep(1)
        posix.close(posix.open("n1/n0", posix.O_WRONLY | posix.O_TRUNC))
        # case 43: EdenFS bug: truncate leaves mtime unchanged
        self.assertLess(ctime1, posix.stat("n1/n0").st_ctime_ns)
        self.assertEqual(0, posix.stat("n1/n0").st_size)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_open_01_enotdir_component_prefix_directory(self) -> None:
        """open/01.t: open returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0/n1/test", posix.O_RDONLY)
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0/n1/test", posix.O_CREAT, 0o644)
        posix.unlink("n0/n1")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0/n1/test", posix.O_RDONLY)
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0/n1/test", posix.O_CREAT, 0o644)
        posix.unlink("n0/n1")
        # cases 10-13: needs root (block device nodes)
        # cases 14-17: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0/n1/test", posix.O_RDONLY)
        with self.assert_errno(errno.ENOTDIR):
            posix.open("n0/n1/test", posix.O_CREAT, 0o644)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_open_02_enametoolong_name_max(self) -> None:
        """open/02.t: open returns ENAMETOOLONG if a component of a pathname exceeded {NAME_MAX} characters"""
        nx = self.name_max()
        nxx = nx + "x"
        posix.close(posix.open(nx, posix.O_CREAT, 0o620))
        st = posix.stat(nx)
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o620, stat.S_IMODE(st.st_mode))
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.open(nxx, posix.O_CREAT, 0o620)

    def test_open_03_enametoolong_path_max(self) -> None:
        """open/03.t: open returns ENAMETOOLONG if an entire path name exceeded ${PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT, 0o642))
        st = posix.stat(nx)
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o642, stat.S_IMODE(st.st_mode))
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.open(nxx, posix.O_CREAT, 0o642)
        self.rm_rf(nx.partition("/")[0])

    def test_open_04_enoent_component_name_must(self) -> None:
        """open/04.t: open returns ENOENT if a component of the path name that must exist does not exist or O_CREAT is not set and the named file does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.open("n0/n1/test", posix.O_CREAT, 0o644)
        with self.assert_errno(errno.ENOENT):
            posix.open("n0/n1", posix.O_RDONLY)
        posix.rmdir("n0")

    def test_open_12_eloop_too_many_symbolic(self) -> None:
        """open/12.t: open returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.open("n0/test", posix.O_RDONLY)
        with self.assert_errno(errno.ELOOP):
            posix.open("n1/test", posix.O_RDONLY)
        posix.unlink("n0")
        posix.unlink("n1")

    def test_open_16_eloop_o_nofollow_target_symbolic(self) -> None:
        """open/16.t: open returns ELOOP when O_NOFOLLOW was specified and the target is a symbolic link"""
        posix.symlink("n0", "n1")
        with self.assert_errno(errno.ELOOP):
            posix.open("n1", posix.O_RDONLY | posix.O_CREAT | posix.O_NOFOLLOW, 0o644)
        with self.assert_errno(errno.ELOOP):
            posix.open("n1", posix.O_RDONLY | posix.O_NOFOLLOW)
        with self.assert_errno(errno.ELOOP):
            posix.open("n1", posix.O_WRONLY | posix.O_NOFOLLOW)
        with self.assert_errno(errno.ELOOP):
            posix.open("n1", posix.O_RDWR | posix.O_NOFOLLOW)
        posix.unlink("n1")

    def test_open_17_enxio_o_nonblock_fifo_o_wronly(self) -> None:
        """open/17.t: open returns ENXIO when O_NONBLOCK is set, the named file is a fifo, O_WRONLY is set, and no process has the file open for reading"""
        posix.mkfifo("n0", 0o644)
        with self.assert_errno(errno.ENXIO):
            posix.open("n0", posix.O_WRONLY | posix.O_NONBLOCK)
        posix.unlink("n0")

    def test_open_22_eexist_o_creat_o_excl(self) -> None:
        """open/22.t: open returns EEXIST when O_CREAT and O_EXCL were specified and the file exists"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.EEXIST):
            posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644)
        posix.unlink("n0")
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.EEXIST):
            posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644)
        posix.rmdir("n0")
        posix.mkfifo("n0", 0o644)
        with self.assert_errno(errno.EEXIST):
            posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644)
        posix.unlink("n0")
        # cases 10-12: needs root (block device nodes)
        # cases 13-15: needs root (char device nodes)
        self.bind_unix_socket("n0")
        with self.assert_errno(errno.EEXIST):
            posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644)
        posix.unlink("n0")
        posix.symlink("test", "n0")
        with self.assert_errno(errno.EEXIST):
            posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644)
        posix.unlink("n0")

    def test_open_23_einval_attempt_made_open(self) -> None:
        """open/23.t: open may return EINVAL when an attempt was made to open a descriptor with an illegal combination of O_RDONLY, O_WRONLY, and O_RDWR"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_EXCL, 0o644))
        try:
            posix.close(posix.open("n0", posix.O_RDONLY | posix.O_RDWR))
        except OSError as e:
            self.assertIn(e.errno, (errno.EINVAL,))
        try:
            posix.close(posix.open("n0", posix.O_WRONLY | posix.O_RDWR))
        except OSError as e:
            self.assertIn(e.errno, (errno.EINVAL,))
        try:
            posix.close(
                posix.open("n0", posix.O_RDONLY | posix.O_WRONLY | posix.O_RDWR)
            )
        except OSError as e:
            self.assertIn(e.errno, (errno.EINVAL,))
        posix.unlink("n0")

    def test_open_24_enxio_trying_open_unix(self) -> None:
        """open/24.t: open returns ENXIO when trying to open UNIX domain socket"""
        self.bind_unix_socket("n0")
        with self.assert_errno(errno.ENXIO):
            posix.open("n0", posix.O_RDONLY)
        with self.assert_errno(errno.ENXIO):
            posix.open("n0", posix.O_WRONLY)
        with self.assert_errno(errno.ENXIO):
            posix.open("n0", posix.O_RDWR)
        posix.unlink("n0")

    def test_open_25_files_larger_than_2gb(self) -> None:
        """open/25.t: interact with > 2 GB files"""
        offset = 2 * 1024 * 1024 * 1024 + 1
        posix.mkdir("n1", 0o755)
        fd = posix.open("n1/n0", posix.O_CREAT | posix.O_WRONLY, 0o755)
        try:
            self.assertEqual(1, posix.pwrite(fd, b"a", offset))
        finally:
            posix.close(fd)
        self.assertEqual(offset + 1, posix.lstat("n1/n0").st_size)
        fd = posix.open("n1/n0", posix.O_RDONLY)
        try:
            self.assertEqual(b"a", posix.pread(fd, 1, offset))
        finally:
            posix.close(fd)
        posix.unlink("n1/n0")
        posix.rmdir("n1")

    def test_open_26_can_create_0000_mode(self) -> None:
        """open/26.t: open can create files with 0000 mode"""
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_WRONLY, 0o000))
        st = posix.lstat("n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o000, stat.S_IMODE(st.st_mode))
        posix.unlink("n0")
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_RDWR, 0o000))
        st = posix.lstat("n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o000, stat.S_IMODE(st.st_mode))
        posix.unlink("n0")
        posix.close(posix.open("n0", posix.O_CREAT | posix.O_RDONLY, 0o000))
        st = posix.lstat("n0")
        self.assertTrue(stat.S_ISREG(st.st_mode))
        self.assertEqual(0o000, stat.S_IMODE(st.st_mode))
        posix.unlink("n0")
