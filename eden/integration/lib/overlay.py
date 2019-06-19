#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import pathlib
import stat
import tempfile

import eden.integration.lib.edenclient as edenclient


class OverlayStore:
    def __init__(self, eden: edenclient.EdenFS, mount: pathlib.Path) -> None:
        self.eden = eden
        self.mount = mount
        self.overlay_dir = eden.overlay_dir_for_mount(mount)

    def materialize_file(self, path: pathlib.Path) -> pathlib.Path:
        """Force the file inode at the specified path to be materialized and recorded in
        the overlay.  Returns the path to the overlay file that stores the data for this
        inode in the overlay.
        """
        path_in_mount = self.mount / path
        # Opening the file in write mode will materialize it
        with path_in_mount.open("w+b") as f:
            s = os.fstat(f.fileno())

        return self._get_overlay_path(s.st_ino)

    def materialize_dir(self, path: pathlib.Path) -> pathlib.Path:
        """Force the directory inode at the specified path to be materialized and
        recorded in the overlay.  Returns the path to the overlay file that stores the
        data for this inode in the overlay.
        """
        path_in_mount = self.mount / path
        s = os.lstat(path_in_mount)
        assert stat.S_ISDIR(s.st_mode)
        # Creating and then removing a file inside the directory will materialize it
        with tempfile.NamedTemporaryFile(dir=str(path_in_mount)):
            pass

        return self._get_overlay_path(s.st_ino)

    def _get_overlay_path(self, inode_number: int) -> pathlib.Path:
        subdir = "{:02x}".format(inode_number % 256)
        return self.overlay_dir / subdir / str(inode_number)

    def delete_cached_next_inode_number(self) -> None:
        (self.overlay_dir / "next-inode-number").unlink()
