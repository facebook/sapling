#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""chown(2) and lchown(2) on EdenFS.

Ported from the pjdfstest POSIX file system test suite
(https://github.com/pjd/pjdfstest, BSD-2-Clause): one test method per
``tests/chown/*.t`` script, named after it, calling the ``posix`` module
directly. Comments such as ``# case 17: needs root`` stand for the script's
TAP cases that are left out; ``README.md`` in this directory explains the
three reasons and lists the cases.
"""

from __future__ import annotations

import errno
import posix

from eden.integration.lib import testcase

from .base import PosixTestBase


@testcase.eden_nfs_repo_test
class ChownTest(PosixTestBase):
    def test_chown_01_enotdir_component_prefix_directory(self) -> None:
        """chown/01.t: chown returns ENOTDIR if a component of the path prefix is not a directory"""
        posix.mkdir("n0", 0o755)
        posix.close(posix.open("n0/n1", posix.O_CREAT | posix.O_EXCL, 0o644))
        with self.assert_errno(errno.ENOTDIR):
            posix.chown("n0/n1/test", 65534, 65534)
        with self.assert_errno(errno.ENOTDIR):
            posix.lchown("n0/n1/test", 65534, 65534)
        posix.unlink("n0/n1")
        posix.mkfifo("n0/n1", 0o644)
        with self.assert_errno(errno.ENOTDIR):
            posix.chown("n0/n1/test", 65534, 65534)
        with self.assert_errno(errno.ENOTDIR):
            posix.lchown("n0/n1/test", 65534, 65534)
        posix.unlink("n0/n1")
        # cases 10-13: needs root (block device nodes)
        # cases 14-17: needs root (char device nodes)
        self.bind_unix_socket("n0/n1")
        with self.assert_errno(errno.ENOTDIR):
            posix.chown("n0/n1/test", 65534, 65534)
        with self.assert_errno(errno.ENOTDIR):
            posix.lchown("n0/n1/test", 65534, 65534)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_chown_03_enametoolong_path_max(self) -> None:
        """chown/03.t: chown returns ENAMETOOLONG if an entire path name exceeded {PATH_MAX} characters"""
        nx = self.path_max()
        nxx = nx + "x"
        self.mkdir_p(nx.rpartition("/")[0])
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 2: needs root
        # case 3: depends on a step above
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.chown(nxx, 65534, 65534)
        posix.close(posix.open(nx, posix.O_CREAT | posix.O_EXCL, 0o644))
        # case 7: needs root
        # case 8: depends on a step above
        posix.unlink(nx)
        with self.assert_errno(errno.ENAMETOOLONG):
            posix.lchown(nxx, 65534, 65534)
        self.rm_rf(nx.partition("/")[0])

    def test_chown_04_enoent(self) -> None:
        """chown/04.t: chown returns ENOENT if the named file does not exist"""
        posix.mkdir("n0", 0o755)
        with self.assert_errno(errno.ENOENT):
            posix.chown("n0/n1/test", 65534, 65534)
        with self.assert_errno(errno.ENOENT):
            posix.chown("n0/n1", 65534, 65534)
        with self.assert_errno(errno.ENOENT):
            posix.lchown("n0/n1/test", 65534, 65534)
        with self.assert_errno(errno.ENOENT):
            posix.lchown("n0/n1", 65534, 65534)
        posix.symlink("n2", "n0/n1")
        with self.assert_errno(errno.ENOENT):
            posix.chown("n0/n1", 65534, 65534)
        posix.unlink("n0/n1")
        posix.rmdir("n0")

    def test_chown_06_eloop_too_many_symbolic(self) -> None:
        """chown/06.t: chown returns ELOOP if too many symbolic links were encountered in translating the pathname"""
        posix.symlink("n0", "n1")
        posix.symlink("n1", "n0")
        with self.assert_errno(errno.ELOOP):
            posix.chown("n0", 65534, 65534)
        with self.assert_errno(errno.ELOOP):
            posix.chown("n1", 65534, 65534)
        with self.assert_errno(errno.ELOOP):
            posix.chown("n0/test", 65534, 65534)
        with self.assert_errno(errno.ELOOP):
            posix.chown("n1/test", 65534, 65534)
        with self.assert_errno(errno.ELOOP):
            posix.lchown("n0/test", 65534, 65534)
        with self.assert_errno(errno.ELOOP):
            posix.lchown("n1/test", 65534, 65534)
        posix.unlink("n0")
        posix.unlink("n1")
