# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import os
import subprocess
from pathlib import Path
from typing import List, Optional, Tuple

from eden.fs.cli.hg_util import parse_config_output
from eden.fs.service.eden.thrift_types import GetCurrentSnapshotInfoRequest, MountId
from eden.integration.hg.lib.hg_extension_test_base import (
    EdenHgTestCase,
    filteredhg_test,
    FilteredHgTestCase,
    hg_test,
)
from eden.integration.lib import hgrepo


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSCloneBase(FilteredHgTestCase):
    """Clone FilteredFS repos using `hg clone`"""

    test_filter0: str = """
[exclude]
foo
filtered
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

    test_filter_foo: str = """
[metadata]
version: 2
required: true
[include]
*
[exclude]
foo
"""

    test_filter_bar: str = """
[metadata]
version: 2
required: true
[include]
*
[exclude]
bar
"""

    test_filter_baz: str = """
[metadata]
version: 2
required: true
[include]
*
[exclude]
baz
"""

    initial_commit: str = ""

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("filter0", self.test_filter0)
        repo.write_file("filter_foo", self.test_filter_foo)
        repo.write_file("filter_bar", self.test_filter_bar)
        repo.write_file("filter_baz", self.test_filter_baz)
        repo.write_file("tools/scm/filter/filter1", self.test_filter1)
        repo.write_file("tools/scm/filter/filter2", self.test_filter2)
        repo.write_file("foo", "foo")
        repo.write_file("bar", "bar")
        repo.write_file("baz", "baz")
        repo.write_file("filtered", "I should be filtered by filter2")
        self.initial_commit = repo.commit("Initial commit.")

    def _get_relative_filter_config_path(self) -> str:
        return os.path.join(".hg", "sparse")

    def eden_clone_filteredhg_repo(
        self,
        backing_store: Optional[str] = None,
        filter_paths: Optional[List[str]] = None,
    ) -> Path:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(empty_dir)
        self.eden.clone(
            self.repo.path,
            empty_dir,
            backing_store=backing_store,
            filter_paths=filter_paths,
        )
        return Path(empty_dir)

    def assert_paths_filtered_unfiltered(
        self, repo: Path, filtered_paths: List[str], unfiltered_paths: List[str]
    ) -> None:
        for u in unfiltered_paths:
            self.assertTrue(
                os.path.exists(os.path.join(repo, u)),
                f"unfiltered path {u} should be present in the repo",
            )

        for f in filtered_paths:
            self.assertFalse(
                os.path.exists(os.path.join(repo, f)),
                f"filtered path {f} should not be present in the repo",
            )

    async def assert_fid_matches(
        self, path: Path, commit: str, filter_paths: List[str]
    ) -> None:
        async with self.get_thrift_client() as client:
            result = await client.getCurrentSnapshotInfo(
                GetCurrentSnapshotInfoRequest(
                    mountId=MountId(mountPoint=os.fsencode(path))
                )
            )
            self.assertIsNotNone(result.fid)
            dbgfid_result = self.repo.run_hg(
                *[
                    "debugfilterid",
                    "-r",
                    commit,
                ]
                + filter_paths
            )
            self.assertEqual(result.fid, dbgfid_result.stdout)

    def hg_clone_filteredhg_repo(
        self,
        repo_name: str,
        filter_paths: Optional[List[Tuple[Optional[str], str]]] = None,
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
        config_args = []
        for config_key, filter_path in filter_paths or []:
            if config_key is None:
                # Use the legacy config option
                config_args += [
                    "--config",
                    f"clone.eden-sparse-filter={filter_path}",
                ]
            else:
                # Use the new/preferred way to specify filter paths
                config_args += [
                    "--config",
                    f"clone.eden-sparse-filter.{config_key}={filter_path}",
                ]

        return self.hg_clone_additional_repo(
            *config_args,
            backing_repo=self.backing_repo,
            client_name=repo_name,
        )

    def test_filteredhg_clone_succeeds_legacy_config(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(
            repo_name="ffs", filter_paths=[(None, "filter0")]
        )
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["foo", "filtered"], ["bar"]
        )

    def test_filteredhg_clone_succeeds_no_filter(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(repo_name="ffs", filter_paths=[])
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), [], ["bar", "foo", "filtered"]
        )

    def test_filteredhg_clone_one_filter(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(
            repo_name="ffs", filter_paths=[("foo", "filter_foo")]
        )
        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["foo"], ["bar", "baz"]
        )

    def test_filteredhg_clone_two_filters(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(
            repo_name="ffs",
            filter_paths=[
                ("foo", "filter_foo"),
                ("bar", "filter_bar"),
            ],
        )

        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["foo", "bar"], ["baz"]
        )

    def test_filteredhg_clone_two_filters_one_legacy(self) -> None:
        ffs_repo = self.hg_clone_filteredhg_repo(
            repo_name="ffs",
            filter_paths=[
                ("foo", "filter_foo"),
                ("bar", "filter_bar"),
                (None, "filter_baz"),
            ],
        )

        self.assert_paths_filtered_unfiltered(
            Path(ffs_repo.path), ["baz", "foo", "bar"], []
        )

    def test_eden_clone_succeeds(self) -> None:
        self.eden_clone_filteredhg_repo(backing_store="filteredhg")

    def test_eden_clone_multiple_filters(self) -> None:
        repo_path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg",
            filter_paths=["filter_foo", "filter_bar"],
        )
        self.assert_paths_filtered_unfiltered(repo_path, ["foo", "bar"], ["baz"])

    def test_eden_clone_with_filter_succeeds(self) -> None:
        repo_path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_paths=["tools/scm/filter/filter1"]
        )
        self.assert_paths_filtered_unfiltered(repo_path, [], ["foo", "bar", "filtered"])

    def test_filter_active_after_eden_clone(self) -> None:
        repo_path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_paths=["tools/scm/filter/filter2"]
        )
        self.assert_paths_filtered_unfiltered(repo_path, ["filtered"], ["foo", "bar"])

    def test_filter_file_contains_warning_eden_clone(self) -> None:
        repo_path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_paths=["tools/scm/filter/filter2"]
        )

        filter_warning = parse_config_output(
            self.hg("config", "sparse.filter-warning", "-Tjson", cwd=str(repo_path))
        )
        self.assertNotEqual(filter_warning, "")
        filter_file = repo_path / self._get_relative_filter_config_path()

        with open(filter_file, "r") as f:
            filter_config = f.read()
            self.assertIn(
                filter_warning,
                filter_config,
            )
            self.assertIn(
                "%include tools/scm/filter/filter2",
                filter_config,
            )

    def test_filter_file_contains_warning_hg_clone(self) -> None:
        repo = self.hg_clone_filteredhg_repo(
            repo_name="ffs", filter_paths=[(None, "tools/scm/filter/filter2")]
        )
        repo_path = Path(repo.path)

        filter_warning = parse_config_output(
            self.hg("config", "sparse.filter-warning", "-Tjson", cwd=str(repo_path))
        )
        self.assertNotEqual(filter_warning, "")
        filter_file = repo_path / self._get_relative_filter_config_path()

        with open(filter_file, "r") as f:
            filter_config = f.read()
            self.assertIn(
                filter_warning,
                filter_config,
            )
            self.assertIn(
                "%include tools/scm/filter/filter2",
                filter_config,
            )

    def test_filter_warning_remains_after_switch(self) -> None:
        repo = self.hg_clone_filteredhg_repo(
            repo_name="ffs", filter_paths=[(None, "tools/scm/filter/filter2")]
        )
        repo_path = Path(repo.path)

        filter_warning = parse_config_output(
            self.hg("config", "sparse.filter-warning", "-Tjson", cwd=str(repo_path))
        )
        self.assertNotEqual(filter_warning, "")
        filter_file = repo_path / self._get_relative_filter_config_path()

        with open(filter_file, "r") as f:
            filter_config = f.read()
            self.assertIn(
                filter_warning,
                filter_config,
            )
            self.assertIn(
                "%include tools/scm/filter/filter2",
                filter_config,
            )

        self.hg("filteredfs", "switch", "filter0", cwd=str(repo_path))
        with open(filter_file, "r") as f:
            filter_config = f.read()
            self.assertIn(
                filter_warning,
                filter_config,
            )
            self.assertIn(
                "%include filter0",
                filter_config,
            )

    def test_clone_filter_without_backing_store_arg_fails(self) -> None:
        with self.assertRaises(subprocess.CalledProcessError) as context:
            self.eden_clone_filteredhg_repo(filter_paths=["tools/scm/filter/filter1"])
        stderr = context.exception.stderr
        self.assertIn(
            "error: --filter-paths can only be used with",
            stderr,
            msg="passing a filter without specifying filteredhg as the backing store should fail",
        )

    async def test_eden_get_filter_empty(self) -> None:
        path = self.eden_clone_filteredhg_repo(backing_store="filteredhg")

        async with self.get_thrift_client() as client:
            result = await client.getCurrentSnapshotInfo(
                GetCurrentSnapshotInfoRequest(
                    mountId=MountId(mountPoint=os.fsencode(path))
                )
            )
            self.assertEqual(b"null", result.fid)

    async def test_eden_get_filter(self) -> None:
        filter_paths = ["tools/scm/filter/filter1"]
        path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg", filter_paths=filter_paths
        )
        await self.assert_fid_matches(path, self.initial_commit, filter_paths)

    async def test_eden_get_two_filters(self) -> None:
        filter_paths = ["tools/scm/filter/filter1", "tools/scm/filter/filter2"]
        path = self.eden_clone_filteredhg_repo(
            backing_store="filteredhg",
            filter_paths=filter_paths,
        )
        await self.assert_fid_matches(path, self.initial_commit, filter_paths)


@hg_test
# pyre-ignore[13]: T62487924
class NonFilteredTestCase(EdenHgTestCase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("foo", "foo")
        repo.write_file("bar", "bar")
        repo.commit("Initial commit.")

    def eden_clone_filteredhg_repo(
        self,
        backing_store: Optional[str] = None,
    ) -> Path:
        tmp = self.make_temporary_directory()
        empty_dir = os.path.join(tmp, "foo/bar/baz")
        os.makedirs(empty_dir)
        self.eden.clone(
            self.repo.path,
            empty_dir,
            backing_store=backing_store,
        )
        return Path(empty_dir)

    async def test_eden_get_filter_nonfiltered(self) -> None:
        path = self.eden_clone_filteredhg_repo(backing_store="hg")

        async with self.get_thrift_client() as client:
            result = await client.getCurrentSnapshotInfo(
                GetCurrentSnapshotInfoRequest(
                    mountId=MountId(mountPoint=os.fsencode(path))
                )
            )
            self.assertIsNone(result.fid)
