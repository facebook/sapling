# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import os
from typing import Optional

from eden.integration.hg.lib.hg_extension_test_base import FilteredHgTestCase

from eden.integration.lib import hgrepo


# pyre-ignore[13]: T62487924
class FilteredFSBase(FilteredHgTestCase):
    """Exercise some fundamental operations with filters enabled/disabled."""

    testFilterNull: str = ""

    testFilter1: str = """
[include]
*
[exclude]
foo
dir2/README
filtered_out
"""

    testFilterOnlyMetadata: str = """
[metadata]
title: Test filter
description: Minimal filter for testing purposes
[include]
[exclude]
"""

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello", "hola\n")
        repo.write_file("adir/file", "foo!\n")
        repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        repo.symlink("slink", os.path.join("adir", "file"))

        # Create some filter files
        repo.write_file("top_level_filter", self.testFilter1)
        repo.write_file("a/nested_filter_file", self.testFilter1)
        repo.write_file("filters/null_filter", self.testFilterNull)
        repo.write_file("filters/metadata_only", self.testFilterOnlyMetadata)

        repo.commit("Initial commit.")

    def set_active_filter(self, path: str):
        self.hg("filteredfs", "enable", path)

    def remove_active_filter(self):
        self.hg("filteredfs", "disable")

    def _get_relative_filter_config_path(self) -> str:
        return os.path.join(".hg", "sparse")

    def _read_file_from_repo(self, path: str) -> str:
        filename = self.repo.get_path(path)
        with open(filename, "r") as f:
            return f.read()

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

    def test_filter_enable(self) -> None:
        self.set_active_filter("top_level_filter")
        self.assertEqual(self.get_active_filter_path(), "top_level_filter")
        self.assertEqual(self.read_active_filter(), self.testFilter1)

        self.set_active_filter("a/nested_filter_file")
        self.assertEqual(self.get_active_filter_path(), "a/nested_filter_file")
        self.assertEqual(self.read_active_filter(), self.testFilter1)

        self.set_active_filter("filters/null_filter")
        self.assertEqual(self.get_active_filter_path(), "filters/null_filter")
        self.assertEqual(self.read_active_filter(), self.testFilterNull)

        self.set_active_filter("filters/metadata_only")
        self.assertEqual(self.get_active_filter_path(), "filters/metadata_only")
        self.assertEqual(self.read_active_filter(), self.testFilterOnlyMetadata)

    def test_filter_disable(self) -> None:
        self.set_active_filter("top_level_filter")
        self.assertEqual(self.get_active_filter_path(), "top_level_filter")

        self.remove_active_filter()
        self.assertEqual(self.get_active_filter_path(), "")

    def test_filter_enable_invalid_path(self) -> None:
        # Filters shouldn't have ":" in them
        with self.assertRaises(hgrepo.HgError):
            self.set_active_filter("top:level:filter")

    def test_filter_shows_correct_include_exclude(self) -> None:
        with self.assertRaises(hgrepo.HgError):
            self.show_active_filter()
        # I will actually write the logic once `hg filter show` is implemented

    # Future test cases:
    # - Enable multiple filters at once just enables the second filter
    # - Disabling a filter that isn't enabled does nothing
    # - Enabling a filter that is already enabled does nothing
    # - Reading a filtered file fails
    # - All sorts of filter edgecases
