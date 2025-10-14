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

    testV1Filter1: str = """
[metadata]
version: 2
required: true
[include]
*
[exclude]
bdir
[include]
bdir/README.md
"""

    testV1Filter2: str = """
[metadata]
version: 2
required: true
[include]
*
[exclude]
adir/file
"""

    initial_commit: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        # Directories that may contain filtered files
        repo.mkdir("dir2")

        # Files touched by no filters
        repo.write_file("hello", "hola\n")
        repo.write_file("adir/file", "foo!\n")
        repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test2\n")
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
        repo.write_file("filters/v1_filter1", self.testV1Filter1)
        repo.write_file("filters/v1_filter2", self.testV1Filter2)

        self.initial_commit = repo.commit("Initial commit.")

    def enable_filters(self, *paths: str) -> None:
        self.hg("filteredfs", "enable", *paths)

    def reset_filters(self) -> None:
        self.hg("filteredfs", "reset")

    def switch_filters(self, *paths: str) -> None:
        self.hg("filteredfs", "switch", *paths)

    def disable_filters(self, *paths: str) -> None:
        self.hg("filteredfs", "disable", *paths)

    def _get_relative_filter_config_path(self) -> str:
        return os.path.join(".hg", "sparse")

    def _read_file_from_repo(self, path: str) -> str:
        filename = self.repo.get_path(path)
        with open(filename, "r") as f:
            return f.read()

    def _path_exists_in_repo(self, path: str) -> bool:
        return os.path.exists(self.repo.get_path(path))

    def assert_filtered_and_unfiltered(
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

    def get_active_filter_paths(self) -> Set[str]:
        # The filter file should always exist when FilteredFS is enabled, so
        # any failure to read the filter file is a legit error.
        lines = self._read_file_from_repo(
            self._get_relative_filter_config_path()
        ).splitlines()
        return {line.removeprefix("%include ") for line in lines}

    def read_active_filters(self) -> Optional[Set[str]]:
        # Empty filter files are valid
        return {
            "" if filt == "" else self._read_file_from_repo(filt)
            for filt in self.get_active_filter_paths()
        }

    def show_active_filters(self) -> str:
        return self.hg("filteredfs", "show")


@filteredhg_test
# pyre-ignore[13]: T62487924
class FilteredFSBasic(FilteredFSBase):
    def test_filter_enable_and_switch(self) -> None:
        self.enable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), {"top_level_filter"})
        self.assertEqual(self.read_active_filters(), {self.testFilter1})

        # double activation does nothing
        self.enable_filters("top_level_filter")
        self.enable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), {"top_level_filter"})
        self.assertEqual(self.read_active_filters(), {self.testFilter1})

        # activating a different filter makes both active
        self.enable_filters("filters/v2")
        self.assertEqual(
            self.get_active_filter_paths(), {"top_level_filter", "filters/v2"}
        )
        self.assertEqual(
            self.read_active_filters(), {self.testFilter1, self.testFilterV2}
        )

        # A filter that's empty is still valid
        self.switch_filters("filters/empty_filter")
        self.assertEqual(self.get_active_filter_paths(), {"filters/empty_filter"})
        # If this filter is successfully turned on, then the repo will be empty
        # (since v2 profiles allow empty [include]). Therefore we can't compare
        # the filter contents.

        # Filters with only metadata are also valid
        self.switch_filters("filters/metadata_only")
        self.assertEqual(self.get_active_filter_paths(), {"filters/metadata_only"})
        # As mentioned above, this filter results in an empty repo. Therefore
        # no comparison can be done on the contents of the filter.

    def test_filter_disable(self) -> None:
        self.enable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), {"top_level_filter"})

        self.disable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), set())

        # A second `disable` does nothing
        self.disable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), set())

    def test_filter_reset(self) -> None:
        self.enable_filters("top_level_filter", "a/nested_filter_file")
        self.assertEqual(
            self.get_active_filter_paths(), {"top_level_filter", "a/nested_filter_file"}
        )
        self.reset_filters()
        self.assertEqual(self.get_active_filter_paths(), set())

        # Resetting a single active filter should work the same way
        self.enable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), {"top_level_filter"})
        self.reset_filters()
        self.assertEqual(self.get_active_filter_paths(), set())

    def test_filter_switch(self) -> None:
        self.assertEqual(self.get_active_filter_paths(), set())
        self.enable_filters("top_level_filter", "a/nested_filter_file")
        self.assertEqual(
            self.get_active_filter_paths(), {"top_level_filter", "a/nested_filter_file"}
        )
        self.switch_filters("filters/v1_filter1", "filters/v1_filter2")
        self.assertEqual(
            self.get_active_filter_paths(), {"filters/v1_filter1", "filters/v1_filter2"}
        )
        self.switch_filters("top_level_filter")

        # Switching from a single active filter should work the same way
        self.enable_filters("top_level_filter")
        self.assertEqual(self.get_active_filter_paths(), {"top_level_filter"})
        self.switch_filters("a/nested_filter_file", "filters/v1_filter1")
        self.assertEqual(
            self.get_active_filter_paths(),
            {"a/nested_filter_file", "filters/v1_filter1"},
        )

    def test_filter_enable_invalid_path(self) -> None:
        # Filters shouldn't have ":" in them
        with self.assertRaises(hgrepo.HgError):
            self.enable_filters("top:level:filter")

    def test_filtered_file_is_omitted(self) -> None:
        initial_files = {"foo"}
        filtered_files = initial_files.copy()

        # File exists initially
        self.assert_filtered_and_unfiltered(set(), initial_files)

        # File is omitted after enabling filter
        self.enable_filters("a/nested_filter_file")
        self.assert_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # File reappears after disabling filter
        self.reset_filters()
        self.assert_filtered_and_unfiltered(set(), initial_files)

    def test_filters_follow_v2_rules(self) -> None:
        initial_files = {"bdir", "bdir/README.md", "bdir/noexec.sh", "bdir/test.sh"}
        filtered_files = {"bdir/noexec.sh", "bdir/test.sh"}

        # Files exist initially
        self.assert_filtered_and_unfiltered(set(), initial_files)

        # Files are omitted after enabling filter
        self.enable_filters("filters/v2")
        self.assert_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

    def test_multiple_active_filters(self) -> None:
        initial_files = {
            "bdir",
            "bdir/README.md",
            "bdir/noexec.sh",
            "bdir/test.sh",
            "adir/file",
            "hello",
        }
        filtered_files = {"bdir/noexec.sh", "bdir/test.sh", "adir/file"}

        # Files exist initially
        self.assert_filtered_and_unfiltered(set(), initial_files)

        # Files are omitted after enabling filter
        self.enable_filters("filters/v1_filter1", "filters/v1_filter2")
        self.assert_filtered_and_unfiltered(
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
        self.assert_filtered_and_unfiltered(set(), initial_files)

        # Directory and children are omitted after enabling filter
        self.enable_filters("a/nested_filter_file")
        self.assert_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # Directory and children reappear after disabling filter
        self.reset_filters()
        self.assert_filtered_and_unfiltered(set(), initial_files)

    def test_some_children_filtered(self) -> None:
        initial_files = {"dir2", "dir2/README", "dir2/not_filtered"}
        filtered_files = {"dir2/README"}

        # Directory and children exist initially
        self.assert_filtered_and_unfiltered(set(), initial_files)

        # Only one child is omitted after enabling filter
        self.enable_filters("a/nested_filter_file")
        self.assert_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # All children reappear after disabling filter
        self.reset_filters()
        self.assert_filtered_and_unfiltered(set(), initial_files)

    def test_filter_shows_correct_include_exclude(self) -> None:
        self.assertEqual(self.show_active_filters(), "")

        self.enable_filters("top_level_filter")
        self.assertIn("~ top_level_filter", self.show_active_filters())

    def test_filtered_file_not_in_status(self) -> None:
        self.assert_status_empty()

        # write to a filtered file
        self.enable_filters("top_level_filter")
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
        self.enable_filters("top_level_filter")
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

    def test_enable_filters_dne(self) -> None:
        initial_files = {"foo", "dir2/README", "filtered_out", "dir2/not_filtered"}
        self.enable_filters("does_not_exist")
        self.assert_filtered_and_unfiltered(set(), initial_files)

    def test_checkout_old_commit(self) -> None:
        self.repo.write_file("new_filter", self.testFilter1)
        self.repo.commit("Add new filter")
        self.assert_status_empty()

        # Filtering works as normal for a new filter file
        initial_files = {"foo", "dir2/README", "filtered_out", "dir2/not_filtered"}
        filtered_files = {"foo", "dir2/README", "filtered_out"}
        self.enable_filters("new_filter")
        self.assert_filtered_and_unfiltered(
            filtered_files, initial_files.difference(filtered_files)
        )

        # Checking out a commit that's older than the commit that introduced the
        # filter will not fail; it will simply apply the null filter
        self.hg("update", self.initial_commit)
        self.assert_filtered_and_unfiltered(set(), initial_files)

    def test_ods_counters_exist(self) -> None:
        self.enable_filters("top_level_filter")
        counters = self.get_counters()
        expected_counters = [
            "edenffi.ffs.lookups",
            "edenffi.ffs.object_cache_misses",
        ]
        for ec in expected_counters:
            self.assertIn(ec, counters)

    def test_lookup_failure_counter(self) -> None:
        self.enable_filters("top_level_filter")
        counters = self.get_counters()
        self.assertNotIn("edenffi.ffs.lookup_failures", counters)
        self.enable_filters("does_not_exist")
        counters = self.get_counters()
        self.assertGreaterEqual(counters["edenffi.ffs.lookup_failures"], 1)

    def test_commit_filter_change(self) -> None:
        self.enable_filters("include_exclude_filter")
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
        self.assert_filtered_and_unfiltered(set(), {"dir2/not_filtered", "dir2/README"})

    def test_filtered_cat(self) -> None:
        initial_files = {
            "bdir",
            "bdir/README.md",
            "bdir/noexec.sh",
            "bdir/test.sh",
            "adir/file",
            "hello",
        }
        unfiltered_files = {"adir/file", "hello", "bdir", "bdir/README.md"}
        self.assert_filtered_and_unfiltered(set(), initial_files)

        self.enable_filters("filters/v1_filter1")
        self.assert_filtered_and_unfiltered(
            initial_files.difference(unfiltered_files), unfiltered_files
        )

        # cat should succeed for unfiltered files
        out = self.hg("cat", "adir/file")
        self.assertEqual(out, "foo!\n")

        # cat should fail for filtered files
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("cat", "bdir/test.sh")
        self.assertEqual(context.exception.returncode, 1)
        self.assertEqual(context.exception.stderr, b"")

        # Killswitch should prevent cat from respecting active filter
        out = self.hg("cat", "bdir/test.sh", "--config", "sparse.killsparsecat=true")
        self.assertEqual(out, "#!/bin/bash\necho test\n")

    # Ensures intersectmatch logic works correctly when provided both a file
    # matcher and sparsematcher
    def test_filtered_cat_from_subdirectory(self) -> None:
        initial_files = {
            "bdir",
            "bdir/README.md",
            "bdir/noexec.sh",
            "bdir/test.sh",
            "adir/file",
            "hello",
        }
        unfiltered_files = {"adir/file", "hello", "bdir", "bdir/README.md"}
        self.assert_filtered_and_unfiltered(set(), initial_files)

        out = self.hg("cat", "file", cwd=self.get_path("adir"))
        self.assertEqual(out, "foo!\n")

        self.enable_filters("filters/v1_filter1")
        self.assert_filtered_and_unfiltered(
            initial_files.difference(unfiltered_files), unfiltered_files
        )

        # cat should fail for filtered files
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("cat", "test.sh", cwd=self.get_path("bdir"))
        self.assertEqual(context.exception.returncode, 1)
        self.assertEqual(context.exception.stderr, b"")

    def test_filtered_grep(self) -> None:
        initial_files = {
            "bdir",
            "bdir/README.md",
            "bdir/noexec.sh",
            "bdir/test.sh",
            "adir/file",
            "hello",
        }
        unfiltered_files = {"adir/file", "hello", "bdir", "bdir/README.md"}
        self.assert_filtered_and_unfiltered(set(), initial_files)

        self.enable_filters("filters/v1_filter1")
        self.assert_filtered_and_unfiltered(
            initial_files.difference(unfiltered_files), unfiltered_files
        )

        # grep should "succeed" for unfiltered files
        # NOTE: it current returns status code 123 despite succeeding... I will
        # fix this in follow-up diffs
        out = self.hg("grep", "foo!", check=False)
        self.assertRegex(out, r"adir(/|\\)file:foo!\n")

        # Grep respects filters for local searches (with "grep"), but it won't
        # respect filters when searching with biggrep or similar tools. This is
        # difficult to test.
        out = self.hg("grep", "'echo test2'", check=False)
        self.assertEqual(out, "")

    def test_filtered_grep_from_subdirectory(self) -> None:
        initial_files = {
            "bdir",
            "bdir/README.md",
            "bdir/noexec.sh",
            "bdir/test.sh",
            "adir/file",
            "hello",
        }
        unfiltered_files = {"adir/file", "hello", "bdir", "bdir/README.md"}
        self.assert_filtered_and_unfiltered(set(), initial_files)

        self.enable_filters("filters/v1_filter1")
        self.assert_filtered_and_unfiltered(
            initial_files.difference(unfiltered_files), unfiltered_files
        )

        # grep should "succeed" for unfiltered files
        # NOTE: it current returns status code 123 despite succeeding... I will
        # fix this in follow-up diffs
        out = self.hg("grep", "foo!", "file", cwd=self.get_path("adir"), check=False)
        self.assertRegex(out, r"file:foo!\n")

        # Grep respects filters for local searches (with "grep"), but it won't
        # respect filters when searching with biggrep or similar tools. This is
        # difficult to test.
        out = self.hg(
            "grep", "'echo test2'", "noexec.sh", cwd=self.get_path("bdir"), check=False
        )
        self.assertEqual(out, "")


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
            "edenffi.ffs.object_cache_cleanups", 0
        )

        # Make an initial request to populate the cache
        self.enable_filters("top_level_filter")

        counters_intermediate = self.get_counters()
        intermediate_cache_cleanups = counters_intermediate.get(
            "edenffi.ffs.object_cache_cleanups", 0
        )
        self.assertEqual(initial_cache_cleanups, intermediate_cache_cleanups)

        # Wait for the TTL to expire (2 seconds + 15 second buffer for cleanup thread)
        time.sleep(17)

        # Make another request - this should be a cache miss since the entry expired
        # and the cleanup thread should have removed it
        self.enable_filters("top_level_filter")

        counters_final = self.get_counters()
        final_cache_cleanups = counters_final.get(
            "edenffi.ffs.object_cache_cleanups", 0
        )

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
            "edenffi.ffs.object_cache_cleanups", 0
        )

        # Make an initial request to populate the cache
        self.enable_filters("top_level_filter")

        # Wait for the TTL to expire (2 seconds + 15 second buffer for cleanup thread)
        time.sleep(17)

        # Make another request - this should be a cache miss since the entry expired
        # and the cleanup thread should have removed it
        self.enable_filters("top_level_filter")

        counters_final = self.get_counters()
        final_cache_cleanups = counters_final.get(
            "edenffi.ffs.object_cache_cleanups", 0
        )

        # We should see cache cleanups have occurred
        self.assertEqual(final_cache_cleanups, initial_cache_cleanups)
