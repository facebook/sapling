#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import errno
import os

from .lib import testcase


@testcase.eden_repo_test
class OpenExclusiveTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("readme.txt", "test\n")
        self.repo.commit("Initial commit.")

    def test_oexcl(self) -> None:
        filename = os.path.join(self.mount, "makeme")

        fd = os.open(filename, os.O_EXCL | os.O_CREAT | os.O_RDWR)
        self.assertGreater(fd, -1, msg="Opened file exclusively")
        try:
            os.write(fd, b"foo\n")
        finally:
            os.close(fd)

        with self.assertRaises(OSError) as context:
            fd = os.open(filename, os.O_EXCL | os.O_CREAT | os.O_RDWR)
            if fd != -1:
                os.close(fd)
        self.assertEqual(
            errno.EEXIST,
            context.exception.errno,
            msg="O_EXCL for an existing file raises EEXIST",
        )

        os.unlink(filename)

        fd = os.open(filename, os.O_EXCL | os.O_CREAT | os.O_RDWR)
        self.assertGreater(
            fd, -1, msg="Subsequent O_EXCL is not blocked after " + "removing the file"
        )
        os.close(fd)
