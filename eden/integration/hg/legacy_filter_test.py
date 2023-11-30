# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import sys
from typing import Dict, List, Optional, Set

from eden.integration.lib import hgrepo

from facebook.eden.ttypes import SHA1Result, SyncBehavior

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class FilterTest(EdenHgTestCase):
    hidden_files: Set[str] = {"foo/foo.cpp", "foo/bar/baz/file", "hello"}
    symlink_to_hidden: Set[str] = {"fooslink", "foofooslink"}

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("foo/foo.cpp", "foo\n")
        repo.write_file("foo/bar/baz/file", "foo\n")
        repo.write_file("bar/bar.cpp", "bar\n")
        repo.write_file("baz/baz.cpp", "baz\n")
        repo.write_file("baz/baz2.cpp", "baz\n")
        repo.write_file("hello", "World\n")
        if sys.platform != "win32":
            repo.symlink("fooslink", "foo")
            repo.symlink("foofooslink", "foo/foo.cpp")
        repo.commit("Initial commit.")

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("hg", []).append(
            "filtered-paths = ['foo', 'hello', 'baz/baz.cpp']"
        )
        return result

    def test_read_dir(self) -> None:
        listing = set(self.read_dir(""))
        if sys.platform == "win32":
            self.assertEqual(listing, {"bar", "baz", ".eden", ".hg"})
        else:
            self.assertEqual(
                listing, {"bar", "baz", "foofooslink", "fooslink", ".eden", ".hg"}
            )

        listing = self.read_dir("baz")
        self.assertEqual(listing, ["baz2.cpp"])

    def test_read_hidden_file(self) -> None:
        for path in self.hidden_files:
            with self.assertRaisesRegex(IOError, ""):
                self.read_file(path)

        if sys.platform != "win32":
            for path in self.symlink_to_hidden:
                with self.assertRaisesRegex(IOError, ""):
                    self.read_file(path)

    def test_get_sha1_thrift(self) -> None:
        with self.get_thrift_client_legacy() as client:
            for path in self.hidden_files:
                result = client.getSHA1(
                    self.mount_path_bytes, [path.encode()], sync=SyncBehavior()
                )
                self.assertEqual(len(result), 1)
                self.assertEqual(result[0].getType(), SHA1Result.ERROR)
                self.assertRegex(
                    result[0].get_error().message, ".*: No such file or directory"
                )
