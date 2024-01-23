# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        # The original repo doesn't need to be setup since we will be cloning separate repos
        pass

    def clone_repo(
        self, repo_name: str
    ) -> Tuple[hgrepo.HgRepository, Optional[hgrepo.HgRepository]]:
        return self.hg_clone_additional_repo("--edensparse", client_name=repo_name)

    def test_filter_enable(self) -> None:
        with self.assertRaisesRegex(
            hgrepo.HgError, r".*option --edensparse not recognized.*"
        ):
            ffs_repo, backing_repo = self.clone_repo("ffs")
