#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import sys
from pathlib import Path

from .lib import testcase

if sys.platform == "win32":
    try:
        from .lib.ntapi import (
            get_directory_entry_size,
            open_directory_handle,
            query_directory_file_ex,
        )
    except ImportError:
        # TODO(T150221518): We should add the ntapi extension module to the
        # getdeps build, but for now we have to account for the possibility that
        # it may not be present.
        pass


SL_RESTART_SCAN = 0x00000001
SL_RETURN_SINGLE_ENTRY = 0x00000002


@testcase.eden_repo_test
class ProjFSEnumeration(testcase.EdenRepoTest):
    """Test ProjFS-specific enumeration behavior.

    Basic directory listing behavior is tested in basic_test, but here we use
    Windows APIs to test ProjFS-specific enumeration behavior.

    """

    def setUp(self):
        super().setUp()

        # Compute the entry size as the base size of the struct plus extra space
        # for one more character, as our entries are two characters long.
        self.entry_size = get_directory_entry_size() + 2

        self.handle = open_directory_handle(str(Path(self.mount) / "somedir"))

    def populate_repo(self) -> None:
        self.repo.mkdir("somedir")
        self.repo.write_file("somedir/1a", "alligator\n")
        self.repo.write_file("somedir/1b", "buffalo\n")
        self.repo.write_file("somedir/2c", "cuttlefish\n")
        self.repo.write_file("somedir/2d", "dingo\n")
        self.repo.commit("Initial commit.")

    def test_return_single_entry(self):
        """Test SL_RETURN_SINGLE_ENTRY behavior"""

        self.assertEqual(
            ["."],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )
        self.assertEqual(
            [".."],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )
        self.assertEqual(
            ["1a"],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )
        self.assertEqual(
            ["1b"],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )
        self.assertEqual(
            ["2c"],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )
        self.assertEqual(
            ["2d"],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )
        self.assertEqual(
            [],
            query_directory_file_ex(
                self.handle, 16 * 1024, SL_RETURN_SINGLE_ENTRY, None
            ),
        )

    def test_single_entry_buffer(self):
        """Test querying with a buffer just large enough for a single entry

        This fails on NTFS, which doesn't produce the ".." entry ¯|_(ツ)_/¯

        """

        self.assertEqual(
            ["."], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )
        self.assertEqual(
            [".."], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )
        self.assertEqual(
            ["1a"], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )
        self.assertEqual(
            ["1b"], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )
        self.assertEqual(
            ["2c"], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )
        self.assertEqual(
            ["2d"], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )
        self.assertEqual(
            [], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )

    def test_restart_scan(self):
        """Test behavior when clients restart a scan"""

        self.assertEqual(
            [".", ".."],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )
        self.assertEqual(
            ["1a", "1b"],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )
        self.assertEqual(
            [".", ".."],
            query_directory_file_ex(
                self.handle, 2 * self.entry_size, SL_RESTART_SCAN, None
            ),
        )
        self.assertEqual(
            ["1a", "1b"],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )
        self.assertEqual(
            ["2c", "2d"],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )

    def test_filename_pattern(self):
        """Test FileName pattern matching

        If FileName is set, we should only return directory entries that match
        its pattern.

        """

        ents = []
        while True:
            batch = query_directory_file_ex(self.handle, 16 * 1024, 0, "2*")
            if not batch:
                break

            ents.extend(batch)

        self.assertEqual(["2c", "2d"], ents)

    def test_filename_pattern_once(self):
        """Test that FileName only needs to be specified once"""

        self.assertEqual(
            ["2c"], query_directory_file_ex(self.handle, self.entry_size, 0, "2*")
        )

        # FileName is taken from the first call to NtQueryDirectoryFileEx, so an
        # unspecified pattern on subsequent calls shouldn't stop the pattern
        # from being applied.
        self.assertEqual(
            ["2d"], query_directory_file_ex(self.handle, self.entry_size, 0, None)
        )

    def test_filename_pattern_changed(self):
        """Test that changing FileName partway through enumeration is a no-op"""

        self.assertEqual(
            ["2c"], query_directory_file_ex(self.handle, self.entry_size, 0, "2*")
        )

        # FileName is taken from the first call to NtQueryDirectoryFileEx, so a
        # changed pattern on subsequent calls shouldn't be applied.
        self.assertEqual(
            ["2d"], query_directory_file_ex(self.handle, self.entry_size, 0, "1*")
        )
        self.assertEqual(
            [], query_directory_file_ex(self.handle, self.entry_size, 0, "1*")
        )

    def test_filename_pattern_initially_empty(self):
        """Test that changing FileName is a no-op even if unset"""

        self.assertEqual(
            [".", ".."],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )
        # FileName is taken from the first call to NtQueryDirectoryFileEx, so a
        # changed pattern on subsequent calls shouldn't be applied.
        self.assertEqual(
            ["1a", "1b"],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, "zz*"),
        )
        self.assertEqual(
            ["2c", "2d"],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )
        self.assertEqual(
            [], query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None)
        )

    def test_filename_pattern_on_restart(self):
        """Test that a new FileName pattern is applied when restarting scan

        It's actually a little unclear what is correct here. A plain reading of
        Microsoft's documentation suggests that we shouldn't apply a new
        FileName in a call with SL_RESTART_SCAN, as it says the following with
        no carve-outs for restarts:

        > The FileName is used as a search expression and is captured on the
        > very first call to NtQueryDirectoryFile for a given handle. Subsequent
        > calls to NtQueryDirectoryFile will use the search expression set in the
        > first call. The FileName parameter passed to subsequent calls will be
        > ignored.

        However, NTFS does apply a new FileName provided during a scan restart.
        So this test asserts that we match NTFS's behavior, which is probably
        what clients will expect.

        """
        self.assertEqual(
            [".", ".."],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )
        self.assertEqual(
            ["1a", "1b"],
            query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None),
        )

        # We should apply a new FileName pattern on scan restart.
        self.assertEqual(
            ["2c", "2d"],
            query_directory_file_ex(
                self.handle, 2 * self.entry_size, SL_RESTART_SCAN, "2*"
            ),
        )
        self.assertEqual(
            [], query_directory_file_ex(self.handle, 2 * self.entry_size, 0, None)
        )


@testcase.eden_repo_test
class ProjFSEnumerationInsufficientBuffer(testcase.EdenRepoTest):
    """Test that we handle filling the ProjFS buffer correctly

    When enumerating many directory entries, we'll eventually fill the ProjFS
    buffer and get an ERROR_INSUFFICIENT_BUFFER.  This tests indirectly that
    don't drop entries when handling this.

    """

    def setUp(self):
        self.filenames = []
        for i in range(1000):
            self.filenames.append("file-{:08}".format(i))

        super().setUp()

        self.handle = open_directory_handle(str(Path(self.mount) / "lots"))

    def populate_repo(self) -> None:
        for filename in self.filenames:
            self.repo.write_file("lots/" + filename, "x\n")
        self.repo.commit("Initial commit.")

    def test_many_directory_entries(self):
        handle = open_directory_handle(str(Path(self.mount) / "lots"))
        queried_filenames = []
        while True:
            query_result = query_directory_file_ex(handle, 16 * 1024, 0, None)
            if not query_result:
                break

            queried_filenames.extend(query_result)

        self.assertEqual([".", ".."] + self.filenames, queried_filenames)
