#!/usr/bin/env python3
#
# Copyright (c) 2004-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse
import binascii
import collections
import json
import os
import re
import shlex
import stat
import sys
from typing import (
    IO,
    Any,
    Callable,
    Dict,
    Iterator,
    List,
    Optional,
    Pattern,
    Tuple,
    cast,
)

import eden.dirstate
from facebook.eden.overlay.ttypes import OverlayDir
from facebook.eden.ttypes import (
    DebugGetRawJournalParams,
    DebugJournalDelta,
    FileDelta,
    JournalPosition,
    NoValueForKeyError,
    TimeSpec,
    TreeInodeDebugInfo,
)

from . import cmd_util, overlay as overlay_mod, subcmd as subcmd_mod, ui as ui_mod
from .config import EdenInstance
from .subcmd import Subcmd


debug_cmd = subcmd_mod.Decorator()


def get_mount_path(path: str) -> Tuple[bytes, bytes]:
    """
    Given a path inside an eden mount, find the path to the eden root.

    Returns a tuple of (eden_mount_path, relative_path)
    where relative_path is the path such that
    os.path.join(eden_mount_path, relative_path) refers to the same file as the
    original input path.
    """

    path = os.path.realpath(path)
    current_path = path
    if not os.path.isdir(current_path):
        current_path = os.path.dirname(current_path)
    mount_path = os.readlink(os.path.join(current_path, ".eden", "root"))
    rel_path = os.path.relpath(path, mount_path)
    if rel_path == ".":
        rel_path = ""
    return (os.fsencode(mount_path), os.fsencode(rel_path))


def escape_path(value: bytes) -> str:
    """
    Take a binary path value, and return a printable string, with special
    characters escaped.
    """

    def human_readable_byte(b: int) -> str:
        if b < 0x20 or b >= 0x7f:
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
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.mount)
        tree_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client() as client:
            entries = client.debugGetScmTree(mount, tree_id, localStoreOnly=local_only)

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
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.mount)
        blob_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client() as client:
            data = client.debugGetScmBlob(mount, blob_id, localStoreOnly=local_only)

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
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.mount)
        blob_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client() as client:
            info = client.debugGetScmBlobMetadata(
                mount, blob_id, localStoreOnly=local_only
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
        mount, _ = get_mount_path(path)
        _parents, _dirstate_tuples, copymap = _get_dirstate_data(mount)
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
        mount, _ = get_mount_path(path)
        _parents, dirstate_tuples, copymap = _get_dirstate_data(mount)
        out = ui_mod.get_output()
        entries = list(dirstate_tuples.items())
        out.writeln(f"Non-normal Files ({len(entries)}):", attr=out.BOLD)
        entries.sort(key=lambda entry: entry[0])  # Sort by key.
        for path, dirstate_tuple in entries:
            _print_hg_nonnormal_file(os.fsencode(path), dirstate_tuple, out)

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
        mount, rel_path = get_mount_path(args.path)
        _parents, dirstate_tuples, _copymap = _get_dirstate_data(mount)
        dirstate_tuple = dirstate_tuples.get(os.fsdecode(rel_path))
        out = ui_mod.get_output()
        if dirstate_tuple:
            _print_hg_nonnormal_file(rel_path, dirstate_tuple, out)
        else:
            instance = cmd_util.get_eden_instance(args)
            with instance.get_thrift_client() as client:
                try:
                    entry = client.getManifestEntry(mount, rel_path)
                    dirstate_tuple = ("n", entry.mode, 0)
                    _print_hg_nonnormal_file(rel_path, dirstate_tuple, out)
                except NoValueForKeyError:
                    print("No tuple for " + os.fsdecode(rel_path), file=sys.stderr)
                    return 1

        return 0


def _print_hg_nonnormal_file(
    rel_path: bytes, dirstate_tuple: Tuple[str, Any, int], out: ui_mod.Output
) -> None:
    status = _dirstate_char_to_name(dirstate_tuple[0])
    merge_state = _dirstate_merge_state_to_name(dirstate_tuple[2])

    out.writeln(f"{os.fsdecode(rel_path)}", fg=out.GREEN)
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
    mount: bytes
) -> Tuple[Tuple[bytes, bytes], Dict[str, Tuple[str, Any, int]], Dict[str, str]]:
    """Returns a tuple of (parents, dirstate_tuples, copymap).
    On error, returns None.
    """
    filename = os.path.join(os.fsdecode(mount), ".hg", "dirstate")
    with open(filename, "rb") as f:
        return eden.dirstate.read(f, filename)


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
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.path)
        with instance.get_thrift_client() as client:
            results = client.debugInodeStatus(mount, rel_path)

        out.write(b"%d loaded TreeInodes\n" % len(results))
        for inode_info in results:
            _print_inode_info(inode_info, out)
        return 0


@debug_cmd("fuse_calls", "Show data about outstanding fuse calls")
class FuseCallsCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", help="The path to the eden mount point.")

    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout.buffer
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.path)
        with instance.get_thrift_client() as client:
            outstanding_call = client.debugOutstandingFuseCalls(mount)

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


@debug_cmd("overlay", "Show data about the overlay")
class OverlayCmd(Subcmd):
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
        parser.add_argument("path", nargs="?", help="The path to the eden mount point.")

    def run(self, args: argparse.Namespace) -> int:
        self.args = args
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.path or os.getcwd())

        # Get the path to the overlay directory for this mount point
        client_dir = instance._get_client_dir_for_mount_point(os.fsdecode(mount))
        self.overlay = overlay_mod.Overlay(os.path.join(client_dir, "local"))

        if args.number is not None:
            self._display_overlay(args.number, "")
        elif rel_path:
            rel_path = os.path.normpath(rel_path)
            inode_number = self.overlay.lookup_path(os.fsdecode(rel_path))
            if inode_number is None:
                print(f"{rel_path} is not materialized", file=sys.stderr)
                return 1
            self._display_overlay(inode_number, os.fsdecode(rel_path))
        else:
            self._display_overlay(1, "/")

        return 0

    def _display_overlay(self, inode_number: int, path: str, level: int = 0) -> None:
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
            entry_path = os.path.join(path, name)
            self._display_overlay(entry.inodeNumber, entry_path, level + 1)

    def _print_overlay_tree(
        self, inode_number: int, path: str, tree_data: OverlayDir
    ) -> None:
        def hex(binhash) -> str:
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
        instance = cmd_util.get_eden_instance(args)
        mount, _ = get_mount_path(args.path or os.getcwd())

        with instance.get_thrift_client() as client:
            inodePathInfo = client.debugGetInodePath(mount, args.number)
        print(
            "%s %s"
            % (
                "loaded" if inodePathInfo.loaded else "unloaded",
                os.fsdecode(os.path.normpath(os.path.join(mount, inodePathInfo.path)))
                if inodePathInfo.linked
                else "unlinked",
            )
        )
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
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.path)

        with instance.get_thrift_client() as client:
            # set the age in nanoSeconds
            age = TimeSpec()
            age.seconds = int(args.age)
            age.nanoSeconds = int((args.age - age.seconds) * 10 ** 9)
            count = client.unloadInodeForPath(mount, rel_path, age)

            unload_path = os.fsdecode(os.path.join(mount, rel_path))
            print(f"Unloaded {count} inodes under {unload_path}")

        return 0


@debug_cmd("flush_cache", "Flush kernel cache for inode")
class FlushCacheCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path", help="Path to a directory/file inside an eden mount."
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        mount, rel_path = get_mount_path(args.path)

        with instance.get_thrift_client() as client:
            client.invalidateKernelInodeCache(mount, rel_path)

        return 0


@debug_cmd("log", "Display the eden log file")
class LogCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        # Display eden's log with the system pager if possible.  We could
        # add a --tail option.
        instance = cmd_util.get_eden_instance(args)

        eden_log_path = instance.get_log_path()
        if not os.path.exists(eden_log_path):
            print("No log file found at " + eden_log_path, file=sys.stderr)
            return 1

        pager_env = os.getenv("PAGER")
        if pager_env:
            pager_cmd = shlex.split(pager_env)
        else:
            pager_cmd = ["less"]
        pager_cmd.append(eden_log_path)

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
            args.config = (
                "WARN:default,eden=DBG2; default=stream:stream=stderr,async=true"
            )

        with instance.get_thrift_client() as client:
            if args.config is not None:
                if args.reset:
                    print(f"Resetting logging configuration to {args.config!r}")
                    config_str = client.setOption("logging_full", args.config)
                else:
                    print(f"Updating logging configuration with {args.config!r}")
                    config_str = client.setOption("logging", args.config)
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


@debug_cmd("journal", "Prints the most recent N entries from the journal")
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
        instance = cmd_util.get_eden_instance(args)
        mount, _ = get_mount_path(args.path or os.getcwd())

        with instance.get_thrift_client() as client:
            to_position = client.getCurrentJournalPosition(mount)
            from_sequence = max(to_position.sequenceNumber - args.limit, 0)
            from_position = JournalPosition(
                mountGeneration=to_position.mountGeneration,
                sequenceNumber=from_sequence,
                snapshotHash=b"",
            )

            params = DebugGetRawJournalParams(
                mountPoint=os.fsencode(mount),
                fromPosition=from_position,
                toPosition=to_position,
            )
            raw_journal = client.debugGetRawJournal(params)
            if args.pattern:
                flags = re.IGNORECASE if args.ignore_case else 0
                pattern: Optional[Pattern] = re.compile(args.pattern, flags)
            else:
                pattern = None
            # debugGetRawJournal() returns the most recent entries first, but
            # we want to display the oldest entries first, so we pass a reversed
            # iterator along.
            print_raw_journal_deltas(reversed(raw_journal.allDeltas), pattern)

        return 0


def print_raw_journal_deltas(
    deltas: Iterator[DebugJournalDelta], pattern: Optional[Pattern]
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
            entries.append(f"{label} {path}")

        for path in delta.uncleanPaths:
            entries.append(f"X {path}")

        if not entries:
            continue

        if delta.fromPosition.sequenceNumber != delta.toPosition.sequenceNumber:
            print(
                f"MERGE {delta.fromPosition.sequenceNumber}-"
                f"{delta.toPosition.sequenceNumber}"
            )
        else:
            print(f"DELTA {delta.fromPosition.sequenceNumber}")

        entries.sort()
        print("\n".join(entries))


@subcmd_mod.subcmd("debug", "Internal commands for examining eden state")
class DebugCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        # Save the parser so we can use it to print help in run() if we are
        # called with no arguments.
        self.parser = parser
        self.add_subcommands(parser, debug_cmd.commands)

    def run(self, args: argparse.Namespace) -> int:
        self.parser.print_help()
        return 0
