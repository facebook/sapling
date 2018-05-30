#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import hashlib
import os

from .lib import fs, testcase


def sha1(value: bytes) -> str:
    return hashlib.sha1(value).hexdigest()


@testcase.eden_repo_test
class XattrTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.commit("Initial commit.")

    def test_get_sha1_xattr(self) -> None:
        filename = os.path.join(self.mount, "hello")
        xattr = fs.getxattr(filename, "user.sha1")
        contents = open(filename, "rb").read()
        expected_sha1 = sha1(contents)
        self.assertEqual(expected_sha1, xattr)

        # and test what happens as we replace the file contents.
        with open(filename, "w") as f:
            f.write("foo")
            f.flush()
            self.assertEqual(sha1(b"foo"), fs.getxattr(filename, "user.sha1"))

            f.write("bar")
            f.flush()
            self.assertEqual(sha1(b"foobar"), fs.getxattr(filename, "user.sha1"))

            f.write("baz")

        self.assertEqual(sha1(b"foobarbaz"), fs.getxattr(filename, "user.sha1"))

    def test_listxattr(self) -> None:
        filename = os.path.join(self.mount, "hello")
        xattrs = fs.listxattr(filename)
        contents = open(filename, "rb").read()
        expected_sha1 = sha1(contents)
        self.assertEqual({"user.sha1": expected_sha1}, xattrs)
