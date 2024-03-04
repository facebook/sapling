# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import sys
from typing import Dict, List, Optional, Set

from eden.integration.lib import hgrepo

from facebook.eden.ttypes import SHA1Result, SyncBehavior

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-ignore[13]: T62487924
class FilterTest(EdenHgTestCase):
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

    def hidden_files(self) -> Set[str]:
        if self.backing_store_type == "filteredhg":
            return set()
        else:
            return {"foo/foo.cpp", "foo/bar/baz/file", "hello"}

    def expected_toplevel_readdir_result(self) -> Set[str]:
        if self.backing_store_type == "filteredhg":
            return {
                "bar",
                "baz",
                ".eden",
                "foo",
                "foofooslink",
                "fooslink",
                "hello",
                ".hg",
            }
        else:
            return {"bar", "baz", "foofooslink", "fooslink", ".eden", ".hg"}

    def expected_baz_readdir_result(self) -> Set[str]:
        if self.backing_store_type == "filteredhg":
            return {"baz.cpp", "baz2.cpp"}
        else:
            return {"baz2.cpp"}

    def symlink_to_hidden(self) -> Set[str]:
        if self.backing_store_type == "filteredhg":
            return set()
        else:
            return {"fooslink", "foofooslink"}

    def test_read_dir(self) -> None:
        listing = set(self.read_dir(""))
        if sys.platform == "win32":
            # On Windows we don't expect symlinked files to exist. Note: this
            # could change if symlinks are rolled out to Windows in the future.
            self.assertEqual(
                listing,
                set(
                    filter(
                        lambda e: "link" in e, self.expected_toplevel_readdir_result()
                    )
                ),
            )
        else:
            self.assertEqual(listing, self.expected_toplevel_readdir_result())

        listing = set(self.read_dir("baz"))
        self.assertEqual(listing, self.expected_baz_readdir_result())

    def test_read_hidden_file(self) -> None:
        for path in self.hidden_files():
            with self.assertRaisesRegex(IOError, ""):
                self.read_file(path)

        if sys.platform != "win32":
            for path in self.symlink_to_hidden():
                with self.assertRaisesRegex(IOError, ""):
                    self.read_file(path)

    def test_get_sha1_thrift(self) -> None:
        with self.get_thrift_client_legacy() as client:
            for path in self.hidden_files():
                result = client.getSHA1(
                    self.mount_path_bytes, [path.encode()], sync=SyncBehavior()
                )
                self.assertEqual(len(result), 1)
                self.assertEqual(result[0].getType(), SHA1Result.ERROR)
                self.assertRegex(
                    result[0].get_error().message, ".*: No such file or directory"
                )
