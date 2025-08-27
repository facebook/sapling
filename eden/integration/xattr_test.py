#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import hashlib
import os
import sys
from typing import Dict

from .lib import testcase

if sys.platform == "linux":
    from errno import ENODATA as kENODATA
else:
    from errno import ENOATTR as kENODATA


def getallxattrs(abspath: str) -> Dict[str, bytes]:
    result = {}
    for xattr in os.listxattr(abspath):
        result[xattr] = os.getxattr(abspath, xattr)
    return result


def sha1(value: bytes) -> bytes:
    return hashlib.sha1(value).hexdigest().encode("ascii")


@testcase.eden_repo_test
class XattrTest(testcase.EdenRepoTest):
    # There's currently no good way to calculate these on the fly. Precompute them for testing purposes.
    expected_file_digest_hash = (
        b"507c3561b91c17f73b215cc95b4456194c2fea86e484339c744065fdb7817ad9"
    )
    expected_dir_digest_hash = (
        b"bd2ac888be110f12ab95a80ed850049c85f759113728271bcb98e11f00fb6bc8"
    )

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("subdir/file", "contents")
        self.repo.commit("Initial commit.")
        if self.repo_type in ["hg", "filteredhg"]:
            self.repo.push(".", "master", create=True)

    def test_get_sha1_xattr(self) -> None:
        filename = os.path.join(self.mount, "hello")
        xattr = os.getxattr(filename, "user.sha1")
        with open(filename, "rb") as f:
            contents = f.read()
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
        self.assertEqual({}, xattrs)

    def test_get_sha1_xattr_succeeds_after_querying_xattr_on_dir(self) -> None:
        with self.assertRaises(OSError):
            os.getxattr(self.mount, "does_not_exist")

        filename = os.path.join(self.mount, "hello")
        xattr = os.getxattr(filename, "user.sha1")
        with open(filename, "rb") as f:
            contents = f.read()
        expected_sha1 = sha1(contents)
        self.assertEqual(expected_sha1, xattr)

    def test_get_digest_hash_xattr(self) -> None:
        filename = os.path.join(self.mount, "hello")
        dirname = os.path.join(self.mount, "subdir")

        # The directory digest hash xattr is only supported on hg repos
        if self.repo.get_type() not in ["hg", "filteredhg"]:
            with self.assertRaises(OSError):
                os.getxattr(dirname, "user.digesthash")
            return

        # For hg repos, we expect digest hashes to be available for all files and unmaterialized dirs
        file_xattr = os.getxattr(filename, "user.digesthash")
        dir_xattr = os.getxattr(dirname, "user.digesthash")
        self.assertEqual(self.expected_file_digest_hash, file_xattr)
        self.assertEqual(self.expected_dir_digest_hash, dir_xattr)

        # and test what happens as we materialize the directory
        with open(os.path.join(dirname, "new_file"), "w") as f:
            f.write("foo")
            f.flush()
            with self.assertRaises(OSError) as cm:
                self.assertEqual(kENODATA, os.getxattr(dirname, "user.digesthash"))
            self.assertEqual(kENODATA, cm.exception.errno)
