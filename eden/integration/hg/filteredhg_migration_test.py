# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import abc
import json
import os
from pathlib import Path
from typing import Callable, Optional, TypeVar

from eden.fs.cli.config import get_snapshot, SNAPSHOT
from eden.fs.cli.util import MIGRATION_MARKER

from eden.integration.hg.lib.hg_extension_test_base import (
    EdenHgTestCase,
    FilteredHgTestCase,
)
from eden.integration.lib import hgrepo
from eden.integration.lib.hgrepo import HgError


T = TypeVar("T")


def duplicate_with_prior_commit(method: T) -> T:
    """
    Decorator to mark a test method for duplication with a prior dummy commit.
    The duplicate will be named with a '_duplicate' suffix.
    """
    method._duplicate_with_prior_commit = True  # type: ignore
    return method


def apply_test_duplicates(cls: type) -> type:
    """
    Class decorator that generates duplicate test cases for cases marked
    with @duplicate_with_prior_commit.
    The duplicate will create a dummy commit before running the original test.
    This is to enforce we have case 4 in SNAPSHOT file.
    """
    testcases_to_duplicate = []

    for name in dir(cls):
        if name.startswith("test_"):
            method = getattr(cls, name)
            if callable(method) and getattr(
                method, "_duplicate_with_prior_commit", False
            ):
                testcases_to_duplicate.append((name, method))

    for testcase_name, original_testcase in testcases_to_duplicate:

        def make_duplicate(orig):
            async def duplicate_wrapper(self):
                self.repo.write_file("dummy", "dummy\n")
                self.repo.commit("dummy commit.")
                await orig(self)

            return duplicate_wrapper

        duplicated_testcase = make_duplicate(original_testcase)
        duplicated_testcase.__name__ = f"{testcase_name}_duplicate"
        setattr(cls, f"{testcase_name}_duplicate", duplicated_testcase)

    return cls


class FilteredFSMigrationTestBase(EdenHgTestCase, metaclass=abc.ABCMeta):
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

    def filteredfs_readiness_check(self, mount_path: Path) -> Optional[str]:
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
        filter_config_file_path = os.path.join(str(mount_path), ".hg", "sparse")
        if not os.path.exists(filter_config_file_path):
            return f"filter config file '{filter_config_file_path}' does not exist"

        # check filter config file content, there should be entries populated
        lines = self.read_file(filter_config_file_path).splitlines()
        lines = {line.removeprefix("%include ") for line in lines}
        is_null_filter = len(lines) == 0  # empty config file means "null" filter

        # examine SNAPSHOT file to see if it has filter id
        client_dir = Path(self.eden.client_dir_for_mount(mount_path))
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

        # All checks passed, we think the repo is FilteredFS ready
        return None

    def assert_filteredfs_enabled(self, mount_path: Path) -> None:
        res = self.filteredfs_readiness_check(mount_path)
        assert res is None, f"filteredfs not enabled: {res}"

    def assert_filteredfs_disabled(self, mount_path: Path) -> None:
        res = self.filteredfs_readiness_check(mount_path)
        assert res is not None, "filteredfs should not be enabled"

    def assert_file_exists(self, path: str) -> None:
        assert os.path.exists(self.repo.get_path(path))

    def assert_filter_applied(self, mount_path: Path) -> None:
        assert not os.path.exists(os.path.join(str(mount_path), "adir/hidden"))

    def assert_filter_not_applied(self, mount_path: Path) -> None:
        assert os.path.exists(os.path.join(str(mount_path), "adir/hidden"))

    def add_file(self, path: str) -> None:
        assert not os.path.exists(self.repo.get_path(path)), f"{path} already exists"
        self.repo.write_file(path, "this is a new file\n")

    def add_dir(self, path: str) -> None:
        assert not os.path.exists(self.repo.get_path(path)), f"{path} already exists"
        os.mkdir(self.repo.get_path(path))

    def modify_file(self, path: str) -> None:
        assert os.path.exists(self.repo.get_path(path)), f"{path} does not exist"
        self.repo.write_file(path, "this is a modified file\n")

    def remove_file(self, path: str) -> None:
        assert os.path.exists(self.repo.get_path(path)), f"{path} does not exist"
        os.remove(self.repo.get_path(path))

    def remove_dir(self, path: str) -> None:
        assert os.path.exists(self.repo.get_path(path)), f"{path} does not exist"
        os.rmdir(self.repo.get_path(path))

    async def enable_config_for_edensparse_migration(self) -> None:
        # make sure edenfs picks up our updated config
        async with self.get_thrift_client() as client:
            # toggle config
            edenrc = os.path.join(self.home_dir, ".edenrc")
            self.write_configs(
                {"experimental": ["enable-edensparse-migration = true"]}, edenrc
            )
            await client.reloadConfig()

    def restart_edenfs_manually(self) -> None:
        self.eden.run_cmd("restart", "--yes", "--allow-root", cwd=self.mount)

    async def edensparse_migration_common(
        self,
        pre_migration: Callable[[], None],
        post_migration: Callable[[], None],
        mount_path: Optional[Path] = None,
        migration_did_happen: bool = True,
    ) -> None:
        mount_path = mount_path or self.mount_path
        if migration_did_happen:
            self.assert_filteredfs_disabled(mount_path)
            self.assert_filter_not_applied(mount_path)
        else:
            self.hg("filteredfs", "enable", "filter/test_filter", cwd=str(mount_path))
            self.assert_filteredfs_enabled(mount_path)
            self.assert_filter_applied(mount_path)

        pre_migration()

        await self.enable_config_for_edensparse_migration()

        # restart edenfs
        self.restart_edenfs_manually()

        # check the marker file existence
        # this should be checked before sapling checkout/rebase commands since
        # these commands would clean up the marker file when invoking EdenFS'
        # checkoutRevision Thrift API.
        marker_file_path = os.path.join(str(mount_path), ".hg", MIGRATION_MARKER)
        if migration_did_happen:
            assert os.path.exists(
                marker_file_path
            ), f"Migration marker file '{marker_file_path}' does not exist"
        else:
            assert not os.path.exists(
                marker_file_path
            ), f"Migration marker file '{marker_file_path}' should not exist"

        self.hg(
            "config",
            "--local",
            "clone.eden-sparse-filter.test",
            "filter/test_filter",
            cwd=str(mount_path),
        )

        self.hg("go", ".", cwd=str(mount_path))

        self.assert_filteredfs_enabled(mount_path)
        self.assert_filter_applied(mount_path)
        post_migration()


@apply_test_duplicates
class FilteredFSMigrationFromUnfilteredTest(
    FilteredFSMigrationTestBase, metaclass=abc.ABCMeta
):
    def test_filteredfs_disabled_init(self) -> None:
        self.assert_filteredfs_disabled(self.mount_path)
        self.assert_filter_not_applied(self.mount_path)

    @duplicate_with_prior_commit
    async def test_filteredfs_migration(self) -> None:
        await self.edensparse_migration_common(lambda: None, lambda: None)

    @duplicate_with_prior_commit
    async def test_empty_status(self) -> None:
        await self.edensparse_migration_common(
            self.assert_status_empty, self.assert_status_empty
        )

    @duplicate_with_prior_commit
    async def test_add_file(self) -> None:
        # regular file
        self.add_file("newfile")

        # tracked file
        self.add_file("newfile-tracked")
        self.hg("add", "newfile-tracked")

        # file under hidden dir
        self.add_file("adir/newfile")

        # tracked file under hidden dir
        self.add_file("adir/newfile-tracked")
        self.hg("add", "adir/newfile-tracked")

        def check_status_pre_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output
                == "A adir/newfile-tracked\nA newfile-tracked\n? adir/newfile\n? newfile\n"
            ), f"unexpected status output: {status_output}"

        def check_status_post_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "A newfile-tracked\n? newfile\n"
            ), f"unexpected status output: {status_output}"

        await self.edensparse_migration_common(
            check_status_pre_migration,
            check_status_post_migration,
        )

    @duplicate_with_prior_commit
    async def test_modify_file(self) -> None:
        # unhidden file
        self.modify_file("hello")

        def check_status_pre_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "M hello\n"
            ), f"unexpected status output: {status_output}"

        def check_status_post_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "M hello\n"
            ), f"unexpected status output: {status_output}"

        await self.edensparse_migration_common(
            check_status_pre_migration,
            check_status_post_migration,
        )

    @duplicate_with_prior_commit
    async def test_modify_hidden_file(self) -> None:
        self.modify_file("adir/file")

        def check_status_pre_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "M adir/file\n"
            ), f"unexpected status output: {status_output}"

        await self.edensparse_migration_common(
            check_status_pre_migration,
            lambda: None,
        )

    @duplicate_with_prior_commit
    async def test_delete_file(self) -> None:
        # unhidden file
        self.remove_file("hello")

        def check_status_pre_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "! hello\n"
            ), f"unexpected status output: {status_output}"

        def check_status_post_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "! hello\n"
            ), f"unexpected status output: {status_output}"

        await self.edensparse_migration_common(
            check_status_pre_migration,
            check_status_post_migration,
        )

    @duplicate_with_prior_commit
    async def test_delete_hidden_file(self) -> None:
        # hidden file
        self.remove_file("adir/file")

        def check_status_pre_migration() -> None:
            status_output = self.hg("status")
            assert (
                status_output == "! adir/file\n"
            ), f"unexpected status output: {status_output}"

        await self.edensparse_migration_common(
            check_status_pre_migration,
            lambda: None,
        )

    @duplicate_with_prior_commit
    async def test_migrate_freshly_cloned_nonfiltered_repo(self) -> None:
        cloned_repo = self.make_temporary_directory()
        self.eden.clone(self.repo.path, cloned_repo)

        await self.edensparse_migration_common(
            lambda: None, lambda: None, mount_path=Path(cloned_repo)
        )


# This test suite is intended for test cases which try to run edensparse
# migration on a repo which is already FilteredFS.
class FilteredFsMigrationFromFilteredTest(
    FilteredHgTestCase, FilteredFSMigrationTestBase, metaclass=abc.ABCMeta
):
    async def test_migrate_freshly_cloned_filtered_repo(self) -> None:
        cloned_repo = self.make_temporary_directory()
        self.eden.clone(
            self.repo.path,
            cloned_repo,
            backing_store="filteredhg",
            filter_paths=["filter/test_filter"],
        )
        self.hg("filteredfs", "enable", "filter/test_filter", cwd=cloned_repo)

        await self.edensparse_migration_common(
            lambda: None,
            lambda: None,
            mount_path=Path(cloned_repo),
            migration_did_happen=False,
        )

    async def test_edensparse_migration_for_filtered_repo(self) -> None:
        await self.edensparse_migration_common(
            lambda: None, lambda: None, migration_did_happen=False
        )
