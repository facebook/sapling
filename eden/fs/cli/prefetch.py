# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
from pathlib import Path
from typing import Optional

from facebook.eden.ttypes import GlobParams

from . import subcmd as subcmd_mod, util
from .cmd_util import get_eden_instance, require_checkout
from .subcmd import Subcmd


prefetch_cmd = subcmd_mod.Decorator()


@prefetch_cmd(
    "record-profile",
    "Start recording fetched file paths. When finish-profile is"
    " called file paths will be saved to a prefetch profile",
)
class RecordProfileCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        with instance.get_thrift_client() as client:
            client.startRecordingBackingStoreFetch()
        return 0


@prefetch_cmd(
    "finish-profile",
    "Stop recording fetched file paths and save previously"
    " collected fetched file paths in the output prefetch profile",
)
class FinishProfileCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "output_path",
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


class PrefetchCmd(Subcmd):
    NAME = "prefetch"
    HELP = "Prefetch content for matching file patterns"

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--repo", help="Specify path to repo root (default: root of cwd)"
        )
        parser.add_argument(
            "--pattern-file",
            help=(
                "Specify path to a file that lists patterns/files "
                "to match, one per line"
            ),
        )
        parser.add_argument(
            "--silent",
            help="Do not print the names of the matching files",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--no-prefetch",
            help="Do not prefetch; only match names",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "PATTERN", nargs="*", help="Filename patterns to match via fnmatch"
        )
        self.add_subcommands(parser, prefetch_cmd.commands)

    def _repo_root(self, path: str) -> Optional[str]:
        try:
            return util.get_eden_mount_name(path)
        except Exception:
            # Likely no .eden dir there, so probably not an eden repo
            return None

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, rel_path = require_checkout(args, args.repo)
        if args.repo and rel_path != Path("."):
            print(f"{args.repo} is not the root of an eden repo")
            return 1

        if args.pattern_file is not None:
            with open(args.pattern_file) as f:
                args.PATTERN += [pat.strip() for pat in f.readlines()]

        with instance.get_thrift_client() as client:
            result = client.globFiles(
                GlobParams(
                    mountPoint=bytes(checkout.path),
                    globs=args.PATTERN,
                    includeDotfiles=False,
                    prefetchFiles=not args.no_prefetch,
                    suppressFileList=args.silent,
                )
            )
            if not args.silent:
                for name in result.matchingFiles:
                    print(os.fsdecode(name))

        return 0
