# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import subprocess
from pathlib import Path
from typing import List, Optional, Tuple

from eden.integration.hg.lib.hg_extension_test_base import (
    filteredhg_test,
    FilteredHgTestCase,
)

from eden.integration.lib import hgrepo


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSCloneBase(FilteredHgTestCase):
    """Clone FilteredFS repos using `hg clone`"""

    filter_contents: str = """
[exclude]
foo
[include]
bar
"""

    test_filter: str = """
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
        repo.write_file("tools/scm/filter/test", self.test_filter)
        repo.write_file("tools/scm/filter/test2", self.test_filter2)
        repo.write_file("foo", "foo")
        repo.write_file("bar", "bar")
        repo.write_file("filtered", "I should be filtered by filter2")
        repo.commit("Initial commit.")

    def clone_eden_repo(
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

    def assert_eden_paths_filtered_unfiltered(
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

    def clone_hg_repo(
        self, repo_name: str
    ) -> Tuple[hgrepo.HgRepository, Optional[hgrepo.HgRepository]]:
        return self.hg_clone_additional_repo(
            "--config", "clone.use-eden-sparse=true", client_name=repo_name
        )

    def test_hg_clone_succeeds(self) -> None:
        ffs_repo, backing_repo = self.clone_hg_repo("ffs")
        ffs_repo.write_file("foo", contents="bar")
        ffs_repo.write_file("bar", contents="baz")
        ffs_repo.write_file("filter", contents=self.filter_contents)
        ffs_repo.commit("Initial commit")
        self.assertTrue(os.path.exists(ffs_repo.get_path("bar")))
        self.assertTrue(os.path.exists(ffs_repo.get_path("foo")))
        ffs_repo.hg("filteredfs", "enable", "filter")
        self.assertTrue(os.path.exists(ffs_repo.get_path("bar")))
        self.assertFalse(os.path.exists(ffs_repo.get_path("foo")))

    def test_eden_clone_succeeds(self) -> None:
        self.clone_eden_repo(backing_store="filteredhg")

    def test_eden_clone_with_filter_succeeds(self) -> None:
        repo_path = self.clone_eden_repo(
            backing_store="filteredhg", filter_path="tools/scm/filter/test"
        )
        self.assert_eden_paths_filtered_unfiltered(
            repo_path, [], ["foo", "bar", "filtered"]
        )

    def test_filter_active_after_eden_clone(self) -> None:
        repo_path = self.clone_eden_repo(
            backing_store="filteredhg", filter_path="tools/scm/filter/test2"
        )
        self.assert_eden_paths_filtered_unfiltered(
            repo_path, ["filtered"], ["foo", "bar"]
        )

    def test_clone_filter_without_backing_store_arg_fails(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.clone_eden_repo(filter_path="tools/scm/filter/test")
        stderr = context.exception.stderr
        self.assertIn(
            "error: --filter-path can only be used with",
            stderr,
            msg="passing a filter without specifying filteredhg as the backing store should fail",
        )
