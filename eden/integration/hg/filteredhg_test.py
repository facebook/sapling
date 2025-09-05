# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe


import abc
import configparser
import os
import time
from typing import Optional, Set

from eden.integration.hg.lib.hg_extension_test_base import (
    filteredhg_test,
    FilteredHgTestCase,
)
from eden.integration.lib import hgrepo


class FilteredFSBase(FilteredHgTestCase, metaclass=abc.ABCMeta):
    """Exercise some fundamental operations with filters enabled/disabled."""

    testFilterEmpty: str = ""

    testFilter1: str = """
[include]
*
[exclude]
foo
dir2/README
filtered_out
"""

    testFilterIncludeExclude: str = """
[metadata]
version: 2
[include]
*
[exclude]
dir2
[include]
dir2/README
"""

    testFilterOnlyMetadata: str = """
[metadata]
title: Test filter
description: Minimal filter for testing purposes
[include]
[exclude]
"""

    testFilterV2: str = """
[metadata]
version: 2
[include]
*
[exclude]
bdir
[include]
bdir/README.md
"""

    initial_commit: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        # Directories that may contain filtered files
        repo.mkdir("dir2")

        # Files touched by no filters
        repo.write_file("hello", "hola\n")
        repo.write_file("adir/file", "foo!\n")
        repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        repo.write_file("bdir/README.md", "This is a README file.\n")
        repo.write_file("dir2/not_filtered", "I shouldn't be filtered")
        repo.symlink("slink", os.path.join("adir", "file"))

        # Files/directories that are filtered out by testFilter1
        repo.write_file("foo", "bar\n")
        repo.write_file("dir2/README", "Please README!")
        repo.mkdir("filtered_out")
        repo.mkdir("filtered_out/a/truly/deeply/nested/directory")
        repo.write_file(
            "filtered_out/a/truly/deeply/nested/directory/file", "test_contents"
        )
        repo.write_file(
            "filtered_out/file", "I should be filtered if testFilter1 is active"
        )

        # Filter files that determine what is filtered
        repo.write_file("top_level_filter", self.testFilter1)
        repo.write_file("a/nested_filter_file", self.testFilter1)
        repo.write_file("include_exclude_filter", self.testFilterIncludeExclude)
        repo.write_file("filters/empty_filter", self.testFilterEmpty)
        repo.write_file("filters/metadata_only", self.testFilterOnlyMetadata)
        repo.write_file("filters/v2", self.testFilterV2)

        self.initial_commit = repo.commit("Initial commit.")

    def set_active_filter(self, path: str) -> None:
        self.hg("filteredfs", "enable", path)

    def remove_active_filter(self) -> None:
        self.hg("filteredfs", "disable")

    def _get_relative_filter_config_path(self) -> str:
        return os.path.join(".hg", "sparse")

    def _read_file_from_repo(self, path: str) -> str:
        filename = self.repo.get_path(path)
        with open(filename, "r") as f:
            return f.read()

    def _path_exists_in_repo(self, path: str) -> bool:
        return os.path.exists(self.repo.get_path(path))

    def ensure_filtered_and_unfiltered(
        self, filtered: Set[str], unfiltered: Set[str]
    ) -> None:
        for f in filtered:
            self.assertFalse(
                self._path_exists_in_repo(f),
                f"{f} is expected to be filtered but it is in the repo",
            )

        for u in unfiltered:
            self.assertTrue(
                self._path_exists_in_repo(u),
                f"{u} is expected to be unfiltered but it is not in the repo",
            )

    def get_active_filter_path(self) -> str:
        # The filter file should always exist when FilteredFS is enabled, so
        # any failure to read the filter file is a legit error.
        return self._read_file_from_repo(
            self._get_relative_filter_config_path()
        ).removeprefix("%include ")

    def read_active_filter(self) -> Optional[str]:
        active_filter = self.get_active_filter_path()
        # Empty filter files are valid
        return "" if active_filter == "" else self._read_file_from_repo(active_filter)

    def show_active_filter(self) -> str:
        return self.hg("filteredfs", "show")


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSBasic(FilteredFSBase):
    def test_filter_enable(self) -> None:
        self.set_active_filter("top_level_filter")
        self.assertEqual(self.get_active_filter_path(), "top_level_filter")
        self.assertEqual(self.read_active_filter(), self.testFilter1)

        # double activation does nothing
        self.set_active_filter("top_level_filter")
        self.set_active_filter("top_level_filter")
        self.assertEqual(self.get_active_filter_path(), "top_level_filter")
        self.assertEqual(self.read_active_filter(), self.testFilter1)

        # activating a different filter replaces the previous one
        self.set_active_filter("a/nested_filter_file")
        self.assertEqual(self.get_active_filter_path(), "a/nested_filter_file")
        self.assertEqual(self.read_active_filter(), self.testFilter1)

        # A filter that's empty is still valid
        self.set_active_filter("filters/empty_filter")
        self.assertEqual(self.get_active_filter_path(), "filters/empty_filter")
        # If this filter is successfully turned on, then the repo will be empty
        # (since v2 profiles allow empty [include]). Therefore we can't compare
        # the filter contents.

        # Filters with only metadata are also valid
        self.set_active_filter("filters/metadata_only")
        self.assertEqual(self.get_active_filter_path(), "filters/metadata_only")
        # As mentioned above, this filter results in an empty repo. Therefore
        # no comparison can be done on the contents of the filter.

    def test_filter_disable(self) -> None:
        self.set_active_filter("top_level_filter")
        self.assertEqual(self.get_active_filter_path(), "top_level_filter")

        self.remove_active_filter()
        self.assertEqual(self.get_active_filter_path(), "")

        # A second `disable` does nothing
        self.remove_active_filter()
        self.assertEqual(self.get_active_filter_path(), "")

    def test_filter_enable_invalid_path(self) -> None:
        # Filters shouldn't have ":" in them
        with self.assertRaises(hgrepo.HgError):
            self.set_active_filter("top:level:filter")

    def test_filtered_file_is_omitted(self) -> None:
        initial_files = {"foo"}
        filtered_files = initial_files.copy()

        # File exists initially
        self.ensure_filtered_and_unfiltered(set(), initial_files)

        # File is omitted after enabling filter
        self.set_active_filter("a/nested_filter_file")
        self.ensure_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # File reappears after disabling filter
        self.remove_active_filter()
        self.ensure_filtered_and_unfiltered(set(), initial_files)

    def test_filters_follow_v2_rules(self) -> None:
        initial_files = {"bdir", "bdir/README.md", "bdir/noexec.sh", "bdir/test.sh"}
        filtered_files = {"bdir/noexec.sh", "bdir/test.sh"}

        # Files exist initially
        self.ensure_filtered_and_unfiltered(set(), initial_files)

        # Files are omitted after enabling filter
        self.set_active_filter("filters/v2")
        self.ensure_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

    def test_entire_directory_is_omitted(self) -> None:
        initial_files = {
            "filtered_out",
            "filtered_out/file",
            "filtered_out/a/truly/deeply/nested/directory",
            "filtered_out/a/truly/deeply/nested/directory/file",
        }
        filtered_files = initial_files.copy()

        # Directory and children initially exist
        self.ensure_filtered_and_unfiltered(set(), initial_files)

        # Directory and children are omitted after enabling filter
        self.set_active_filter("a/nested_filter_file")
        self.ensure_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # Directory and children reappear after disabling filter
        self.remove_active_filter()
        self.ensure_filtered_and_unfiltered(set(), initial_files)

    def test_some_children_filtered(self) -> None:
        initial_files = {"dir2", "dir2/README", "dir2/not_filtered"}
        filtered_files = {"dir2/README"}

        # Directory and children exist initially
        self.ensure_filtered_and_unfiltered(set(), initial_files)

        # Only one child is omitted after enabling filter
        self.set_active_filter("a/nested_filter_file")
        self.ensure_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # All children reappear after disabling filter
        self.remove_active_filter()
        self.ensure_filtered_and_unfiltered(set(), initial_files)

    def test_filter_shows_correct_include_exclude(self) -> None:
        self.assertEqual(self.show_active_filter(), "")

        self.set_active_filter("top_level_filter")
        self.assertIn("~ top_level_filter", self.show_active_filter())

    def test_filtered_file_not_in_status(self) -> None:
        self.assert_status_empty()

        # write to a filtered file
        self.set_active_filter("top_level_filter")
        self.repo.write_file("foo", "a change")

        # Ensure the filtered file isn't reflected in status
        self.assert_status_empty()

    def test_filtered_merge(self) -> None:
        # Set up two commits that will conflict when rebased
        self.repo.write_file("foo", "a separate change\n")
        new1 = self.repo.commit("Change contents of foo")
        self.repo.update(self.initial_commit)
        self.repo.write_file("foo", "completely different change\n")
        new2 = self.repo.commit("Change contents of foo again")

        # enable the active filter so "foo" is filtered and attempt rebase
        self.set_active_filter("top_level_filter")
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("rebase", "-s", new1, "-d", new2)

        self.assertIn(
            b"conflicts while merging foo!",
            context.exception.stderr,
        )
        self.assert_unresolved(unresolved=["foo"])
        self.assert_status({"foo": "M"}, op="rebase")
        print(self.read_file("foo"))
        self.assert_file_regex(
            "foo",
            """\
            <<<<<<< .*
            completely different change
            =======
            a separate change
            >>>>>>> .*
            """,
        )

        self.repo.write_file("foo", "completely different change\na separate change")
        self.hg("resolve", "--mark", "foo")
        self.hg("rebase", "--continue")
        self.assertEqual(len(self.repo.log(revset="all()")), 3)

    def test_enable_filter_dne(self) -> None:
        initial_files = {"foo", "dir2/README", "filtered_out", "dir2/not_filtered"}
        self.set_active_filter("does_not_exist")
        self.ensure_filtered_and_unfiltered(set(), initial_files)

    def test_checkout_old_commit(self) -> None:
        self.repo.write_file("new_filter", self.testFilter1)
        self.repo.commit("Add new filter")
        self.assert_status_empty()

        # Filtering works as normal for a new filter file
        initial_files = {"foo", "dir2/README", "filtered_out", "dir2/not_filtered"}
        filtered_files = {"foo", "dir2/README", "filtered_out"}
        self.set_active_filter("new_filter")
        self.ensure_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # Checking out a commit that's older than the commit that introduced the
        # filter will not fail; it will simply apply the null filter
        self.hg("update", self.initial_commit)
        self.ensure_filtered_and_unfiltered(set(), initial_files)

    def test_ods_counters_exist(self) -> None:
        self.set_active_filter("top_level_filter")
        counters = self.get_counters()
        expected_counters = [
            "edenffi.ffs.lookups",
            "edenffi.ffs.repo_cache_misses",
        ]
        for ec in expected_counters:
            self.assertIn(ec, counters)

    def test_lookup_failure_counter(self) -> None:
        self.set_active_filter("top_level_filter")
        counters = self.get_counters()
        self.assertNotIn("edenffi.ffs.lookup_failures", counters)
        self.set_active_filter("does_not_exist")
        counters = self.get_counters()
        self.assertGreaterEqual(counters["edenffi.ffs.lookup_failures"], 1)

    def test_commit_filter_change(self) -> None:
        self.set_active_filter("include_exclude_filter")
        self.assert_status_empty()

        # Modify the active filter file
        filter_change = "dir2/not_filtered"
        new_filter_contents = self.testFilterIncludeExclude + filter_change
        self.write_file("include_exclude_filter", new_filter_contents)
        self.assert_status({"include_exclude_filter": "M"})
        self.repo.commit("Change contents of include_exclude_filter")

        # Status should be empty since the working copy reflects the changes
        # made to the filter file.
        self.assert_status_empty()

        # The newly unfiltered files should be unfiltered
        self.ensure_filtered_and_unfiltered(set(), {"dir2/not_filtered", "dir2/README"})


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSRepoCacheTest(FilteredFSBase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        super().populate_backing_repo(repo)

    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        super().apply_hg_config_variant(hgrc)
        hgrc["edenfs"] = {"ffs-repo-cache-ttl": "2s"}

    def test_repo_cache_eviction(self) -> None:
        """Test that repos are evicted from cache after TTL expires."""
        # Check initial counters
        counters_initial = self.get_counters()
        initial_cache_cleanups = counters_initial.get(
            "edenffi.ffs.repo_cache_cleanups", 0
        )

        # Make an initial request to populate the cache
        self.set_active_filter("top_level_filter")

        counters_intermediate = self.get_counters()
        intermediate_cache_cleanups = counters_intermediate.get(
            "edenffi.ffs.repo_cache_cleanups", 0
        )
        self.assertEqual(initial_cache_cleanups, intermediate_cache_cleanups)

        # Wait for the TTL to expire (2 seconds + 15 second buffer for cleanup thread)
        time.sleep(17)

        # Make another request - this should be a cache miss since the entry expired
        # and the cleanup thread should have removed it
        self.set_active_filter("top_level_filter")

        counters_final = self.get_counters()
        final_cache_cleanups = counters_final.get("edenffi.ffs.repo_cache_cleanups", 0)

        # We should see cache cleanups have occurred
        self.assertGreater(final_cache_cleanups, initial_cache_cleanups)


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSRepoCacheNeverExpiresTest(FilteredFSBase):
    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        super().populate_backing_repo(repo)

    def apply_hg_config_variant(self, hgrc: configparser.ConfigParser) -> None:
        super().apply_hg_config_variant(hgrc)
        hgrc["edenfs"] = {"ffs-repo-cache-ttl": "0s"}

    def test_repo_cache_eviction_never_expires(self) -> None:
        """Test that repos are evicted from cache after TTL expires."""
        # Check initial counters
        counters_initial = self.get_counters()
        initial_cache_cleanups = counters_initial.get(
            "edenffi.ffs.repo_cache_cleanups", 0
        )

        # Make an initial request to populate the cache
        self.set_active_filter("top_level_filter")

        # Wait for the TTL to expire (2 seconds + 15 second buffer for cleanup thread)
        time.sleep(17)

        # Make another request - this should be a cache miss since the entry expired
        # and the cleanup thread should have removed it
        self.set_active_filter("top_level_filter")

        counters_final = self.get_counters()
        final_cache_cleanups = counters_final.get("edenffi.ffs.repo_cache_cleanups", 0)

        # We should see cache cleanups have occurred
        self.assertEqual(final_cache_cleanups, initial_cache_cleanups)
