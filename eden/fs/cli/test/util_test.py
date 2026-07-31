#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import json
import os
import stat
import tempfile
import unittest
from pathlib import Path
from typing import Optional

from eden.fs.service.eden.thrift_types import (
    TreeInodeDebugInfo,
    TreeInodeEntryDebugInfo,
)

from .. import rage, util


class UtilTest(unittest.TestCase):
    def test_is_valid_sha1(self) -> None:
        def is_valid(sha1: str) -> bool:
            return util.is_valid_sha1(sha1)

        self.assertTrue(is_valid("0123456789abcabcabcd0123456789abcabcabcd"))
        self.assertTrue(is_valid("0" * 40))

        self.assertFalse(is_valid("0123456789abcabcabcd0123456789abcabcabc"))
        self.assertFalse(is_valid("z123456789abcabcabcd0123456789abcabcabcd"))
        self.assertFalse(is_valid(""))
        self.assertFalse(is_valid("abc"))
        self.assertFalse(is_valid("z" * 40))

    INODE_RESULTS_0 = [
        TreeInodeDebugInfo(
            inodeNumber=1,
            path=os.path.join("some_path", "d1").encode(),
            materialized=True,
            treeHash=b"abc",
            entries=[
                TreeInodeEntryDebugInfo(
                    name=b"read_file",
                    inodeNumber=2,
                    mode=stat.S_IFREG,
                    loaded=True,
                    materialized=False,
                    hash=b"1abc",
                    fileSize=300,
                ),
                TreeInodeEntryDebugInfo(
                    name=b"written_file",
                    inodeNumber=3,
                    mode=stat.S_IFREG,
                    loaded=True,
                    materialized=True,
                    fileSize=400,
                ),
            ],
            refcount=0,
        )
    ]

    def test_read_write_separation(self) -> None:
        read_files, written_files = util.split_inodes_by_operation_type(
            self.INODE_RESULTS_0
        )
        self.assertListEqual(
            read_files, [(os.path.join("some_path", "d1", "read_file"), 300)]
        )
        self.assertListEqual(
            written_files, [(os.path.join("some_path", "d1", "written_file"), 400)]
        )

    INODE_RESULTS_1 = [
        TreeInodeDebugInfo(
            inodeNumber=1,
            path=b"some_path/d1",
            materialized=True,
            treeHash=b"abc",
            entries=[
                TreeInodeEntryDebugInfo(
                    name=b"read_file",
                    inodeNumber=2,
                    mode=stat.S_IFLNK,
                    loaded=True,
                    materialized=False,
                    hash=b"1abc",
                    fileSize=300,
                ),
                TreeInodeEntryDebugInfo(
                    name=b"written_file",
                    inodeNumber=3,
                    mode=stat.S_IFDIR,
                    loaded=True,
                    materialized=True,
                    fileSize=400,
                ),
            ],
            refcount=0,
        )
    ]

    def test_ignore_symlinks_and_directories(self) -> None:
        read_files, written_files = util.split_inodes_by_operation_type(
            self.INODE_RESULTS_1
        )
        self.assertListEqual(read_files, [])
        self.assertListEqual(written_files, [])

    INODE_RESULTS_2 = [
        TreeInodeDebugInfo(
            inodeNumber=1,
            path=os.path.join("some_path", "d1").encode(),
            materialized=True,
            treeHash=b"abc",
            entries=[
                TreeInodeEntryDebugInfo(
                    name=b"read_file",
                    inodeNumber=2,
                    mode=stat.S_IFREG,
                    loaded=True,
                    materialized=False,
                    hash=b"1abc",
                    fileSize=300,
                )
            ],
            refcount=0,
        ),
        TreeInodeDebugInfo(
            inodeNumber=3,
            path=os.path.join("some_path", "d1").encode(),
            materialized=True,
            treeHash=b"abc",
            entries=[
                TreeInodeEntryDebugInfo(
                    name=b"written_file",
                    inodeNumber=4,
                    mode=stat.S_IFREG,
                    loaded=True,
                    materialized=True,
                    fileSize=400,
                )
            ],
            refcount=0,
        ),
    ]

    def test_multiple_trees(self) -> None:
        read_files, written_files = util.split_inodes_by_operation_type(
            self.INODE_RESULTS_2
        )
        self.assertListEqual(
            read_files, [(os.path.join("some_path", "d1", "read_file"), 300)]
        )
        self.assertListEqual(
            written_files, [(os.path.join("some_path", "d1", "written_file"), 400)]
        )

    INODE_RESULTS_3 = [
        TreeInodeDebugInfo(
            inodeNumber=1,
            path=b"some_path/d1",
            materialized=True,
            treeHash=b"abc",
            entries=[
                TreeInodeEntryDebugInfo(
                    name=b"read_file",
                    inodeNumber=2,
                    mode=stat.S_IFREG,
                    loaded=False,
                    materialized=False,
                    hash=b"1abc",
                    fileSize=300,
                ),
                TreeInodeEntryDebugInfo(
                    name=b"written_file",
                    inodeNumber=3,
                    mode=stat.S_IFREG,
                    loaded=False,
                    materialized=True,
                    fileSize=400,
                ),
            ],
            refcount=0,
        )
    ]

    def test_ignore_unloaded(self) -> None:
        read_files, written_files = util.split_inodes_by_operation_type(
            self.INODE_RESULTS_3
        )
        self.assertListEqual(read_files, [])
        self.assertListEqual(written_files, [])


class CheckArcrcAuthTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp_dir.cleanup)
        self.arcrc = Path(self._tmp_dir.name) / ".arcrc"

    def check(self) -> Optional[str]:
        return util.check_arcrc_auth(self.arcrc)

    def test_valid(self) -> None:
        self.arcrc.write_text(
            json.dumps(
                {"hosts": {"https://phabricator.internmc.facebook.com/api/": {}}}
            )
        )
        self.assertIsNone(self.check())

    def test_missing_file(self) -> None:
        problem = self.check()
        assert problem is not None
        self.assertIn("does not exist", problem)

    def test_empty_file(self) -> None:
        self.arcrc.write_text("")
        problem = self.check()
        assert problem is not None
        self.assertIn("is empty", problem)

    def test_whitespace_only_file(self) -> None:
        self.arcrc.write_text("\n  \n")
        problem = self.check()
        assert problem is not None
        self.assertIn("is empty", problem)

    def test_invalid_json(self) -> None:
        self.arcrc.write_text('{"hosts": ')
        problem = self.check()
        assert problem is not None
        self.assertIn("does not contain valid JSON", problem)

    def test_json_not_an_object(self) -> None:
        self.arcrc.write_text("[]")
        problem = self.check()
        assert problem is not None
        self.assertIn("does not contain a JSON object", problem)

    def test_missing_hosts(self) -> None:
        self.arcrc.write_text(json.dumps({"config": {}}))
        problem = self.check()
        assert problem is not None
        self.assertIn("no `hosts` credentials", problem)

    def test_empty_hosts(self) -> None:
        self.arcrc.write_text(json.dumps({"hosts": {}}))
        problem = self.check()
        assert problem is not None
        self.assertIn("no `hosts` credentials", problem)


class ReporterNeedsArcAuthTest(unittest.TestCase):
    def test_arc_authed_reporters(self) -> None:
        for processor in (
            'pastry --title "eden rage from host"',
            "/usr/local/bin/pastry",
            "jf paste",
            "arc paste",
            "pastry.exe --title foo",
        ):
            with self.subTest(processor=processor):
                self.assertTrue(rage.reporter_needs_arc_auth(processor))

    def test_other_reporters(self) -> None:
        for processor in ("", "   ", "cat", "/bin/tee /tmp/rage.txt"):
            with self.subTest(processor=processor):
                self.assertFalse(rage.reporter_needs_arc_auth(processor))

    def test_check_skipped_for_non_arc_reporter(self) -> None:
        self.assertIsNone(rage.check_rage_reporter_auth("cat"))
