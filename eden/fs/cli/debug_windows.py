#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
from pathlib import Path

from .debug import debug_cmd as cmd
from .subcmd import Subcmd


@cmd("prjfs-state", "Show ProjectedFS file state")
class PrjfsStateCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", help="The path to the file or directory")

    def run(self, args: argparse.Namespace) -> int:
        path = Path(args.path)

        if path.is_dir():
            self._print_state(path)
            for child in path.iterdir():
                self._print_state(child)
        else:
            self._print_state(path)

        return 0

    def _print_state(self, p: Path) -> None:
        from . import prjfs

        # pyre-fixme[16]: TODO(T65013742): Can't type check Windows on Linux
        state = prjfs.PrjGetOnDiskFileState(p)
        state_formatted = ", ".join(
            [str(flag.name) for flag in prjfs.PRJ_FILE_STATE if flag in state]
        )

        print("{0!s:50} {1!s}".format(p, state_formatted))
