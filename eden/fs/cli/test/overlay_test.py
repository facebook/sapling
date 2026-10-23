#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import stat
from typing import Dict

from eden.test_support.testcase import EdenTestCaseBase
from facebook.eden.overlay.thrift_types import OverlayDir, OverlayEntry
from thrift.python.serializer import Protocol, serialize

from .. import overlay as overlay_mod


class OverlayExtractTest(EdenTestCaseBase):
    def setUp(self) -> None:
        super().setUp()
        overlay_dir = self.tmp_dir / "local"
        for shard in range(256):
            (overlay_dir / f"{shard:02x}").mkdir(parents=True)
        self.overlay = overlay_mod.Overlay(str(overlay_dir))
        self.output_dir = self.tmp_dir / "out"

    def write_file_inode(self, inode_number: int, contents: bytes) -> None:
        self.overlay._write_inode(
            inode_number, overlay_mod.OverlayHeader.TYPE_FILE, contents
        )

    def write_dir_inode(self, inode_number: int, children: Dict[str, int]) -> None:
        entries = {
            name: OverlayEntry(mode=stat.S_IFREG | 0o644, inodeNumber=child)
            for name, child in children.items()
        }
        self.overlay._write_inode(
            inode_number,
            overlay_mod.OverlayHeader.TYPE_DIR,
            serialize(OverlayDir(entries=entries), protocol=Protocol.COMPACT),
        )

    def overlay_file_exists(self, inode_number: int) -> bool:
        return os.path.exists(self.overlay.get_path(inode_number))

    def test_extract_file_without_remove(self) -> None:
        self.write_file_inode(5, b"hello")
        self.overlay.extract_file(5, self.output_dir / "five", stat.S_IFREG | 0o644)
        self.assertEqual(b"hello", (self.output_dir / "five").read_bytes())
        self.assertFalse(self.overlay_file_exists(5))

    def test_extract_file_with_remove(self) -> None:
        self.write_file_inode(5, b"hello")
        self.overlay.extract_file(
            5, self.output_dir / "five", stat.S_IFREG | 0o644, remove=True
        )
        self.assertEqual(b"hello", (self.output_dir / "five").read_bytes())
        self.assertFalse(self.overlay_file_exists(5))

    def test_extract_dir_without_remove(self) -> None:
        self.write_file_inode(7, b"seven")
        self.write_file_inode(8, b"eight")
        self.write_dir_inode(6, {"a": 7, "b": 8})
        self.overlay.extract_dir(6, self.output_dir)
        self.assertEqual(b"seven", (self.output_dir / "a").read_bytes())
        self.assertEqual(b"eight", (self.output_dir / "b").read_bytes())
        self.assertTrue(self.overlay_file_exists(6))
        self.assertFalse(self.overlay_file_exists(7))
        self.assertFalse(self.overlay_file_exists(8))

    def test_extract_dir_with_remove(self) -> None:
        self.write_file_inode(7, b"seven")
        self.write_file_inode(8, b"eight")
        self.write_dir_inode(6, {"a": 7, "b": 8})
        self.overlay.extract_dir(6, self.output_dir, remove=True)
        self.assertEqual(b"seven", (self.output_dir / "a").read_bytes())
        self.assertEqual(b"eight", (self.output_dir / "b").read_bytes())
        self.assertFalse(self.overlay_file_exists(6))
        self.assertFalse(self.overlay_file_exists(7))
        self.assertFalse(self.overlay_file_exists(8))
