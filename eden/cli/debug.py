#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import binascii
import collections
import json
import os
import re
import shlex
import stat
import sys
import time
from pathlib import Path
from typing import (
    IO,
    Any,
    Callable,
    DefaultDict,
    Dict,
    Iterator,
    List,
    Optional,
    Pattern,
    Tuple,
    Union,
    cast,
)

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
import thrift.util.inspect
from facebook.eden import EdenService
from facebook.eden.ttypes import (
    DebugGetRawJournalParams,
    DebugJournalDelta,
    EdenError,
    NoValueForKeyError,
    TimeSpec,
    TreeInodeDebugInfo,
)
from fb303_core import BaseService
from thrift.protocol.TSimpleJSONProtocol import TSimpleJSONProtocolFactory
from thrift.util import Serializer

from . import cmd_util, stats_print, subcmd as subcmd_mod, ui as ui_mod
from .config import EdenCheckout, EdenInstance
from .subcmd import Subcmd
from .util import split_inodes_by_operation_type


MB = 1024 ** 2
debug_cmd = subcmd_mod.Decorator()


def escape_path(value: bytes) -> str:
    """
    Take a binary path value, and return a printable string, with special
    characters escaped.
    """

    def human_readable_byte(b: int) -> str:
        if b < 0x20 or b >= 0x7F:
            return "\\x{:02x}".format(b)
        elif b == ord(b"\\"):
            return "\\\\"
        return chr(b)

    return "".join(human_readable_byte(b) for b in value)


def hash_str(value: bytes) -> str:
    """
    Take a hash as a binary value, and return it represented as a hexadecimal
    string.
    """
    return binascii.hexlify(value).decode("utf-8")


def parse_object_id(value: str) -> bytes:
    """
    Parse an object ID as a 40-byte hexadecimal string, and return a 20-byte
    binary value.
    """
    try:
        binary = binascii.unhexlify(value)
        if len(binary) != 20:
            raise ValueError()
    except ValueError:
        raise ValueError("blob ID must be a 40-byte hexadecimal value")
    return binary


@debug_cmd("parents", "Show Eden's current working copy parent")
class ParentsCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

        parser.add_argument(
            "--hg", action="store_true", help="Include Mercurial's parents in output"
        )

    def _commit_hex(self, commit: bytes) -> str:
        return binascii.hexlify(commit).decode("utf-8")

    def run(self, args: argparse.Namespace) -> int:
        null_commit_id = 20 * b"\x00"

        path = args.path or os.getcwd()
        _, checkout, _ = cmd_util.require_checkout(args, path)
        try:
            snapshot_hex = checkout.get_snapshot()
        except Exception as ex:
            print(f"error parsing Eden snapshot : {ex}")
            return 1

        if args.hg:
            hg_parents, _, _ = _get_dirstate_data(checkout)

            print("Mercurial p0: {}".format(self._commit_hex(hg_parents[0])))
            if hg_parents[1] != null_commit_id:
                print("Mercurial p1: {}".format(self._commit_hex(hg_parents[1])))
            print("Eden snapshot: {}".format(snapshot_hex))
        else:
            print(snapshot_hex)

        return 0


@debug_cmd("tree", "Show eden's data for a source control tree")
class TreeCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-L",
            "--load",
            action="store_true",
            default=False,
            help="Load data from the backing store if necessary",
        )
        parser.add_argument("mount", help="The eden mount point path.")
        parser.add_argument("id", help="The tree ID")

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        tree_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client() as client:
            entries = client.debugGetScmTree(
                bytes(checkout.path), tree_id, localStoreOnly=local_only
            )

        for entry in entries:
            file_type_flags, perms = _parse_mode(entry.mode)
            print(
                "{} {:4o} {:40} {}".format(
                    file_type_flags, perms, hash_str(entry.id), escape_path(entry.name)
                )
            )

        return 0


@debug_cmd("blob", "Show eden's data for a source control blob")
class BlobCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-L",
            "--load",
            action="store_true",
            default=False,
            help="Load data from the backing store if necessary",
        )
        parser.add_argument("mount", help="The eden mount point path.")
        parser.add_argument("id", help="The blob ID")

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        blob_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client() as client:
            data = client.debugGetScmBlob(
                bytes(checkout.path), blob_id, localStoreOnly=local_only
            )

        sys.stdout.buffer.write(data)
        return 0


@debug_cmd("blobmeta", "Show eden's metadata about a source control blob")
class BlobMetaCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-L",
            "--load",
            action="store_true",
            default=False,
            help="Load data from the backing store if necessary",
        )
        parser.add_argument("mount", help="The eden mount point path.")
        parser.add_argument("id", help="The blob ID")

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        blob_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client() as client:
            info = client.debugGetScmBlobMetadata(
                bytes(checkout.path), blob_id, localStoreOnly=local_only
            )

        print("Blob ID: {}".format(args.id))
        print("Size:    {}".format(info.size))
        print("SHA1:    {}".format(hash_str(info.contentsSha1)))
        return 0


_FILE_TYPE_FLAGS = {stat.S_IFREG: "f", stat.S_IFDIR: "d", stat.S_IFLNK: "l"}


def _parse_mode(mode: int) -> Tuple[str, int]:
    """
    Take a mode value, and return a tuple of (file_type, permissions)
    where file type is a one-character flag indicating if this is a file,
    directory, or symbolic link.
    """
    file_type_str = _FILE_TYPE_FLAGS.get(stat.S_IFMT(mode), "?")
    perms = mode & 0o7777
    return file_type_str, perms


@debug_cmd("buildinfo", "Show the build info for the Eden server")
class BuildInfoCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        do_buildinfo(instance)
        return 0


def do_buildinfo(instance: EdenInstance, out: Optional[IO[bytes]] = None) -> None:
    if out is None:
        out = sys.stdout.buffer
    build_info = instance.get_server_build_info()
    sorted_build_info = collections.OrderedDict(sorted(build_info.items()))
    for key, value in sorted_build_info.items():
        out.write(b"%s: %s\n" % (key.encode(), value.encode()))


@debug_cmd("clear_local_caches", "Clears local caches of objects stored in RocksDB")
class ClearLocalCachesCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            client.debugClearLocalStoreCaches()
        return 0


@debug_cmd("compact_local_storage", "Asks RocksDB to compact its storage")
class CompactLocalStorageCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            client.debugCompactLocalStorage()
        return 0


@debug_cmd("uptime", "Check how long edenfs has been running")
class UptimeCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        do_uptime(instance)
        return 0


def do_uptime(instance: EdenInstance, out: Optional[IO[bytes]] = None) -> None:
    if out is None:
        out = sys.stdout.buffer
    uptime = instance.get_uptime()  # Check if uptime is negative?
    days = uptime.days
    hours, remainder = divmod(uptime.seconds, 3600)
    minutes, seconds = divmod(remainder, 60)
    out.write(b"%dd:%02dh:%02dm:%02ds\n" % (days, hours, minutes, seconds))


@debug_cmd("hg_copy_map_get_all", "Copymap for dirstate")
class HgCopyMapGetAllCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        path = args.path or os.getcwd()
        _instance, checkout, _rel_path = cmd_util.require_checkout(args, path)
        _parents, _dirstate_tuples, copymap = _get_dirstate_data(checkout)
        _print_copymap(copymap)
        return 0


def _print_copymap(copy_map: Dict[str, str]) -> None:
    copies = [f"{item[1]} -> {item[0]}" for item in copy_map.items()]
    copies.sort()
    for copy in copies:
        print(copy)


@debug_cmd("hg_dirstate", "Print full dirstate")
class HgDirstateCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        path = args.path or os.getcwd()
        _instance, checkout, _rel_path = cmd_util.require_checkout(args, path)
        _parents, dirstate_tuples, copymap = _get_dirstate_data(checkout)
        out = ui_mod.get_output()
        entries = list(dirstate_tuples.items())
        out.writeln(f"Non-normal Files ({len(entries)}):", attr=out.BOLD)
        entries.sort(key=lambda entry: entry[0])  # Sort by key.
        for path, dirstate_tuple in entries:
            _print_hg_nonnormal_file(Path(os.fsdecode(path)), dirstate_tuple, out)

        out.writeln(f"Copymap ({len(copymap)}):", attr=out.BOLD)
        _print_copymap(copymap)
        return 0


@debug_cmd("hg_get_dirstate_tuple", "Dirstate status for file")
class HgGetDirstateTupleCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path", help="The path to the file whose status should be queried."
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)
        _parents, dirstate_tuples, _copymap = _get_dirstate_data(checkout)
        dirstate_tuple = dirstate_tuples.get(str(rel_path))
        out = ui_mod.get_output()
        if dirstate_tuple:
            _print_hg_nonnormal_file(rel_path, dirstate_tuple, out)
        else:
            instance = cmd_util.get_eden_instance(args)
            with instance.get_thrift_client() as client:
                try:
                    entry = client.getManifestEntry(
                        bytes(checkout.path), bytes(rel_path)
                    )
                    dirstate_tuple = ("n", entry.mode, 0)
                    _print_hg_nonnormal_file(rel_path, dirstate_tuple, out)
                except NoValueForKeyError:
                    print(f"No tuple for {rel_path}", file=sys.stderr)
                    return 1

        return 0


def _print_hg_nonnormal_file(
    rel_path: Path, dirstate_tuple: Tuple[str, Any, int], out: ui_mod.Output
) -> None:
    status = _dirstate_char_to_name(dirstate_tuple[0])
    merge_state = _dirstate_merge_state_to_name(dirstate_tuple[2])

    out.writeln(f"{rel_path}", fg=out.GREEN)
    out.writeln(f"    status = {status}")
    out.writeln(f"    mode = {oct(dirstate_tuple[1])}")
    out.writeln(f"    mergeState = {merge_state}")


def _dirstate_char_to_name(state: str) -> str:
    if state == "n":
        return "Normal"
    elif state == "m":
        return "NeedsMerging"
    elif state == "r":
        return "MarkedForRemoval"
    elif state == "a":
        return "MarkedForAddition"
    elif state == "?":
        return "NotTracked"
    else:
        raise Exception(f"Unrecognized dirstate char: {state}")


def _dirstate_merge_state_to_name(merge_state: int) -> str:
    if merge_state == 0:
        return "NotApplicable"
    elif merge_state == -1:
        return "BothParents"
    elif merge_state == -2:
        return "OtherParent"
    else:
        raise Exception(f"Unrecognized merge_state value: {merge_state}")


def _get_dirstate_data(
    checkout: EdenCheckout,
) -> Tuple[Tuple[bytes, bytes], Dict[str, Tuple[str, Any, int]], Dict[str, str]]:
    """Returns a tuple of (parents, dirstate_tuples, copymap).
    On error, returns None.
    """
    filename = checkout.path.joinpath(".hg", "dirstate")
    with filename.open("rb") as f:
        return eden.dirstate.read(f, str(filename))


@debug_cmd("inode", "Show data about loaded inodes")
class InodeCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            help="The path to the eden mount point.  If a subdirectory inside "
            "a mount point is specified, only data about inodes under the "
            "specified subdirectory will be reported.",
        )

    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout.buffer
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)
        with instance.get_thrift_client() as client:
            results = client.debugInodeStatus(bytes(checkout.path), bytes(rel_path))

        out.write(b"%d loaded TreeInodes\n" % len(results))
        for inode_info in results:
            _print_inode_info(inode_info, out)
        return 0


@debug_cmd("file_stats", "Show data about loaded and written files")
class FileStatsCMD(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", help="The path to the eden mount point")

    def make_file_entries(
        self, paths_and_sizes: List[Tuple[str, int]]
    ) -> List[Dict[str, Union[str, int]]]:
        return [
            {"path": path, "size": file_size} for (path, file_size) in paths_and_sizes
        ]

    def make_summary(self, paths_and_sizes: List[Tuple[str, int]]) -> Dict[str, Any]:
        # large files larger than 10mb are processed differently by mercurial
        large_paths_and_sizes = [
            (path, size) for path, size in paths_and_sizes if size > 10 * MB
        ]
        summary = {
            "file_count": len(paths_and_sizes),
            "total_bytes": sum(size for _, size in paths_and_sizes),
            "large_file_count": len(large_paths_and_sizes),
            "large_files": self.make_file_entries(large_paths_and_sizes),
            "largest_directories_by_file_count": self.get_largest_directories_by_count(
                paths_and_sizes
            ),
        }

        return summary

    @staticmethod
    def get_largest_directories_by_count(
        paths_and_sizes: List[Tuple[str, int]], min_file_count: int = 1000
    ) -> List[Dict[str, Union[int, str]]]:
        """
        Returns a list of directories that contain more than min_file_count
        files.
        """
        directories: DefaultDict[str, int] = collections.defaultdict(int)
        directories["."] = 0
        for filepath, _ in paths_and_sizes:
            for parent in Path(filepath).parents:
                directories[str(parent)] += 1

        directory_list: List[Dict[str, Union[int, str]]] = sorted(
            (
                {"path": path, "file_count": file_count}
                for path, file_count in directories.items()
                if file_count >= min_file_count
            ),
            key=lambda d: d["path"],
        )

        return directory_list

    def run(self, args: argparse.Namespace) -> int:
        request_root = args.path
        instance, checkout, rel_path = cmd_util.require_checkout(args, request_root)

        with instance.get_thrift_client() as client:
            inode_results = client.debugInodeStatus(
                bytes(checkout.path), bytes(rel_path)
            )

        read_files, written_files = split_inodes_by_operation_type(inode_results)
        operations = {
            "summary": {
                "read": self.make_summary(read_files),
                "written": self.make_summary(written_files),
            },
            "details": {
                "read_files": self.make_file_entries(read_files),
                "written_files": self.make_file_entries(written_files),
            },
        }
        json.dump(operations, fp=sys.stdout, indent=4, separators=(",", ": "))
        return 0


@debug_cmd("fuse_calls", "Show data about outstanding fuse calls")
class FuseCallsCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", help="The path to the eden mount point.")

    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout.buffer
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)
        with instance.get_thrift_client() as client:
            outstanding_call = client.debugOutstandingFuseCalls(bytes(checkout.path))

        out.write(b"Number of outstanding Calls: %d\n" % len(outstanding_call))
        for count, call in enumerate(outstanding_call):
            out.write(b"Call %d\n" % (count + 1))
            out.write(b"\tlen: %d\n" % call.len)
            out.write(b"\topcode: %d\n" % call.opcode)
            out.write(b"\tunique: %d\n" % call.unique)
            out.write(b"\tnodeid: %d\n" % call.nodeid)
            out.write(b"\tuid: %d\n" % call.uid)
            out.write(b"\tgid: %d\n" % call.gid)
            out.write(b"\tpid: %d\n" % call.pid)

        return 0


def _print_inode_info(inode_info: TreeInodeDebugInfo, out: IO[bytes]) -> None:
    out.write(inode_info.path + b"\n")
    out.write(b"  Inode number:  %d\n" % inode_info.inodeNumber)
    out.write(b"  Ref count:     %d\n" % inode_info.refcount)
    out.write(b"  Materialized?: %s\n" % str(inode_info.materialized).encode())
    out.write(b"  Object ID:     %s\n" % hash_str(inode_info.treeHash).encode())
    out.write(b"  Entries (%d total):\n" % len(inode_info.entries))
    for entry in inode_info.entries:
        if entry.loaded:
            loaded_flag = "L"
        else:
            loaded_flag = "-"

        file_type_str, perms = _parse_mode(entry.mode)
        line = "    {:9} {} {:4o} {} {:40} {}\n".format(
            entry.inodeNumber,
            file_type_str,
            perms,
            loaded_flag,
            hash_str(entry.hash),
            escape_path(entry.name),
        )
        out.write(line.encode())


@debug_cmd("getpath", "Get the eden path that corresponds to an inode number")
class GetPathCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )
        parser.add_argument(
            "number",
            type=int,
            help="Display information for the specified inode number.",
        )

    def run(self, args: argparse.Namespace) -> int:
        path = args.path or os.getcwd()
        instance, checkout, _rel_path = cmd_util.require_checkout(args, path)

        with instance.get_thrift_client() as client:
            inodePathInfo = client.debugGetInodePath(bytes(checkout.path), args.number)

        state = "loaded" if inodePathInfo.loaded else "unloaded"
        resolved_path = (
            checkout.path.joinpath(os.fsdecode(inodePathInfo.path))
            if inodePathInfo.linked
            else "[unlinked]"
        )
        print(f"{state} {resolved_path}")
        return 0


@debug_cmd("unload", "Unload unused inodes")
class UnloadInodesCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            help="The path to the eden mount point.  If a subdirectory inside "
            "a mount point is specified, only inodes under the "
            "specified subdirectory will be unloaded.",
        )
        parser.add_argument(
            "age",
            type=float,
            nargs="?",
            default=0,
            help="Minimum age of the inodes to be unloaded in seconds",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client() as client:
            # set the age in nanoSeconds
            age = TimeSpec()
            age.seconds = int(args.age)
            age.nanoSeconds = int((args.age - age.seconds) * 10 ** 9)
            count = client.unloadInodeForPath(
                bytes(checkout.path), bytes(rel_path), age
            )

            unload_path = checkout.path.joinpath(rel_path)
            print(f"Unloaded {count} inodes under {unload_path}")

        return 0


@debug_cmd("flush_cache", "Flush kernel cache for inode")
class FlushCacheCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path", help="Path to a directory/file inside an eden mount."
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client() as client:
            client.invalidateKernelInodeCache(bytes(checkout.path), bytes(rel_path))

        return 0


@debug_cmd("log", "Display the eden log file")
class LogCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        # Display eden's log with the system pager if possible.  We could
        # add a --tail option.
        instance = cmd_util.get_eden_instance(args)

        eden_log_path = instance.get_log_path()
        if not eden_log_path.exists():
            print(f"No log file found at {eden_log_path}", file=sys.stderr)
            return 1

        pager_env = os.getenv("PAGER")
        if pager_env:
            pager_cmd = shlex.split(pager_env)
        else:
            pager_cmd = ["less"]
        pager_cmd.append(str(eden_log_path))

        os.execvp(pager_cmd[0], pager_cmd)
        raise Exception("we should never reach here")


@debug_cmd("logging", "Display or modify logging configuration for the edenfs daemon")
class LoggingCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-a",
            "--all",
            action="store_true",
            help="Show the configuration of all logging categories, even ones "
            "with default configuration settings",
        )
        parser.add_argument(
            "--reset",
            action="store_true",
            help="Fully reset the logging config to the specified settings rather "
            "than updating the current configuration with the new settings.  "
            "(Beware that you need to specify log handlers unless you want them "
            "all to be deleted.)",
        )
        parser.add_argument(
            "config",
            type=str,
            nargs="?",
            help="A log configuration string to use to modify the log settings.  See "
            "folly/logging/docs/Config.md (https://git.io/fNZhr)"
            " for syntax documentation.  The most basic syntax is CATEGORY=LEVEL.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)

        if args.reset and args.config is None:
            # The configuration to use if the caller specifies --reset with no
            # explicit config argument.
            # pyre-fixme[16]: `Namespace` has no attribute `config`.
            args.config = (
                "WARN:default,eden=DBG2; default=stream:stream=stderr,async=true"
            )

        with instance.get_thrift_client() as client:
            if args.config is not None:
                if args.reset:
                    print(f"Resetting logging configuration to {args.config!r}")
                    client.setOption("logging_full", args.config)
                else:
                    print(f"Updating logging configuration with {args.config!r}")
                    client.setOption("logging", args.config)
                print("Updated configuration.  New config settings:")
            else:
                print("Current logging configuration:")

            if args.all:
                config_str = client.getOption("logging_full")
            else:
                config_str = client.getOption("logging")
            self.print_config(config_str)

        return 0

    def print_config(self, config_str: str) -> None:
        config = json.loads(config_str)

        handler_fmt = "  {:12} {:12} {}"
        separator = "  " + ("-" * 76)

        print("=== Log Handlers ===")
        if not config["handlers"]:
            print("  Warning: no log handlers configured!")
        else:
            print(handler_fmt.format("Name", "Type", "Options"))
            print(separator)
            for name, handler in sorted(config["handlers"].items()):
                options_str = ", ".join(
                    sorted("{}={}".format(k, v) for k, v in handler["options"].items())
                )
                print(handler_fmt.format(name, handler["type"], options_str))

        print("\n=== Log Categories ===")
        category_fmt = "  {:50} {:12} {}"
        print(category_fmt.format("Name", "Level", "Handlers"))
        print(separator)
        for name, category in sorted(config["categories"].items()):
            # For categories that do not inherit their parent's level (unusual)
            # show the level with a trailing '!'
            # Don't do this for the root category, though--it never inherits it's
            # parent's level since it has no parent.
            level_str = category["level"]
            if not category["inherit"] and name != "":
                level_str = level_str + "!"

            # Print the root category name as '.' instead of the empty string just
            # to help make it clear that there is a category name here.
            # (The logging config parsing code accepts '.' as the root category
            # name too.)
            if name == "":
                name = "."

            handlers_str = ", ".join(category["handlers"])
            print(category_fmt.format(name, level_str, handlers_str))


# set_log_level is deprecated.
# We should delete it in a few weeks (from 2018-07-18).
# The debugSetLogLevel() API in eden/fs/service/eden.thrift can also be removed at the
# same time.
@debug_cmd("set_log_level", help=None)
class SetLogLevelCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("category", type=str, help="Period-separated log category.")
        parser.add_argument(
            "level",
            type=str,
            help="Log level string as understood by stringToLogLevel.",
        )

    def run(self, args: argparse.Namespace) -> int:
        print(
            "The set_log_level command is deprecated.  "
            "Use `eden debug logging` instead:"
        )
        log_arg = shlex.quote(f"{args.category}={args.level}")
        print(f"  eden debug logging {log_arg}")
        return 1


@debug_cmd("journal_set_memory_limit", "Sets the journal memory limit")
class DebugJournalSetMemoryLimitCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "limit",
            type=int,
            help="The amount of memory (in bytes) that the journal can keep.",
        )
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client() as client:
            try:
                client.setJournalMemoryLimit(bytes(checkout.path), args.limit)
            except EdenError as err:
                print(err, file=sys.stderr)
                return 1
            return 0


@debug_cmd("journal_get_memory_limit", "Gets the journal memory limit")
class DebugJournalGetMemoryLimitCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client() as client:
            try:
                mem = client.getJournalMemoryLimit(bytes(checkout.path))
            except EdenError as err:
                print(err, file=sys.stderr)
                return 1
            print("Journal memory limit is " + stats_print.format_size(mem))
            return 0


@debug_cmd(
    "flush_journal",
    "Flushes the journal, and causes any subscribers to get a truncated result",
)
class DebugFlushJournalCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client() as client:
            try:
                client.flushJournal(bytes(checkout.path))
            except EdenError as err:
                print(err, file=sys.stderr)
                return 1
            return 0


@debug_cmd("journal", "Prints the most recent entries from the journal")
class DebugJournalCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-n",
            "--limit",
            type=int,
            default=1000,
            help="The number of journal entries to print.",
        )
        parser.add_argument(
            "-e",
            "--pattern",
            type=str,
            help="Show only deltas for paths matching this pattern. "
            "Specify '^((?!^\\.hg/).)*$' to exclude the .hg/ directory.",
        )
        parser.add_argument(
            "-f",
            "--follow",
            action="store_true",
            default=False,
            help="Output appended data as the journal grows.",
        )
        parser.add_argument(
            "-i",
            "--ignore-case",
            action="store_true",
            default=False,
            help="Ignore case in the pattern specified by --pattern.",
        )
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an Eden mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        pattern: Optional[Pattern[bytes]] = None
        if args.pattern:
            pattern_bytes = args.pattern.encode("utf-8")
            flags = re.IGNORECASE if args.ignore_case else 0
            pattern = re.compile(pattern_bytes, flags)

        instance, checkout, _ = cmd_util.require_checkout(args, args.path)
        mount = bytes(checkout.path)

        def refresh(params):
            with instance.get_thrift_client() as client:
                journal = client.debugGetRawJournal(params)

            deltas = journal.allDeltas
            if len(deltas) == 0:
                seq_num = params.fromSequenceNumber
            else:
                seq_num = deltas[0].fromPosition.sequenceNumber + 1
                _print_raw_journal_deltas(reversed(deltas), pattern)

            return seq_num

        try:
            params = DebugGetRawJournalParams(
                mountPoint=mount, fromSequenceNumber=1, limit=args.limit
            )
            seq_num = refresh(params)
            while args.follow:
                REFRESH_SEC = 2
                time.sleep(REFRESH_SEC)
                params = DebugGetRawJournalParams(
                    mountPoint=mount, fromSequenceNumber=seq_num
                )
                seq_num = refresh(params)
        except EdenError as err:
            print(err, file=sys.stderr)
            return 1
        except KeyboardInterrupt:
            if args.follow:
                pass
            else:
                raise

        return 0


def _print_raw_journal_deltas(
    deltas: Iterator[DebugJournalDelta], pattern: Optional[Pattern[bytes]]
) -> None:
    matcher: Callable[[bytes], bool] = (lambda x: True) if pattern is None else cast(
        Any, pattern.match
    )

    labels = {
        (False, False): "_",
        (False, True): "A",
        (True, False): "R",
        (True, True): "M",
    }

    for delta in deltas:
        entries: List[str] = []

        for path, info in delta.changedPaths.items():
            if not matcher(path):
                continue

            label = labels[(info.existedBefore, info.existedAfter)]
            entries.append(f"{label} {os.fsdecode(path)}")

        for path in delta.uncleanPaths:
            entries.append(f"X {os.fsdecode(path)}")

        # Only print journal entries if they changed paths that matched the matcher
        # or if they change the current working directory commit.
        if entries or delta.fromPosition.snapshotHash != delta.toPosition.snapshotHash:
            _print_journal_entry(delta, entries)


def _print_journal_entry(delta: DebugJournalDelta, entries: List[str]) -> None:
    if delta.fromPosition.snapshotHash != delta.toPosition.snapshotHash:
        from_commit = hash_str(delta.fromPosition.snapshotHash)
        to_commit = hash_str(delta.toPosition.snapshotHash)
        commit_ids = f"{from_commit} -> {to_commit}"
    else:
        commit_ids = hash_str(delta.toPosition.snapshotHash)

    if delta.fromPosition.sequenceNumber != delta.toPosition.sequenceNumber:
        print(
            f"MERGE {delta.fromPosition.sequenceNumber}-"
            f"{delta.toPosition.sequenceNumber} {commit_ids}"
        )
    else:
        print(f"DELTA {delta.fromPosition.sequenceNumber} {commit_ids}")

    if entries:
        entries.sort()
        print("  " + "\n  ".join(entries))


@debug_cmd("thrift", "Invoke a thrift function")
class DebugThriftCmd(Subcmd):
    args_suffix = "_args"
    result_suffix = "_result"

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-l",
            "--list",
            action="store_true",
            help="List the available thrift functions.",
        )
        parser.add_argument(
            "--eval-all-args",
            action="store_true",
            help="Always pass all arguments through eval(), even for plain strings.",
        )
        parser.add_argument(
            "--json", action="store_true", help="Attempt to encode the result as JSON."
        )
        parser.add_argument(
            "function_name", nargs="?", help="The thrift function to call."
        )
        parser.add_argument(
            "args", nargs="*", help="The arguments to the thrift function."
        )

    def run(self, args: argparse.Namespace) -> int:
        if args.list:
            self._list_functions()
            return 0

        if not args.function_name:
            print(f"Error: no function name specified", file=sys.stderr)
            print(
                "Use the --list argument to see a list of available functions, or "
                "specify a function name",
                file=sys.stderr,
            )
            return 1

        # Look up the function information
        try:
            fn_info = thrift.util.inspect.get_function_info(
                EdenService, args.function_name
            )
        except thrift.util.inspect.NoSuchFunctionError:
            print(f"Error: unknown function {args.function_name!r}", file=sys.stderr)
            print(
                'Run "eden debug thrift --list" to see a list of available functions',
                file=sys.stderr,
            )
            return 1

        if len(args.args) != len(fn_info.arg_specs):
            print(
                f"Error: {args.function_name} requires {len(fn_info.arg_specs)} "
                f"arguments, but {len(args.args)} were supplied>",
                file=sys.stderr,
            )
            return 1

        python_args = self._eval_args(
            args.args, fn_info, eval_strings=args.eval_all_args
        )

        def lookup_module_member(modules, name):
            for module in modules:
                try:
                    return getattr(module, name)
                except AttributeError:
                    continue
            raise AttributeError(f"Failed to find {name} in {modules}")

        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client() as client:
            fn = getattr(client, args.function_name)
            result = fn(**python_args)
            if args.json:
                # The following back-and-forth is required to reliably
                # convert a Python Thrift client result into its JSON
                # form. The Python Thrift client returns native Python
                # lists and dicts for lists and maps, but they cannot
                # be passed directly to TSimpleJSONProtocol. Instead,
                # map the result back into a Thrift message, and then
                # serialize that as JSON. Finally, strip the message
                # container.
                #
                # NOTE: Stripping the root object means the output may
                # not have a root dict or array, which is required by
                # most JSON specs. But Python's json module and jq are
                # both fine with this deviation.
                result_type = lookup_module_member(
                    [EdenService, BaseService], args.function_name + "_result"
                )
                json_data = Serializer.serialize(
                    TSimpleJSONProtocolFactory(), result_type(result)
                )
                json.dump(
                    # If the method returns void, json_data will not
                    # have a "success" field. Print `null` in that
                    # case.
                    json.loads(json_data).get("success"),
                    sys.stdout,
                    sort_keys=True,
                    indent=2,
                )
                sys.stdout.write("\n")
            else:
                print(result)

        return 0

    def _eval_args(
        self, args: List[str], fn_info: thrift.util.inspect.Function, eval_strings: bool
    ) -> Dict[str, Any]:
        from thrift.Thrift import TType

        code_globals = {key: getattr(eden_ttypes, key) for key in dir(eden_ttypes)}
        parsed_args = {}
        for arg, arg_spec in zip(args, fn_info.arg_specs):
            (
                _field_id,
                thrift_type,
                arg_name,
                _extra_spec,
                _default,
                _required,
            ) = arg_spec
            # If the argument is a string type, don't pass it through eval.
            # This is purely to make it easier for humans to input strings.
            if not eval_strings and thrift_type == TType.STRING:
                parsed_arg = arg
            else:
                code = compile(arg, "<command_line>", "eval", 0, 1)
                parsed_arg = eval(code, code_globals.copy())
            parsed_args[arg_name] = parsed_arg

        return parsed_args

    def _list_functions(self) -> None:
        # Report functions by module, from parent service downwards
        modules = thrift.util.inspect.get_service_module_hierarchy(EdenService)
        for module in reversed(modules):
            module_functions = thrift.util.inspect.list_service_functions(module)
            print(f"From {module.__name__}:")

            for _fn_name, fn_info in sorted(module_functions.items()):
                print(f"  {fn_info}")


@subcmd_mod.subcmd("debug", "Internal commands for examining eden state")
# pyre-fixme[13]: Attribute `parser` is never initialized.
class DebugCmd(Subcmd):
    parser: argparse.ArgumentParser

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        # The "debug_posix" module contains other debug subcommands that are
        # supported on POSIX platforms (basically, not Windows).  Import this module
        # if we aren't running on Windows.  This will make sure it has registered all of
        # its subcommands in our debug_cmd.commands list.
        if os.name != "nt":
            from . import debug_posix  # noqa: F401

        # Save the parser so we can use it to print help in run() if we are
        # called with no arguments.
        self.parser = parser
        self.add_subcommands(parser, debug_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        self.parser.print_help()
        return 0
