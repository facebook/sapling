# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import subprocess
from pathlib import Path
from typing import List, Optional

from eden.integration.hg.lib.hg_extension_test_base import (
    EdenHgTestCase,
    filteredhg_test,
    FilteredHgTestCase,
    hg_test,
)

from eden.integration.lib import hgrepo
from facebook.eden.ttypes import GetCurrentSnapshotInfoRequest, MountId


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSCloneBase(FilteredHgTestCase):
    """Clone FilteredFS repos using `hg clone`"""

    test_filter0: str = """
[exclude]
foo
[include]
bar
"""

    test_filter1: str = """
[include]
*
"""

    test_filter2: str = """
[include]
*

[exclude]
filtered
"""

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("filter0", self.test_filter0)
        repo.write_file("tools/scm/filter/filter1", self.test_filter1)
        repo.write_file("tools/scm/filter/filter2", self.test_filter2)
        repo.write_file("foo", "foo")
        repo.write_file("bar", "bar")
        repo.write_file("filtered", "I should be filtered by filter2")
        repo.commit("Initial commit.")

    def eden_clone_filteredhg_repo(
        self, backing_store: Optional[str] = None, filter_path: Optional[str] = None
    ) -> Path:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(empty_dir)
        self.eden.clone(
            self.repo.path,
            empty_dir,
            backing_store=backing_store,
            filter_path=filter_path,
        )
        return Path(empty_dir)

    def assert_paths_filtered_unfiltered(
        self, repo: Path, filtered_paths: List[str], unfiltered_paths: List[str]
    ) -> None:
        for u in unfiltered_paths:
            self.assertTrue(
                os.path.exists(os.path.join(repo, u)),
                "unfiltered path should be present in the repo",
            )

        for f in filtered_paths:
            self.assertFalse(
                os.path.exists(os.path.join(repo, f)),
                "filtered path should not be present in the repo",
            )

    def hg_clone_filteredhg_repo_legacy(
        self, repo_name: str, filter_path: Optional[str] = None
    ) -> hgrepo.HgRepository:
        """
        Uses the old method of cloning FilteredFS repositories (setting a config value to true).
        Takes an optional filter_path to test using both FilteredFS cloning methods at once.
        """
        args = ["--config", "clone.use-eden-sparse=true"]
        if filter_path is not None:
            args.extend(["--config", f"clone.eden-sparse-filter={filter_path}"])

        return self.hg_clone_additional_repo(
            *args, backing_repo=self.backing_repo, client_name=repo_name
        )

    def hg_clone_filteredhg_repo(
        self, repo_name: str, filter_path: Optional[str] = ""
    ) -> hgrepo.HgRepository:
        """
        Uses the new method of cloning FilteredFS repositories (setting a string config value).
        The config works as follows
            - An empty string indicates that FilteredFS should be used, but no filter should be
              activated at clone time.
            - A non-empty string indicates that FilteredFS should be used, and the given filter
              should be activated.
            - None indicates that FilteredFS should not be used.

        This function assumes that FilteredFS should be used at all times and therefore always
        passes a config value.
        """
        return self.hg_clone_additional_repo(
            "--config",
            f"clone.eden-sparse-filter={filter_path}",
            backing_repo=self.backing_repo,
            client_name=repo_name,
        )

    def test_legacy_filteredhg_clone_succeeds(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo_legacy(repo_name="ffs")
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), [], ["foo", "bar", "filtered"]
        )
        ffs_repo.hg("filteredfs", "enable", "filter0")
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["foo", "filtered"], ["bar"]
        )

    def test_filteredhg_clone_succeeds(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(repo_name="ffs", filter_path="filter0")
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["foo", "filtered"], ["bar"]
        )

    def test_filteredhg_clone_succeeds_no_filter(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(repo_name="ffs")
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), [], ["bar", "foo", "filtered"]
        )

    def test_legacy_filteredhg_clone_with_filter(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo_legacy(
            repo_name="ffs", filter_path="tools/scm/filter/filter2"
        )
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["filtered"], ["foo", "bar"]
        )

    def test_eden_clone_succeeds(self) -> None:
        self.eden_clone_filteredhg_repo(backing_store="filteredhg")

    def test_eden_clone_with_filter_succeeds(self) -> None:
        repo_path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_path="tools/scm/filter/filter1"
        )
        self.assert_paths_filtered_unfiltered(repo_path, [], ["foo", "bar", "filtered"])

    def test_filter_active_after_eden_clone(self) -> None:
        repo_path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_path="tools/scm/filter/filter2"
        )
        self.assert_paths_filtered_unfiltered(repo_path, ["filtered"], ["foo", "bar"])

    def test_clone_filter_without_backing_store_arg_fails(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden_clone_filteredhg_repo(filter_path="tools/scm/filter/filter1")
        stderr = context.exception.stderr
        self.assertIn(
            "error: --filter-path can only be used with",
            stderr,
            msg="passing a filter without specifying filteredhg as the backing store should fail",
        )

    def test_eden_get_filter_empty(self) -> None:
        path = self.eden_clone_filteredhg_repo(backing_store="filteredhg")

        with self.get_thrift_client_legacy() as client:
            result = client.getCurrentSnapshotInfo(
                GetCurrentSnapshotInfoRequest(MountId(os.fsencode(path)))
            )
            self.assertEqual("null", result.filterId)

    def test_eden_get_filter(self) -> None:
        path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_path="tools/scm/filter/filter1"
        )

        with self.get_thrift_client_legacy() as client:
            result = client.getCurrentSnapshotInfo(
                GetCurrentSnapshotInfoRequest(MountId(os.fsencode(path)))
            )
            self.assertNotEqual(None, result.filterId)
            if result.filterId is not None:
                self.assertIn("tools/scm/filter/filter1", result.filterId)


@hg_test
# pyre-ignore[13]: T62487924
class NonFilteredTestCase(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("foo", "foo")
        repo.write_file("bar", "bar")
        repo.commit("Initial commit.")

    def eden_clone_filteredhg_repo(
        self, backing_store: Optional[str] = None, filter_path: Optional[str] = None
    ) -> Path:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(empty_dir)
        self.eden.clone(
            self.repo.path,
            empty_dir,
            backing_store=backing_store,
            filter_path=filter_path,
        )
        return Path(empty_dir)

    def test_eden_get_filter_nonfiltered(self) -> None:
        path = self.eden_clone_filteredhg_repo(backing_store="hg")

        with self.get_thrift_client_legacy() as client:
            result = client.getCurrentSnapshotInfo(
                GetCurrentSnapshotInfoRequest(MountId(os.fsencode(path)))
            )
            print(result)
            self.assertEqual(None, result.filterId)
