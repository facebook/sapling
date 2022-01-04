# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import sys
from pathlib import Path
from typing import NamedTuple, List

from facebook.eden.ttypes import GlobParams

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
        help=(
            "Specify path to a file that lists patterns/files to match, one per line"
        ),
    )
    parser.add_argument(
        "PATTERN",
        nargs="*",
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


class CheckoutAndPatterns(NamedTuple):
    instance: EdenInstance
    checkout: EdenCheckout
    rel_path: Path
    patterns: List[str]


# \ is a path separator on Windows. On Linux, we allow special characters in the
# paterns, which use \. It's much more common to use \ as a path separator
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
        with open(args.pattern_file) as f:
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
    DESCRIPTION = """Print matching filenames. Glob patterns can be provided
    either via stdin or a pattern file. This command does not do any filtering
    based on source control state or gitignore files."""

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        _add_common_arguments(parser)

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
                    prefetchMetadata=False,
                    searchRoot=os.fsencode(checkout_and_patterns.rel_path),
                    listOnlyFiles=args.list_only_files,
                )
            )
            for name in result.matchingFiles:
                _println(os.fsdecode(name))
        return 0


class PrefetchCmd(Subcmd):
    NAME = "prefetch"
    HELP = "Prefetch content for matching file patterns"
    DESCRIPTION = """Prefetch content for matching file patterns.
    Glob patterns can be provided either via stdin or a pattern file.
    This command does not do any filtering based on source control state or
    gitignore files."""

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        _add_common_arguments(parser)
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
            "--prefetch-metadata",
            help="Prefetch file metadata (sha1 and size) for each file in a "
            + "tree when we fetch trees during this prefetch. This may send a "
            + "large amount of requests to the server and should only be used if "
            + "you understand the risks.",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--background",
            help="Run the prefetch in the background",
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
            telemetry_sample.add_bool("skip_prefetch", args.no_prefetch)
            telemetry_sample.add_bool("background", args.background)
            telemetry_sample.add_bool("prefetch_metadata", args.prefetch_metadata)
            if args.pattern_file:
                telemetry_sample.add_string("pattern_file", args.pattern_file)
            if args.PATTERN:
                telemetry_sample.add_normvector("patterns", args.PATTERN)

            with checkout_and_patterns.instance.get_thrift_client_legacy() as client:
                result = client.globFiles(
                    GlobParams(
                        mountPoint=bytes(checkout_and_patterns.checkout.path),
                        globs=checkout_and_patterns.patterns,
                        includeDotfiles=args.include_dot_files,
                        prefetchFiles=not args.no_prefetch,
                        suppressFileList=args.silent,
                        prefetchMetadata=args.prefetch_metadata,
                        background=args.background,
                        listOnlyFiles=args.list_only_files,
                    )
                )
                if args.background or args.silent:
                    return 0

                telemetry_sample.add_int("files_fetched", len(result.matchingFiles))

                if not args.silent:
                    if checkout_and_patterns.patterns and not result.matchingFiles:
                        _eprintln(
                            f"No files were matched by the pattern{'s' if len(checkout_and_patterns.patterns) else ''} specified.\n"
                            "See `eden prefetch -h` for docs on pattern matching.",
                        )
                    _println(
                        "\n".join(os.fsdecode(name) for name in result.matchingFiles)
                    )

        return 0
