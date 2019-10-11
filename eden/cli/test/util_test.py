#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import stat
import unittest

from facebook.eden.ttypes import TreeInodeDebugInfo, TreeInodeEntryDebugInfo

from .. import util


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
            path=b"some_path/d1",
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
        self.assertListEqual(read_files, [("some_path/d1/read_file", 300)])
        self.assertListEqual(written_files, [("some_path/d1/written_file", 400)])

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
            path=b"some_path/d1",
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
            path=b"some_path/d1",
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
        self.assertListEqual(read_files, [("some_path/d1/read_file", 300)])
        self.assertListEqual(written_files, [("some_path/d1/written_file", 400)])

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
