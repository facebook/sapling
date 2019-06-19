#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import hashlib
import os
from typing import Dict

from .lib import testcase


def getallxattrs(abspath: str) -> Dict[str, bytes]:
    result = {}
    for xattr in os.listxattr(abspath):
        result[xattr] = os.getxattr(abspath, xattr)
    return result


def sha1(value: bytes) -> bytes:
    return hashlib.sha1(value).hexdigest().encode("ascii")


@testcase.eden_repo_test
class XattrTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("subdir/file", "contents")
        self.repo.commit("Initial commit.")

    def test_get_sha1_xattr(self) -> None:
        filename = os.path.join(self.mount, "hello")
        xattr = os.getxattr(filename, "user.sha1")
        contents = open(filename, "rb").read()
        expected_sha1 = sha1(contents)
        self.assertEqual(expected_sha1, xattr)

        # and test what happens as we replace the file contents.
        with open(filename, "w") as f:
            f.write("foo")
            f.flush()
            self.assertEqual(sha1(b"foo"), os.getxattr(filename, "user.sha1"))

            f.write("bar")
            f.flush()
            self.assertEqual(sha1(b"foobar"), os.getxattr(filename, "user.sha1"))

            f.write("baz")

        self.assertEqual(sha1(b"foobarbaz"), os.getxattr(filename, "user.sha1"))

    def test_listxattr(self) -> None:
        # Assert that listxattr on a directory is empty and does not break
        # future listxattr calls.
        self.assertEqual([], os.listxattr(os.path.join(self.mount, "subdir")))

        filename = os.path.join(self.mount, "hello")
        xattrs = getallxattrs(filename)
        contents = open(filename, "rb").read()
        expected_sha1 = sha1(contents)
        self.assertEqual({"user.sha1": expected_sha1}, xattrs)

    def test_get_sha1_xattr_succeeds_after_querying_xattr_on_dir(self) -> None:
        with self.assertRaises(OSError):
            os.getxattr(self.mount, "does_not_exist")

        filename = os.path.join(self.mount, "hello")
        xattr = os.getxattr(filename, "user.sha1")
        contents = open(filename, "rb").read()
        expected_sha1 = sha1(contents)
        self.assertEqual(expected_sha1, xattr)
