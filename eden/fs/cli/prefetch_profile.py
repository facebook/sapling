# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os

from . import subcmd as subcmd_mod
from .cmd_util import get_eden_instance
from .subcmd import Subcmd


prefetch_profile_cmd = subcmd_mod.Decorator()


@prefetch_profile_cmd("record", "Start recording fetched file paths.")
class RecordProfileCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client() as client:
            client.startRecordingBackingStoreFetch()
        return 0


@prefetch_profile_cmd(
    "finish",
    "Stop recording fetched file paths and save previously"
    " collected fetched file paths in the output prefetch profile",
)
class FinishProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--output-path",
            nargs="?",
            help="The output path to store the prefetch profile",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client() as client:
            files = client.stopRecordingBackingStoreFetch()
            output_path = (
                args.output_path
                if args.output_path
                else os.path.abspath("prefetch_profile.txt")
            )
            with open(output_path, "w") as f:
                f.write("HgQueuedBackingStore:\n")
                for path in files.fetchedFilePaths["HgQueuedBackingStore"]:
                    f.write(os.fsdecode(path))
                    f.write("\n")
                f.write("\n")
        return 0


class PrefetchProfileCmd(Subcmd):
    NAME = "prefetch_profile"
    HELP = "Collect backing store fetched file paths to obtain a prefetch profile"

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        self.add_subcommands(parser, prefetch_profile_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        return 0
