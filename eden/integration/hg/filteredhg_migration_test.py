# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import abc
import json
import os
from pathlib import Path
from typing import Optional

from eden.fs.cli.config import get_snapshot, SNAPSHOT

from eden.fs.cli.util import MIGRATION_MARKER

from eden.integration.hg.lib.hg_extension_test_base import EdenHgTestCase
from eden.integration.lib import hgrepo
from eden.integration.lib.hgrepo import HgError


class FilteredFSMigrationTest(EdenHgTestCase, metaclass=abc.ABCMeta):
    SAMPLE_FILTER_FILE: str = """
[metadata]
version: 2
required: true
[include]
*
[exclude]
adir
"""

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello", "hola\n")
        repo.write_file("world", "mundo\n")
        repo.write_file("subdir/ok", "ok\n")
        repo.write_file("subdir/bad", "bad\n")
        repo.write_file("subdir/ok2", "ok2\n")
        repo.write_file(
            "filter/test_filter", self.SAMPLE_FILTER_FILE
        )  # filter to hide 'adir/file'
        repo.write_file("adir/file", "file\n")
        repo.write_file("adir/hidden", "YOU_SHOULD_NOT_SEE_ME\n")
        repo.commit("Initial commit.")

    def get_scm_type(self) -> str:
        stdout = self.eden.run_cmd("info", cwd=self.mount)
        info_dict = json.loads(stdout)
        return info_dict.get("scm_type")

    def filteredfs_readiness_check(self) -> Optional[str]:
        """
        Checks if the repository is ready for FilteredFS by verifying:
        - The existence of the filter config file.
        - The SNAPSHOT file contains a valid filter id.
        - The existence of 'edensparse' in .hg/requires file.
        - The existence of the marker file: '.hg/edensparse_migration'
        - The 'filteredfs' command is available.
        - The SCM type is 'filteredhg'.

        Returns:
            None if all checks pass (FilteredFS is ready), otherwise a string
            describing the reason why FilteredFS is not enabled.
        """

        # check existence of filter config file
        filter_config_file_path = os.path.join(self.mount, ".hg", "sparse")
        if not os.path.exists(filter_config_file_path):
            return f"filter config file '{filter_config_file_path}' does not exist"

        # check filter config file content, there should be entries populated
        lines = self.read_file(filter_config_file_path).splitlines()
        lines = {line.removeprefix("%include ") for line in lines}
        is_null_filter = len(lines) == 0  # empty config file means "null" filter

        # examine SNAPSHOT file to see if it has filter id
        client_dir = Path(self.eden.client_dir_for_mount(self.mount_path))
        scm_type = self.get_scm_type()
        snapshot_state = get_snapshot(client_dir / SNAPSHOT, scm_type)
        if snapshot_state.last_filter_id is None:
            return "SNAPSHOT file with no filter id"
        if is_null_filter and snapshot_state.last_filter_id != b"null":
            return "filter id in SNAPSHOT file should be 'null'"

        # `sl filteredfs` command should be available by now
        try:
            self.hg("filteredfs", "--help")
        except HgError as e:
            assert (
                b"unknown command 'filteredfs'" in e.stderr
            ), f"unexpected exception: {e}"
            return "sapling does not know about 'filteredfs' command"

        # run `eden info` and check the backing store type
        if (scm_type := self.get_scm_type()) != "filteredhg":
            return f"scm_type = {scm_type}"

        # check the marker file existence
        marker_file_path = os.path.join(self.mount, ".hg", MIGRATION_MARKER)
        if not os.path.exists(marker_file_path):
            return f"Migration marker file '{marker_file_path}' does not exist"

        # All checks passed, we think the repo is FilteredFS ready
        return None

    def assert_filteredfs_enabled(self) -> None:
        res = self.filteredfs_readiness_check()
        assert res is None, f"filteredfs not enabled: {res}"

    def assert_filteredfs_disabled(self) -> None:
        res = self.filteredfs_readiness_check()
        assert res is not None, "filteredfs should not be enabled"

    def assert_file_exists(self, path: str) -> None:
        assert os.path.exists(self.repo.get_path(path))

    def assert_filter_applied(self) -> None:
        assert not os.path.exists(self.repo.get_path("adir/hidden"))

    def assert_filter_not_applied(self) -> None:
        assert os.path.exists(self.repo.get_path("adir/hidden"))

    def test_filteredfs_disabled(self) -> None:
        self.assert_filteredfs_disabled()
        self.assert_filter_not_applied()
