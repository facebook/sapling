#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import errno
import os
import sys
from contextlib import contextmanager
from typing import Generator

from .lib import testcase


@testcase.eden_repo_test
# pyre-ignore[13]: T62487924
class UnicodeTest(testcase.EdenRepoTest):
    """Verify that non-utf8 files cannot be created on EdenFS."""

    non_utf8_path: bytes

    def populate_repo(self) -> None:
        self.repo.write_file("a", "a")
        self.repo.commit("Initial commit.")

        self.non_utf8_path = os.path.join(self.mount.encode("utf-8"), b"\xff\xfffoobar")

    @contextmanager
    def verifyUtf8Error(self) -> Generator[None, None, None]:
        if sys.platform == "win32":
            with self.assertRaises(UnicodeDecodeError):
                yield
        else:
            with self.assertRaises(OSError) as exc:
                yield

            expectedErrno = errno.EILSEQ
            if self.use_nfs():
                expectedErrno = errno.EINVAL
            self.assertEqual(expectedErrno, exc.exception.errno)

    def test_mkdir_non_utf8(self) -> None:
        with self.verifyUtf8Error():
            os.mkdir(self.non_utf8_path)

    def test_create_file_non_utf8(self) -> None:
        with self.verifyUtf8Error():
            with open(self.non_utf8_path, "w") as f:
                f.write("foo")

    def test_rename_non_utf8(self) -> None:
        with self.verifyUtf8Error():
            os.rename(os.path.join(self.mount, "a"), self.non_utf8_path)
