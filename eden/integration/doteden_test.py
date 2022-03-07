#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os
import stat
import sys
from pathlib import Path
from typing import List

from .lib import testcase


@testcase.eden_repo_test
# pyre-fixme[13]: Attribute `dot_eden_path` is never initialized.
class DotEdenTest(testcase.EdenRepoTest):
    """\
    Verify manipulating the .eden directory is disallowed.
    """

    dot_eden_path: Path
    entries: List[Path]

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")
        self.dot_eden_path = Path(self.mount) / ".eden"

    def setUp(self) -> None:
        super().setUp()
        self.entries = list(self.dot_eden_path.iterdir())
        self.assertNotEqual([], self.entries)
        self.epermError = errno.EINVAL if sys.platform == "win32" else errno.EPERM

    def test_rm_existing_contents_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                entry.unlink()
            self.assertEqual(self.epermError, cm.exception.errno)

    def test_mkdir_fails(self) -> None:
        with self.assertRaises(OSError) as cm:
            (self.dot_eden_path / "subdir").mkdir()
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_rmdir_fails(self) -> None:
        with self.assertRaises(OSError) as cm:
            (self.dot_eden_path / "subdir").rmdir()
        # It is no longer possible to create a directory inside .eden -
        # if it was, EPERM would be the right errno value.
        self.assertEqual(errno.ENOENT, cm.exception.errno)

    def test_create_file_fails(self) -> None:
        with self.assertRaises(OSError) as cm:
            os.open(bytes(self.dot_eden_path / "file"), os.O_CREAT | os.O_RDWR)
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_mknod_fails(self) -> None:
        with self.assertRaises(OSError) as cm:
            os.mknod(bytes(self.dot_eden_path / "file"), stat.S_IFREG | 0o600)
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_symlink_fails(self) -> None:
        with self.assertRaises(OSError) as cm:
            (self.dot_eden_path / "lnk").symlink_to("/", target_is_directory=True)
        self.assertEqual(errno.EPERM, cm.exception.errno)

    def test_rename_in_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                entry.rename(self.dot_eden_path / "dst")
            self.assertEqual(self.epermError, cm.exception.errno)

    def test_rename_from_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                entry.rename(Path(self.mount) / "dst")
            self.assertEqual(self.epermError, cm.exception.errno)

    def test_rename_to_fails(self) -> None:
        with self.assertRaises(OSError) as cm:
            (Path(self.mount) / "hello").rename(self.dot_eden_path / "dst")
        self.assertEqual(self.epermError, cm.exception.errno)

    def test_chown_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.chown(
                    bytes(entry),
                    uid=os.getuid(),
                    gid=os.getgid(),
                    follow_symlinks=False,
                )
            self.assertEqual(errno.EPERM, cm.exception.errno)

    # Linux does not allow setting permissions on a symlink.
    def xtest_chmod_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.chmod(bytes(entry), 0o543, follow_symlinks=False)
            self.assertEqual(errno.EPERM, cm.exception.errno)

    # utime() has no effect on symlinks.
    def xtest_touch_fails(self) -> None:
        for entry in self.entries:
            with self.assertRaises(OSError) as cm:
                os.utime(bytes(entry))
            self.assertEqual(errno.EPERM, cm.exception.errno)
