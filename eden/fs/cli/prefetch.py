# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
from pathlib import Path

from facebook.eden.ttypes import GlobParams

from .cmd_util import require_checkout
from .subcmd import Subcmd


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
