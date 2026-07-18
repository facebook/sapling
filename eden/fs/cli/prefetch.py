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

from eden.fs.service.eden.thrift_types import GlobParams, PrefetchParams, PrefetchStats

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


def _format_bytes(num_bytes: int) -> str:
    """Format bytes in a human-readable way."""
    if num_bytes < 1024:
        return f"{num_bytes} B"
    elif num_bytes < 1024 * 1024:
        return f"{num_bytes / 1024:.1f} KiB"
    elif num_bytes < 1024 * 1024 * 1024:
        return f"{num_bytes / (1024 * 1024):.1f} MiB"
    else:
        return f"{num_bytes / (1024 * 1024 * 1024):.2f} GiB"


def _format_count(count: int) -> str:
    """Format large numbers with commas for readability."""
    return f"{count:,}"


def _print_prefetch_stats(
    stats: PrefetchStats,
    globs: Optional[List[str]] = None,
    preload_duration_ms: Optional[int] = None,
    actual_total_ms: Optional[int] = None,
) -> None:
    """Print prefetch statistics in a human-readable format.

    Args:
        stats: Prefetch statistics from the server.
        globs: The glob patterns that were prefetched.
        preload_duration_ms: Duration of just the preload operation (if used).
        actual_total_ms: Actual wall clock time from start to finish (reflects
            interleaved execution). If not provided, falls back to prefetch + preload.
    """
    _println("")
    _println("=== Prefetch Statistics ===")
    _println("")

    # Summary section
    _println("Summary:")
    if globs:
        globs_str = " ".join(globs)
        _println(f"  Globs:            {globs_str}")
    _println(f"  Files prefetched: {_format_count(stats.filesPrefetched)}")
    if stats.filesFailed > 0:
        _println(f"  Files failed:     {_format_count(stats.filesFailed)}")

    # Timing section
    prefetch_secs = stats.totalDurationMs / 1000.0
    _println(f"  Prefetch time:    {prefetch_secs:.2f} s")

    # total_secs is the user-observed wall-clock time for the whole
    # operation; when preload runs it dominates by 5x or more, so we
    # use it (not prefetch_secs alone) as the denominator for file
    # throughput below.
    if preload_duration_ms is not None:
        preload_secs = preload_duration_ms / 1000.0
        _println(f"  Preload time:     {preload_secs:.2f} s")
        # Use actual wall clock time if available (reflects interleaved execution)
        if actual_total_ms is not None:
            total_secs = actual_total_ms / 1000.0
        else:
            total_secs = prefetch_secs + preload_secs
        _println(f"  Total time:       {total_secs:.2f} s")
    else:
        total_secs = prefetch_secs

    if total_secs > 0:
        # Files-per-second over the FULL wall-clock duration — what the
        # user actually observes. Anchoring this on prefetch_secs alone
        # makes preload-heavy runs look 5-9x faster than reality.
        throughput = stats.filesPrefetched / total_secs
        _println(f"  Throughput:       {throughput:,.1f} files/s")
        # Network throughput: decompressed bytes pulled over the wire,
        # divided by the prefetch phase (which is when network fetches
        # happen). Distinct from Throughput above — this measures the
        # daemon's effective network bandwidth, not end-user rate.
        network_bytes = stats.blobBytesFromNetwork + stats.treeBytesFromNetwork
        if network_bytes > 0 and prefetch_secs > 0:
            net_mib_s = (network_bytes / (1024 * 1024)) / prefetch_secs
            _println(f"  Network:          {net_mib_s:.1f} MiB/s (during prefetch)")
    # Calculate and display tree cache hit rate with counts and bytes
    total_trees = (
        stats.treesFromMemoryCache + stats.treesFromDiskCache + stats.treesFromNetwork
    )
    total_tree_bytes = (
        stats.treeBytesFromMemoryCache
        + stats.treeBytesFromDiskCache
        + stats.treeBytesFromNetwork
    )
    if total_trees > 0:
        tree_cached = stats.treesFromMemoryCache + stats.treesFromDiskCache
        tree_bytes_cached = (
            stats.treeBytesFromMemoryCache + stats.treeBytesFromDiskCache
        )
        tree_hit_rate = 100.0 * tree_cached / total_trees
        _println(
            f"  Tree cache:       {tree_hit_rate:.2f}% ({_format_count(tree_cached)} of {_format_count(total_trees)}, {_format_bytes(tree_bytes_cached)} of {_format_bytes(total_tree_bytes)})"
        )

    # Calculate and display blob cache hit rate with counts and bytes
    total_blobs = (
        stats.blobsFromMemoryCache + stats.blobsFromDiskCache + stats.blobsFromNetwork
    )
    total_blob_bytes = (
        stats.blobBytesFromMemoryCache
        + stats.blobBytesFromDiskCache
        + stats.blobBytesFromNetwork
    )
    if total_blobs > 0:
        blob_cached = stats.blobsFromMemoryCache + stats.blobsFromDiskCache
        blob_bytes_cached = (
            stats.blobBytesFromMemoryCache + stats.blobBytesFromDiskCache
        )
        blob_hit_rate = 100.0 * blob_cached / total_blobs
        _println(
            f"  Blob cache:       {blob_hit_rate:.2f}% ({_format_count(blob_cached)} of {_format_count(total_blobs)}, {_format_bytes(blob_bytes_cached)} of {_format_bytes(total_blob_bytes)})"
        )

    _println(f"  Overall cache:    {stats.cacheHitRate:.1f}%")
    _println("")

    # Trees detail section (reuse total_trees and total_tree_bytes from above)
    _println(
        f"Trees: {_format_count(total_trees)} (data volume: {_format_bytes(total_tree_bytes)})"
    )
    _println(f"  Memory cache: {_format_count(stats.treesFromMemoryCache)}")
    _println(f"  Disk cache:   {_format_count(stats.treesFromDiskCache)}")
    _println(f"  Network:      {_format_count(stats.treesFromNetwork)}")
    _println("")

    # Blobs detail section (reuse total_blobs and total_blob_bytes from above)
    _println(
        f"Blobs: {_format_count(total_blobs)} (data volume: {_format_bytes(total_blob_bytes)})"
    )
    _println(f"  Memory cache: {_format_count(stats.blobsFromMemoryCache)}")
    _println(f"  Disk cache:   {_format_count(stats.blobsFromDiskCache)}")
    _println(f"  Network:      {_format_count(stats.blobsFromNetwork)}")


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
# Should a user need to use special charters on windows we could have them escape
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
            help="Display the origin hash of the matching files. Only populated when multiple --revision flags are specified.",
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

        with checkout_and_patterns.instance.get_thrift_client() as client:
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
                    revisions=[rev.encode() for rev in args.revision],
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
                # originHashes may be empty when there are 0 or 1 revisions.
                # When populated, it should match matchingFiles in length.
                has_origin_hashes = len(result.originHashes) > 0
                if has_origin_hashes and len(result.matchingFiles) != len(
                    result.originHashes
                ):
                    _println("Error globbing files: mismatched results")
                    return 1
                if args.dtype:
                    if len(result.dtypes) != len(result.matchingFiles):
                        _println("Error globbing files: mismatched results")
                        return 1
                for i, name in enumerate(result.matchingFiles):
                    baseString = os.fsdecode(name)
                    if args.list_origin_hash and has_origin_hashes:
                        baseString += f"@{result.originHashes[i].hex()}"
                    if args.dtype:
                        baseString += f" {parseDtype(result.dtypes[i])}"
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
        parser.add_argument(
            "-r",
            "--relative",
            help="Resolve patterns relative to the current working directory instead of the repo root",
            default=False,
            action="store_true",
        )
        parser.add_argument(
            "--stats",
            help="Print statistics about the prefetch operation (cache hits, timing, etc.)",
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

            search_root = None
            if args.relative:
                search_root = os.fsencode(checkout_and_patterns.rel_path)

            with checkout_and_patterns.instance.get_thrift_client() as client:
                prefetchResult = client.prefetchFilesV2(
                    PrefetchParams(
                        mountPoint=bytes(checkout_and_patterns.checkout.path),
                        globs=checkout_and_patterns.patterns,
                        directoriesOnly=args.directories_only,
                        searchRoot=search_root,
                        background=args.background,
                        returnPrefetchedFiles=not args.background
                        and not args.silent
                        and args.debug_print,
                        returnStats=args.stats,
                    )
                )

                result = prefetchResult.prefetchedFiles
                if args.stats and prefetchResult.stats is not None:
                    _print_prefetch_stats(
                        prefetchResult.stats,
                        globs=checkout_and_patterns.patterns,
                    )
                elif args.stats:
                    _eprintln("Prefetch stats unavailable")

                if result:
                    telemetry_sample.add_int("files_fetched", len(result.matchingFiles))

                    if checkout_and_patterns.patterns and not result.matchingFiles:
                        _eprintln(
                            f"No files were matched by the pattern{'s' if len(checkout_and_patterns.patterns) else ''} specified.\n"
                            "See `eden prefetch -h` for docs on pattern matching.",
                        )
                    _println(
                        "\n".join(os.fsdecode(name) for name in result.matchingFiles)
                    )

        return 0
