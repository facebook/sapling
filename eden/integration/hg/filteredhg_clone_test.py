# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import Optional, Tuple

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

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        # The original repo doesn't need to be setup since we will be cloning separate repos
        pass

    def clone_repo(
        self, repo_name: str
    ) -> Tuple[hgrepo.HgRepository, Optional[hgrepo.HgRepository]]:
        return self.hg_clone_additional_repo(
            "--config", "clone.use-eden-sparse=true", client_name=repo_name
        )

    def test_clone_succeeds(self) -> None:
        ffs_repo, backing_repo = self.clone_repo("ffs")
        ffs_repo.write_file("foo", contents="bar")
        ffs_repo.write_file("bar", contents="baz")
        ffs_repo.write_file("filter", contents=self.filter_contents)
        ffs_repo.commit("Initial commit")
        self.assertTrue(os.path.exists(ffs_repo.get_path("bar")))
        self.assertTrue(os.path.exists(ffs_repo.get_path("foo")))
        ffs_repo.hg("filteredfs", "enable", "filter")
        self.assertTrue(os.path.exists(ffs_repo.get_path("bar")))
        self.assertFalse(os.path.exists(ffs_repo.get_path("foo")))
