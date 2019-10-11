#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import binascii
import os
import shutil
import stat
import sys
from pathlib import Path
from typing import BinaryIO, Optional

from facebook.eden.overlay.ttypes import OverlayDir

from . import cmd_util, debug as debug_mod, overlay as overlay_mod, subcmd as subcmd_mod


cmd = debug_mod.debug_cmd


@cmd("overlay", "Show data about the overlay")
class OverlayCmd(subcmd_mod.Subcmd):
    # pyre-fixme[13]: Attribute `args` is never initialized.
    args: argparse.Namespace
    # pyre-fixme[13]: Attribute `overlay` is never initialized.
    overlay: overlay_mod.Overlay

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-n",
            "--number",
            type=int,
            help="Display information for the specified inode number.",
        )
        parser.add_argument(
            "-d", "--depth", type=int, default=0, help="Recurse to the specified depth."
        )
        parser.add_argument(
            "-r",
            "--recurse",
            action="store_const",
            const=-1,
            dest="depth",
            default=0,
            help="Recursively print child entries.",
        )
        parser.add_argument(
            "-O",
            "--overlay",
            help="Explicitly specify the path to the overlay directory.",
        )
        parser.add_argument(
            "-x",
            "--extract-to",
            dest="output_path",
            help="Copy the specified inode data to the destination path.",
        )
        parser.add_argument("path", nargs="?", help="The path to the eden mount point.")

    def run(self, args: argparse.Namespace) -> int:
        self.args = args
        if args.overlay is not None:
            if args.path:
                rel_path = Path(args.path)
            else:
                rel_path = Path()
            overlay_dir = Path(args.overlay)
        else:
            path = args.path or os.getcwd()
            _instance, checkout, rel_path = cmd_util.require_checkout(args, path)
            overlay_dir = checkout.state_dir.joinpath("local")

        self.overlay = overlay_mod.Overlay(str(overlay_dir))

        if args.number is not None:
            self._process_root(args.number, Path())
        elif rel_path != Path():
            inode_number = self.overlay.lookup_path(rel_path)
            if inode_number is None:
                print(f"{rel_path} is not materialized", file=sys.stderr)
                return 1
            self._process_root(inode_number, rel_path)
        else:
            self._process_root(1, Path())

        return 0

    def _process_root(self, inode_number: int, initial_path: Path):
        output_path: Optional[Path] = None
        if self.args.output_path is not None:
            output_path = Path(self.args.output_path)

        with self.overlay.open_overlay_file(inode_number) as f:
            header = self.overlay.read_header(f)
            if header.type == overlay_mod.OverlayHeader.TYPE_DIR:
                if output_path:
                    self.overlay.extract_dir(inode_number, output_path)
                    print(f"Extracted materialized directory contents to {output_path}")
                else:
                    self._process_overlay(inode_number, initial_path)
            elif header.type == overlay_mod.OverlayHeader.TYPE_FILE:
                if output_path:
                    self.overlay.extract_file(
                        inode_number, output_path, stat.S_IFREG | 0o644
                    )
                    print(f"Extracted inode {inode_number} to {output_path}")
                else:
                    self._print_file(f)
            else:
                raise Exception(
                    f"found invalid file type information in overlay "
                    f"file header: {type!r}"
                )

    def _print_file(self, f: BinaryIO) -> None:
        shutil.copyfileobj(f, sys.stdout.buffer)  # type: ignore

    def _process_overlay(self, inode_number: int, path: Path, level: int = 0) -> None:
        data = self.overlay.read_dir_inode(inode_number)
        self._print_overlay_tree(inode_number, path, data)

        # If self.args.depth is negative, recurse forever.
        # Stop if self.args.depth is non-negative, and level reaches the maximum
        # requested recursion depth.
        if self.args.depth >= 0 and level >= self.args.depth:
            return

        entries = {} if data.entries is None else data.entries
        for name, entry in entries.items():
            if entry.hash or entry.inodeNumber is None or entry.inodeNumber == 0:
                # This entry is not materialized
                continue
            if entry.mode is None or stat.S_IFMT(entry.mode) != stat.S_IFDIR:
                # Only display data for directories
                continue
            print()
            entry_path = path.joinpath(name)
            self._process_overlay(entry.inodeNumber, entry_path, level + 1)

    def _print_overlay_tree(
        self, inode_number: int, path: Path, tree_data: OverlayDir
    ) -> None:
        def hex(binhash: Optional[bytes]) -> str:
            if binhash is None:
                return "None"
            else:
                return binascii.hexlify(binhash).decode("utf-8")

        print("Inode {}: {}".format(inode_number, path))
        if not tree_data.entries:
            return
        name_width = max(len(name) for name in tree_data.entries)
        for name, entry in tree_data.entries.items():
            assert entry.mode is not None
            perms = entry.mode & 0o7777
            file_type = stat.S_IFMT(entry.mode)
            if file_type == stat.S_IFREG:
                file_type_flag = "f"
            elif file_type == stat.S_IFDIR:
                file_type_flag = "d"
            elif file_type == stat.S_IFLNK:
                file_type_flag = "l"
            else:
                file_type_flag = "?"

            print(
                "    {:{name_width}s} : {:12d} {} {:04o} {}".format(
                    name,
                    entry.inodeNumber,
                    file_type_flag,
                    perms,
                    hex(entry.hash),
                    name_width=name_width,
                )
            )
