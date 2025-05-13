# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import json
import os
import sys
from pathlib import Path
from typing import List, NamedTuple, Optional

from facebook.eden.ttypes import GlobParams, PrefetchParams
from thrift.Thrift import TApplicationException

from .cmd_util import require_checkout
from .config import EdenCheckout, EdenInstance
from .subcmd import Subcmd


# Avoid CRLF line-endings on Windows.
def _println(val: str) -> None:
    buffer = sys.stdout.buffer
    buffer.write(val.encode("utf-8") + b"\n")
    buffer.flush()


# Avoid CRLF line-endings on Windows.
def _eprintln(val: str) -> None:
    buffer = sys.stderr.buffer
    buffer.write(val.encode("utf-8") + b"\n")
    buffer.flush()


def _add_common_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--repo", help="Specify path to repo root (default: root of cwd)"
    )
    parser.add_argument(
        "--pattern-file",
        metavar="FILE",
        help=(
            "Obtain patterns to match from FILE, one per line. "
            "If FILE is - , read patterns from standard input."
        ),
    )
    parser.add_argument(
        "PATTERN",
        nargs=argparse.ZERO_OR_MORE,
        # Technically, we use fnmatch, but it uses glob for pattern strings.
        # source: https://man7.org/linux/man-pages/man3/fnmatch.3.html
        help="Filename patterns (relative to repo root) to match via glob, see: https://man7.org/linux/man-pages/man7/glob.7.html",
    )
    parser.add_argument(
        "--list-only-files",
        help="When printing the list of matching files, exclude directories.",
        default=False,
        action="store_true",
    )
    parser.add_argument(
        "--include-dot-files",
        help="Include hidden files in the list of returned matching files.",
        default=False,
        action="store_true",
    )


def parseDtype(dtype: Optional[int]) -> str:
    match dtype:
        case None:
            return "Unrequested"
        case 0:
            return "Unknown"
        case 1:
            return "Fifo"
        case 2:
            return "Char"
        case 4:
            return "Dir"
        case 6:
            return "Block"
        case 8:
            return "Regular"
        case 10:
            return "Symlink"
        case 12:
            return "Socket"
        case 14:
            return "Whiteout"
    return "Unknown"


class CheckoutAndPatterns(NamedTuple):
    instance: EdenInstance
    checkout: EdenCheckout
    rel_path: Path
    patterns: List[str]


# \ is a path separator on Windows. On Linux, we allow special characters in the
# patterns, which use \. It's much more common to use \ as a path separator
# instead of a special character on Windows. Many Windows tools will return the
# path with \, so it would be nicer if we could be compatible with this.
# Sould a user need to use special charters on windows we could have them escape
# the backslash. Then we would turn '\\\\' into '\\' here instead of '//'.
# However, changes would need to be made to the daemon as well to teach our
# path abstractions to recognize this as a special character because
# they treat \ as a path separator.
def _clean_pattern(pattern: str) -> str:
    if sys.platform == "win32":
        return pattern.replace("\\", "/")
    else:
        return pattern


def _find_checkout_and_patterns(
    args: argparse.Namespace,
) -> CheckoutAndPatterns:
    instance, checkout, rel_path = require_checkout(args, args.repo)
    if args.repo and rel_path != Path("."):
        _eprintln(f"{args.repo} is not the root of an EdenFS repo")
        raise SystemExit(1)

    raw_patterns = list(args.PATTERN)
    if args.pattern_file is not None:
        handle = sys.stdin if args.pattern_file == "-" else open(args.pattern_file)
        with handle as f:
            raw_patterns.extend(pat.strip() for pat in f.readlines())

    patterns = [_clean_pattern(pattern) for pattern in raw_patterns]

    return CheckoutAndPatterns(
        instance=instance,
        checkout=checkout,
        rel_path=rel_path,
        patterns=patterns,
    )


class GlobCmd(Subcmd):
    NAME = "glob"
    HELP = "Print matching filenames"
    DESCRIPTION = """Print matching filenames.
    Glob patterns can be provided via a pattern file.
    This command does not do any filtering based on source control state or
    gitignore files."""

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        _add_common_arguments(parser)
        parser.add_argument(
            "--json",
            help="Return results as JSON",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--verbose",
            help="Display additional data",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--list-origin-hash",
            help="Display the origin hash of the matching files.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--dtype",
            help="Display the dtype of the matching files.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--revision",
            help="Revisions to search within. Can be used multiple times",
            default=[],
            action="append",
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout_and_patterns = _find_checkout_and_patterns(args)

        with checkout_and_patterns.instance.get_thrift_client_legacy() as client:
            result = client.globFiles(
                GlobParams(
                    mountPoint=bytes(checkout_and_patterns.checkout.path),
                    globs=checkout_and_patterns.patterns,
                    includeDotfiles=args.include_dot_files,
                    prefetchFiles=False,
                    suppressFileList=False,
                    wantDtype=args.dtype,
                    searchRoot=os.fsencode(checkout_and_patterns.rel_path),
                    listOnlyFiles=args.list_only_files,
                    revisions=args.revision,
                )
            )
            if args.json:
                _println(
                    json.dumps(
                        {
                            "matching_files": [
                                os.fsdecode(name) for name in result.matchingFiles
                            ],
                            "dtype": [parseDtype(dtype) for dtype in result.dtypes],
                            "origin_hashes": [
                                ohash.hex() for ohash in result.originHashes
                            ],
                        }
                    )
                )
            else:
                # Name and origin hashes should be the same size.
                # If dtype is set then it should also be the same size, otherwise 0
                if len(result.matchingFiles) != len(result.originHashes):
                    _println("Error globbing files: mismatched results")
                    return 1
                if args.dtype:
                    if len(result.dtypes) != len(result.matchingFiles):
                        _println("Error globbing files: mismatched results")
                        return 1
                    entries = zip(
                        result.matchingFiles, result.dtypes, result.originHashes
                    )
                else:
                    entries = zip(
                        result.matchingFiles,
                        [None] * len(result.matchingFiles),
                        result.originHashes,
                    )
                for name, dtype, ohash in entries:
                    baseString = os.fsdecode(name)
                    if args.list_origin_hash:
                        baseString += f"@{ohash.hex()}"
                    if args.dtype:
                        baseString += f" {parseDtype(dtype)}"
                    _println(os.fsdecode(baseString))
                if args.verbose:
                    _println(
                        f"Num matching files: {len(result.matchingFiles)}\n"
                        f"Num dtypes: {len(result.dtypes)}\n"
                        f"Num origin hashes: {len(result.originHashes)}"
                    )
        return 0


class PrefetchCmd(Subcmd):
    NAME = "prefetch"
    HELP = "Prefetch content for matching file patterns"
    DESCRIPTION = """Prefetch content for matching file patterns.
    Glob patterns can be provided via a pattern file.
    This command does not do any filtering based on source control state or
    gitignore files."""

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        _add_common_arguments(parser)
        # TODO: replace --silent with --debug-print, only to be used for console info logging
        parser.add_argument(
            "--silent",
            help="DEPRECATED: Do not print the names of the matching files",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--directories-only",
            help="Do not prefetch files; only prefetch directories",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--background",
            help="Run the prefetch in the background",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--debug-print",
            help="Print the paths being prefetched. Does not work if using --background",
            default=False,
            action="store_true",
        )

    def run(self, args: argparse.Namespace) -> int:
        checkout_and_patterns = _find_checkout_and_patterns(args)

        with checkout_and_patterns.instance.get_telemetry_logger().new_sample(
            "prefetch"
        ) as telemetry_sample:
            telemetry_sample.add_string(
                "checkout", checkout_and_patterns.checkout.path.name
            )
            telemetry_sample.add_bool("directories_only", args.directories_only)
            telemetry_sample.add_bool("background", args.background)
            if args.pattern_file:
                telemetry_sample.add_string("pattern_file", args.pattern_file)
            if args.PATTERN:
                telemetry_sample.add_normvector("patterns", args.PATTERN)

            silent = args.silent or not args.debug_print

            # TODO(T183395303) Remove fallback when prefetch_fallback ODS counter hits 0.
            with checkout_and_patterns.instance.get_thrift_client_legacy() as client:
                if args.background or silent:
                    try:
                        client.prefetchFilesV2(
                            PrefetchParams(
                                mountPoint=bytes(checkout_and_patterns.checkout.path),
                                globs=checkout_and_patterns.patterns,
                                directoriesOnly=args.directories_only,
                                background=args.background,
                                returnPrefetchedFiles=False,
                            )
                        )
                        telemetry_sample.add_bool("prefetchV2_fallback", False)
                    except TApplicationException:
                        client.prefetchFiles(
                            PrefetchParams(
                                mountPoint=bytes(checkout_and_patterns.checkout.path),
                                globs=checkout_and_patterns.patterns,
                                directoriesOnly=args.directories_only,
                                background=args.background,
                            )
                        )
                        telemetry_sample.add_bool("prefetchV2_fallback", True)
                else:
                    try:
                        # Not handling the following arguments used in globFiles
                        # includeDotfiles is true by default in prefetching
                        # prefetchFiles is happening in this command
                        # suppressFileList is replaced by returnPrefetchedFiles
                        # listOnlyFiles is false by default in prefetching
                        prefetchResult = client.prefetchFilesV2(
                            PrefetchParams(
                                mountPoint=bytes(checkout_and_patterns.checkout.path),
                                globs=checkout_and_patterns.patterns,
                                directoriesOnly=args.directories_only,
                                background=False,
                                returnPrefetchedFiles=True,
                            )
                        )
                        telemetry_sample.add_bool("prefetchV2_fallback", False)
                        if prefetchResult.prefetchedFiles:
                            result = prefetchResult.prefetchedFiles
                        else:
                            result = None
                    except TApplicationException:
                        # Falling back to globFiles if V2 doesn't exist
                        result = client.globFiles(
                            GlobParams(
                                mountPoint=bytes(checkout_and_patterns.checkout.path),
                                globs=checkout_and_patterns.patterns,
                                includeDotfiles=args.include_dot_files,
                                prefetchFiles=not args.directories_only,
                                suppressFileList=False,
                                background=False,
                                listOnlyFiles=args.list_only_files,
                            )
                        )
                        telemetry_sample.add_bool("prefetchV2_fallback", True)
                    # result should always be set unless there was an error with prefetchV2
                    if result:
                        telemetry_sample.add_int(
                            "files_fetched", len(result.matchingFiles)
                        )

                        if checkout_and_patterns.patterns and not result.matchingFiles:
                            _eprintln(
                                f"No files were matched by the pattern{'s' if len(checkout_and_patterns.patterns) else ''} specified.\n"
                                "See `eden prefetch -h` for docs on pattern matching.",
                            )
                        _println(
                            "\n".join(
                                os.fsdecode(name) for name in result.matchingFiles
                            )
                        )
                    else:
                        _eprintln("Error prefetching files")

        return 0
