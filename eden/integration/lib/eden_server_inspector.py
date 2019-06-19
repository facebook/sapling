#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import Iterable

import eden.thrift
from facebook.eden.ttypes import TimeSpec

from . import edenclient


"""Utilities for inspecting the state of the Eden server via Thrift.

This utility is parameterized by a specific mount point so that it need not be
specified for each instance method.
"""


class EdenServerInspector(object):
    def __init__(self, eden: edenclient.EdenFS, mount_point: str) -> None:
        self._eden = eden
        self._mount_point = mount_point

    def create_thrift_client(self) -> eden.thrift.EdenClient:
        return self._eden.get_thrift_client()

    def unload_inode_for_path(self, path: str = "") -> None:
        """path: relative path to a directory under the mount."""
        with self.create_thrift_client() as client:
            client.unloadInodeForPath(
                os.fsencode(self._mount_point), os.fsencode(path), age=TimeSpec(0, 0)
            )

    def get_inode_count(self, path: str = "") -> int:
        """path: relative path to a directory under the mount.

        Use '' for the root. Note that this will include the inode count for
        the root .hg and .eden entries.
        """
        with self.create_thrift_client() as client:
            debug_info = client.debugInodeStatus(
                os.fsencode(self._mount_point), os.fsencode(path)
            )
        count = 0
        for tree_inode_debug_info in debug_info:
            count += sum(1 for entry in tree_inode_debug_info.entries if entry.loaded)
        return count

    def get_paths_for_inodes(self, path: str = "") -> Iterable[str]:
        """path: relative path to a directory under the mount."""
        with self.create_thrift_client() as client:
            debug_info = client.debugInodeStatus(
                os.fsencode(self._mount_point), os.fsencode(path)
            )
        for tree_inode_debug_info in debug_info:
            parent_dir = tree_inode_debug_info.path.decode("utf-8")
            for entry in tree_inode_debug_info.entries:
                if entry.loaded:
                    yield f'{parent_dir}/{entry.name.decode("utf-8")}'
