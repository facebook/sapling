#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import argparse
import binascii
import collections
import json
import os
import re
import shlex
import stat
import subprocess
import sys
import time
from pathlib import Path
from typing import (
    Any,
    Callable,
    cast,
    DefaultDict,
    Dict,
    IO,
    Iterator,
    List,
    Optional,
    Pattern,
    Tuple,
    Type,
    Union,
)

import eden.dirstate
import facebook.eden.ttypes as eden_ttypes
import thrift.util.inspect
from eden.fs.cli.cmd_util import get_eden_instance
from eden.thrift.legacy import EdenClient
from facebook.eden import EdenService
from facebook.eden.constants import DIS_REQUIRE_LOADED, DIS_REQUIRE_MATERIALIZED
from facebook.eden.ttypes import (
    DataFetchOrigin,
    DebugGetRawJournalParams,
    DebugGetScmBlobRequest,
    DebugJournalDelta,
    EdenError,
    MountId,
    NoValueForKeyError,
    ScmBlobOrError,
    ScmBlobWithOrigin,
    SyncBehavior,
    TimeSpec,
    TreeInodeDebugInfo,
)
from fb303_core import BaseService
from thrift.protocol.TSimpleJSONProtocol import TSimpleJSONProtocolFactory
from thrift.Thrift import TApplicationException
from thrift.util import Serializer

try:
    from tqdm import tqdm
except ModuleNotFoundError:

    def tqmd(x):
        return x


from . import (
    cmd_util,
    hg_util,
    prefetch_profile as prefetch_profile_mod,
    rage as rage_mod,
    stats_print,
    subcmd as subcmd_mod,
    tabulate,
    ui as ui_mod,
)
from .config import EdenCheckout, EdenInstance
from .subcmd import Subcmd
from .util import format_cmd, format_mount, print_stderr, split_inodes_by_operation_type


MB: int = 1024**2
debug_cmd = subcmd_mod.Decorator()


# This is backported from Python 3.9.
#
# TODO: Use argparse.BooleanOptionalAction when we
# can expect Python 3.9 or later.
class BooleanOptionalAction(argparse.Action):
    def __init__(
        self,
        option_strings,
        dest,
        default=None,
        type=None,
        choices=None,
        required=False,
        help=None,
        metavar=None,
    ):

        _option_strings = []
        for option_string in option_strings:
            _option_strings.append(option_string)

            if option_string.startswith("--"):
                option_string = "--no-" + option_string[2:]
                _option_strings.append(option_string)

        super().__init__(
            option_strings=_option_strings,
            dest=dest,
            nargs=0,
            default=default,
            type=type,
            choices=choices,
            required=required,
            help=help,
            metavar=metavar,
        )

    def __call__(self, parser, namespace, values, option_string=None):
        if option_string in self.option_strings:
            setattr(namespace, self.dest, not option_string.startswith("--no-"))

    def format_usage(self):
        return " | ".join(self.option_strings)


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


def object_id_str(value: bytes) -> str:
    # While we migrate the representation of object IDs, continue to support
    # older versions of EdenFS returning 20-byte binary hashes.
    if len(value) == 20:
        return hash_str(value)
    return value.decode("utf-8", errors="replace")


def parse_object_id(value: str) -> bytes:
    return value.encode()


@debug_cmd("parents", "Show EdenFS's current working copy parent")
class ParentsCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
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
            working_copy_parent, checked_out_revision = checkout.get_snapshot()
        except Exception as ex:
            print_stderr(f"error parsing EdenFS snapshot : {ex}")
            return 1

        if args.hg:
            hg_parents, _, _ = _get_dirstate_data(checkout)

            print("Mercurial p0: {}".format(self._commit_hex(hg_parents[0])))
            if hg_parents[1] != null_commit_id:
                print("Mercurial p1: {}".format(self._commit_hex(hg_parents[1])))
            print("EdenFS snapshot: {}".format(working_copy_parent))
        else:
            print(working_copy_parent)

        return 0


@debug_cmd("tree", "Show EdenFS's data for a source control tree")
class TreeCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-L",
            "--load",
            action=BooleanOptionalAction,
            default=True,
            help="Load data from the backing store if necessary",
        )
        parser.add_argument("mount", help="The EdenFS mount point path.")
        parser.add_argument("id", help="The tree ID")

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        tree_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client_legacy() as client:
            entries = client.debugGetScmTree(
                bytes(checkout.path), tree_id, localStoreOnly=local_only
            )

        max_object_id_len = max(
            (len(object_id_str(entry.id)) for entry in entries), default=0
        )
        for entry in entries:
            file_type_flags, perms = _parse_mode(entry.mode)
            print(
                "{} {:4o} {:<{}} {}".format(
                    file_type_flags,
                    perms,
                    object_id_str(entry.id),
                    max_object_id_len,
                    escape_path(entry.name),
                )
            )

        return 0


class Process:
    def __init__(self, pid, cmd, mount) -> None:
        self.pid = pid
        self.cmd = format_cmd(cmd)
        self.fetch_count = 0
        self.mount = format_mount(mount)

    def set_fetchs(self, fetch_counts: int) -> None:
        self.fetch_count = fetch_counts


@debug_cmd("processfetch", "List processes and fetch counts")
class ProcessFetchCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-s",
            "--short-cmdline",
            action="store_true",
            default=False,
            help="Show commands without arguments, otherwise show the entire cmdlines",
        )
        parser.add_argument(
            "-a",
            "--all-processes",
            action="store_true",
            default=False,
            help="Default option only lists recent processes. This option shows all "
            "processes from the beginning of this EdenFS. Old cmdlines might be unavailable",
        )
        parser.add_argument(
            "-m",
            "--mount",
            action="store_true",
            default=False,
            help="Show mount base name for each process",
        )

    def run(self, args: argparse.Namespace) -> int:
        # pyre-fixme[31]: Expression `Process())]` is not a valid type.
        processes: Dict[int, Process()] = {}

        header = ["PID", "FETCH COUNT", "CMD"]
        if args.mount:
            header.insert(1, "MOUNT")
        rows = []

        eden = cmd_util.get_eden_instance(args)
        with eden.get_thrift_client_legacy() as client:

            # Get the data in the past 16 seconds. All data is collected only within
            # this period except that fetchCountsByPid is from the beginning of start
            counts = client.getAccessCounts(16)

            for mount, accesses in counts.accessesByMount.items():
                # Get recent process accesses
                for pid, _ in accesses.accessCountsByPid.items():
                    cmd = counts.cmdsByPid.get(pid, b"<unknown>")
                    processes[pid] = Process(pid, cmd, mount)

                # When querying older versions of EdenFS fetchCountsByPid will be None
                fetch_counts_by_pid = accesses.fetchCountsByPid or {}

                # Set fetch counts for recent processes
                for pid, fetch_counts in fetch_counts_by_pid.items():
                    if pid not in processes:
                        if not args.all_processes:
                            continue
                        else:
                            cmd = counts.cmdsByPid.get(pid, b"<unknown>")
                            processes[pid] = Process(pid, cmd, mount)

                    processes[pid].set_fetchs(fetch_counts)

        sorted_processes = sorted(
            processes.items(), key=lambda x: x[1].fetch_count, reverse=True
        )

        for (pid, process) in sorted_processes:
            if process.fetch_count:
                row: Dict[str, str] = {}
                cmd = process.cmd
                if args.short_cmdline:
                    cmd = cmd.split()[0]
                row["PID"] = pid
                row["FETCH COUNT"] = process.fetch_count
                row["CMD"] = cmd
                if args.mount:
                    row["MOUNT"] = process.mount
                rows.append(row)

        print(tabulate.tabulate(header, rows))
        return 0


@debug_cmd(
    "blob",
    "Show EdenFS's data for a source control blob. Fetches from ObjectStore "
    "by default: use options to inspect different origins.",
)
class BlobCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        group = parser.add_mutually_exclusive_group()
        group.add_argument(
            "-o",
            "--object-cache-only",
            action="store_true",
            default=False,
            help="Only check the in memory object cache for the blob",
        )
        group.add_argument(
            "-l",
            "--local-store-only",
            action="store_true",
            default=False,
            help="Only check the EdenFS LocalStore for blob. ",
        )
        group.add_argument(
            "-d",  # d for "disk cache"
            "--hgcache-only",
            action="store_true",
            default=False,
            help="Only check the hgcache for the blob",
        )
        group.add_argument(
            "-r",
            "--remote-only",
            action="store_true",
            default=False,
            help="Only fetch the data from the servers. ",
        )
        group.add_argument(
            "-a",
            "--all",
            action="store_true",
            default=False,
            help="Fetch the blob from all storage locations and display their contents. ",
        )
        parser.add_argument(
            "mount",
            help="The EdenFS mount point path.",
        )
        parser.add_argument("id", help="The blob ID")

    def origin_to_text(self, origin: DataFetchOrigin) -> str:
        if origin == DataFetchOrigin.MEMORY_CACHE:
            return "object cache"
        elif origin == DataFetchOrigin.DISK_CACHE:
            return "local store"
        elif origin == DataFetchOrigin.LOCAL_BACKING_STORE:
            return "hgcache"
        elif origin == DataFetchOrigin.REMOTE_BACKING_STORE:
            return "servers"
        elif origin == DataFetchOrigin.ANYWHERE:
            return "EdenFS production data fetching process"
        return "<unknown>"

    def print_blob_or_error(self, blobOrError: ScmBlobOrError) -> None:
        if blobOrError.getType() == ScmBlobOrError.BLOB:
            sys.stdout.buffer.write(blobOrError.get_blob())
        else:
            error = blobOrError.get_error()
            sys.stdout.buffer.write(f"ERROR fetching data: {error}\n".encode())

    def print_all_blobs(self, blobs: List[ScmBlobWithOrigin]) -> None:
        non_error_blobs = []
        for blob in blobs:
            blob_found = blob.blob.getType() == ScmBlobOrError.BLOB
            pretty_origin = self.origin_to_text(blob.origin)
            pretty_blob_found = "hit" if blob_found else "miss"
            print(f"{pretty_origin}: {pretty_blob_found}")
            if blob_found:
                non_error_blobs.append(blob)

        if len(non_error_blobs) == 0:
            return
        if len(non_error_blobs) == 1:
            print("\n")
            sys.stdout.buffer.write(non_error_blobs[0].blob.get_blob())
            return

        blobs_match = True
        for blob in non_error_blobs[1::]:
            if blob.blob.get_blob() != non_error_blobs[0].blob.get_blob():
                blobs_match = False
                break

        if blobs_match:
            print("\nAll blobs match :) \n")
            sys.stdout.buffer.write(non_error_blobs[0].blob.get_blob())
        else:
            print("\n!!!!! Blob mismatch !!!!! \n")
            for blob in non_error_blobs:
                prety_fromwhere = self.origin_to_text(blob.origin)
                print(f"Blob from {prety_fromwhere}\n")
                print("-----------------------------\n")
                sys.stdout.buffer.write(blob.blob.get_blob())
                print("\n-----------------------------\n\n")

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        blob_id = parse_object_id(args.id)

        origin_flags = DataFetchOrigin.ANYWHERE
        if args.object_cache_only:
            origin_flags = DataFetchOrigin.MEMORY_CACHE
        elif args.local_store_only:
            origin_flags = DataFetchOrigin.DISK_CACHE
        elif args.hgcache_only:
            origin_flags = DataFetchOrigin.LOCAL_BACKING_STORE
        elif args.remote_only:
            origin_flags = DataFetchOrigin.REMOTE_BACKING_STORE
        elif args.all:
            origin_flags = (
                DataFetchOrigin.MEMORY_CACHE
                | DataFetchOrigin.DISK_CACHE
                | DataFetchOrigin.LOCAL_BACKING_STORE
                | DataFetchOrigin.REMOTE_BACKING_STORE
                | DataFetchOrigin.ANYWHERE
            )

        with instance.get_thrift_client_legacy() as client:
            data = client.debugGetBlob(
                DebugGetScmBlobRequest(
                    MountId(bytes(checkout.path)),
                    blob_id,
                    origin_flags,
                )
            )
            if args.all:
                self.print_all_blobs(data.blobs)
            else:
                self.print_blob_or_error(data.blobs[0].blob)

        return 0


@debug_cmd("blobmeta", "Show EdenFS's metadata about a source control blob")
class BlobMetaCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-L",
            "--load",
            action="store_true",
            default=False,
            help="Load data from the backing store if necessary",
        )
        parser.add_argument("mount", help="The EdenFS mount point path.")
        parser.add_argument("id", help="The blob ID")

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.mount)
        blob_id = parse_object_id(args.id)

        local_only = not args.load
        with instance.get_thrift_client_legacy() as client:
            info = client.debugGetScmBlobMetadata(
                bytes(checkout.path), blob_id, localStoreOnly=local_only
            )

        print("Blob ID: {}".format(args.id))
        print("Size:    {}".format(info.size))
        print("SHA1:    {}".format(hash_str(info.contentsSha1)))
        return 0


class MismatchedBlobSize:
    actual_blobsize: int
    cached_blobsize: int

    def __init__(self, actual_blobsize: int, cached_blobsize: int) -> None:
        self.actual_blobsize = actual_blobsize
        self.cached_blobsize = cached_blobsize


def check_blob_and_size_match(
    client: EdenClient, checkout: Path, identifying_hash: bytes
) -> Optional[MismatchedBlobSize]:
    try:
        response = client.debugGetBlob(
            DebugGetScmBlobRequest(
                mountId=MountId(bytes(checkout)),
                id=identifying_hash,
                origins=DataFetchOrigin.LOCAL_BACKING_STORE,  # We don't want to cause any network fetches.
            )
        )
        blob = None
        for blobFromACertainPlace in response.blobs:
            try:
                blob = blobFromACertainPlace.blob.get_blob()
            except AssertionError:
                # only care to check blobs that exist
                pass

        blobmeta = client.debugGetScmBlobMetadata(
            mountPoint=bytes(checkout),
            id=identifying_hash,
            localStoreOnly=True,  # We don't want to cause any network fetches.
        )
        if blob is not None and blobmeta.size != len(blob):
            return MismatchedBlobSize(
                actual_blobsize=len(blob), cached_blobsize=blobmeta.size
            )
    except EdenError:
        # we don't care if debugGetScmBlobV2 returns an EdenError because
        # we only care about data that has been read by the user and thus is
        # present locally being incorrect.
        # We don't care if debugGetScmBlobMetadata returns an EdenError because
        # we only care about cached data being incorrect.
        return None
    except TApplicationException as ex:
        # we don't care about older versions of eden being incompatible, we will
        # just run the check when we can.
        if ex.type == TApplicationException.UNKNOWN_METHOD:
            return None


def check_size_corruption(
    client: EdenClient,
    instance: EdenInstance,
    checkout: Path,
    loaded_tree_inodes: List[TreeInodeDebugInfo],
) -> int:
    # list of files whose size is wrongly cached in the local store
    local_store_corruption: List[Tuple[bytes, MismatchedBlobSize]] = []

    for loaded_dir in tqdm(loaded_tree_inodes):
        for dirent in loaded_dir.entries:
            if not stat.S_ISREG(dirent.mode) or dirent.materialized:
                continue
            result = check_blob_and_size_match(client, checkout, dirent.hash)
            if result is not None:
                local_store_corruption.append((dirent.name, result))

    if local_store_corruption:
        print(f"{len(local_store_corruption)} corrupted sizes in the local store")
        for (filename, mismatch) in local_store_corruption[:10]:
            print(
                f"{filename} --"
                f"actual size: {mismatch.actual_blobsize} -- "
                f"local store blob size: {mismatch.cached_blobsize}"
            )
        if len(local_store_corruption) > 10:
            print("...")
        return 1
    return 0


@debug_cmd(
    "sizecorruption", "Check if the metadata blob size match the actual blob size"
)
class SizeCorruptionCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        checkouts = instance.get_mounts()

        number_effected_mounts = 0
        with instance.get_thrift_client_legacy() as client:
            for path in sorted(checkouts.keys()):
                print(f"Checking {path}")
                inodes = client.debugInodeStatus(
                    bytes(path),
                    b"",
                    flags=DIS_REQUIRE_LOADED,
                    sync=SyncBehavior(),
                )

                number_effected_mounts += check_size_corruption(
                    client, instance, path, inodes
                )
        return number_effected_mounts


_FILE_TYPE_FLAGS: Dict[int, str] = {
    stat.S_IFREG: "f",
    stat.S_IFDIR: "d",
    stat.S_IFLNK: "l",
}


def _parse_mode(mode: int) -> Tuple[str, int]:
    """
    Take a mode value, and return a tuple of (file_type, permissions)
    where file type is a one-character flag indicating if this is a file,
    directory, or symbolic link.
    """
    file_type_str = _FILE_TYPE_FLAGS.get(stat.S_IFMT(mode), "?")
    perms = mode & 0o7777
    return file_type_str, perms


@debug_cmd("buildinfo", "Show the build info for the EdenFS server")
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


@debug_cmd(
    "gc_process_fetch", "clear and start a new recording of process fetch counts"
)
class GcProcessFetchCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "mount",
            nargs="?",
            help="The path to an EdenFS mount point. If not specified,"
            " process fetch data will be cleared for all mounts.",
        )

    def run(self, args: argparse.Namespace) -> int:
        eden = cmd_util.get_eden_instance(args)
        with eden.get_thrift_client_legacy() as client:
            if args.mount:
                instance, checkout, _rel_path = cmd_util.require_checkout(
                    args, args.mount
                )
                client.clearFetchCountsByMount(bytes(checkout.path))
            else:
                client.clearFetchCounts()
        return 0


@debug_cmd("clear_local_caches", "Clears local caches of objects stored in RocksDB")
class ClearLocalCachesCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            client.debugClearLocalStoreCaches()
        return 0


@debug_cmd("compact_local_storage", "Asks RocksDB to compact its storage")
class CompactLocalStorageCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            client.debugCompactLocalStorage()
        return 0


@debug_cmd("hg_copy_map_get_all", "Copymap for dirstate")
class HgCopyMapGetAllCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
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
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
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
    filename = checkout.hg_dot_path.joinpath("dirstate")
    with filename.open("rb") as f:
        return eden.dirstate.read(f, str(filename))


@debug_cmd("inode", "Show data about loaded inodes")
class InodeCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            help="The path to the EdenFS mount point.  If a subdirectory inside "
            "a mount point is specified, only data about inodes under the "
            "specified subdirectory will be reported.",
        )

    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout.buffer
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)
        with instance.get_thrift_client_legacy() as client:
            results = client.debugInodeStatus(
                bytes(checkout.path),
                bytes(rel_path),
                flags=0,
                sync=SyncBehavior(),
            )

        out.write(b"%d loaded TreeInodes\n" % len(results))
        for inode_info in results:
            _print_inode_info(inode_info, out)
        return 0


@debug_cmd(
    "modified",
    "Enumerate all potentially-modified inode paths",
    aliases=["materialized"],
)
class MaterializedCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            default=None,
            nargs="?",
            help="The path to the EdenFS mount point.  If a subdirectory inside "
            "a mount point is specified, only data about inodes under the "
            "specified subdirectory will be reported.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client_legacy() as client:
            results = client.debugInodeStatus(
                bytes(checkout.path),
                bytes(rel_path),
                DIS_REQUIRE_MATERIALIZED,
                sync=SyncBehavior(),
            )

        if not results:
            return 0

        by_inode = {}
        for result in results:
            by_inode[result.inodeNumber] = result

        def walk(ino, path):
            print(os.fsdecode(path if path else b"/"))
            try:
                inode = by_inode[ino]
            except KeyError:
                return
            for entry in inode.entries:
                if entry.materialized:
                    walk(entry.inodeNumber, os.path.join(path, entry.name))

        root = results[0]
        # In practice, this condition is always true, because edenfs creates .eden at startup.
        if root.materialized:
            walk(root.inodeNumber, root.path)

        return 0


@debug_cmd("file_stats", "Show data about loaded and written files")
class FileStatsCMD(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", help="The path to the EdenFS mount point")

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

        with instance.get_thrift_client_legacy() as client:
            inode_results = client.debugInodeStatus(
                bytes(checkout.path), bytes(rel_path), flags=0, sync=SyncBehavior()
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
        parser.add_argument("path", help="The path to the EdenFS mount point.")

    def run(self, args: argparse.Namespace) -> int:
        out = sys.stdout.buffer
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)
        with instance.get_thrift_client_legacy() as client:
            outstanding_call = client.debugOutstandingFuseCalls(bytes(checkout.path))

        out.write(b"Outstanding FUSE calls: %d\n" % len(outstanding_call))
        for count, call in enumerate(outstanding_call):
            out.write(b"Call %d\n" % (count + 1))
            out.write(b"\topcode: %d\n" % call.opcode)
            out.write(b"\tunique: %d\n" % call.unique)
            out.write(b"\tnodeid: %d\n" % call.nodeid)
            out.write(b"\tuid: %d\n" % call.uid)
            out.write(b"\tgid: %d\n" % call.gid)
            out.write(b"\tpid: %d\n" % call.pid)

        return 0


@debug_cmd(
    "start_recording",
    "Start an activity recording session and get its id",
)
class StartRecordingCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--output-dir",
            help="The output dir to store the performance profile",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, os.getcwd())
        with instance.get_thrift_client_legacy() as client:
            result = client.debugStartRecordingActivity(
                bytes(checkout.path), args.output_dir.encode()
            )
            if result.unique:
                sys.stdout.buffer.write(str(result.unique).encode())
                return 0
        print(f"Fail to start recording at {args.output_dir}", file=sys.stderr)
        return 1


@debug_cmd(
    "stop_recording",
    "Stop the given activity recording session and get the output file",
)
class StopRecordingCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--unique",
            type=int,
            help="The id of the recording to stop",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, os.getcwd())
        output_path: Optional[bytes] = None
        with instance.get_thrift_client_legacy() as client:
            result = client.debugStopRecordingActivity(
                bytes(checkout.path), args.unique
            )
            output_path = result.path
        if output_path is None:
            print(f"Fail to stop recording: {args.unique}", file=sys.stderr)
            return 1
        sys.stdout.buffer.write(output_path)
        return 0


@debug_cmd(
    "list_recordings",
    "List active activity recording sessions",
)
class ListRecordingsCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, os.getcwd())
        with instance.get_thrift_client_legacy() as client:
            result = client.debugListActivityRecordings(bytes(checkout.path))
            if not result.recordings:
                print("There is no active activity recording sessions.")
            else:
                for recording in result.recordings:
                    path = recording.path
                    print(
                        f"ID: {recording.unique} Output file: {'' if path is None else path.decode()}"
                    )
        return 0


def _print_inode_info(inode_info: TreeInodeDebugInfo, out: IO[bytes]) -> None:
    out.write(inode_info.path + b"\n")
    out.write(b"  Inode number:  %d\n" % inode_info.inodeNumber)
    out.write(b"  Ref count:     %d\n" % inode_info.refcount)
    out.write(b"  Materialized?: %s\n" % str(inode_info.materialized).encode())
    out.write(b"  Object ID:     %s\n" % object_id_str(inode_info.treeHash).encode())
    out.write(b"  Entries (%d total):\n" % len(inode_info.entries))

    max_object_id_len = max(
        (len(object_id_str(entry.hash)) for entry in inode_info.entries), default=0
    )

    for entry in inode_info.entries:
        if entry.loaded:
            loaded_flag = "L"
        else:
            loaded_flag = "-"

        file_type_str, perms = _parse_mode(entry.mode)
        line = "    {:9} {} {:4o} {} {:<{}} {}\n".format(
            entry.inodeNumber,
            file_type_str,
            perms,
            loaded_flag,
            object_id_str(entry.hash),
            max_object_id_len,
            escape_path(entry.name),
        )
        out.write(line.encode())


@debug_cmd("getpath", "Get the EdenFS path that corresponds to an inode number")
class GetPathCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "path",
            nargs="?",
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
        )
        parser.add_argument(
            "number",
            type=int,
            help="Display information for the specified inode number.",
        )

    def run(self, args: argparse.Namespace) -> int:
        path = args.path or os.getcwd()
        instance, checkout, _rel_path = cmd_util.require_checkout(args, path)

        with instance.get_thrift_client_legacy() as client:
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
            help="The path to the EdenFS mount point.  If a subdirectory inside "
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

        with instance.get_thrift_client_legacy() as client:
            # set the age in nanoSeconds
            age = TimeSpec()
            age.seconds = int(args.age)
            age.nanoSeconds = int((args.age - age.seconds) * 10**9)
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
            "path", help="Path to a directory/file inside an EdenFS mount."
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client_legacy() as client:
            client.invalidateKernelInodeCache(bytes(checkout.path), bytes(rel_path))

        return 0


@debug_cmd("log", "Display/Gather the EdenFS log file. Defaults to Display mode.")
class LogCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        group = parser.add_mutually_exclusive_group()
        group.add_argument(
            "--upload",
            action="store_true",
            help=(
                "Gather logs from eden and uploads them externally. "
                "This uses the upload tool specified by the rage.reporter config value"
            ),
        )
        group.add_argument(
            "--stdout",
            action="store_true",
            help="Print the logs to stdout: ignore reporter.",
        )
        parser.add_argument(
            "--full",
            action="store_true",
            help="Gather the full logs from eden. Works with the upload and stdout options",
        )
        parser.add_argument(
            "--size",
            type=int,
            default=1000000,
            help=(
                "The amount of the logs we should gather in bytes. "
                "Size is ignored if --full is set. Defaults to 1M. Works with --upload and --stdout"
            ),
        )
        parser.add_argument(
            "--path",
            action="store_true",
            help="Print the location of the EdenFS log file",
        )

    def upload_logs(
        self, args: argparse.Namespace, instance: EdenInstance, eden_log_path: Path
    ) -> int:
        # For ease of use, just use the same rage reporter
        rage_processor = instance.get_config_value("rage.reporter", default="")

        proc: Optional[subprocess.Popen] = None
        if rage_processor and not args.stdout:
            proc = subprocess.Popen(shlex.split(rage_processor), stdin=subprocess.PIPE)
            sink = proc.stdin
        else:
            proc = None
            sink = sys.stdout.buffer

        # pyre-fixme[6]: Expected `IO[bytes]` for 2nd param but got
        #  `Optional[typing.IO[typing.Any]]`.
        rage_mod.print_log_file(eden_log_path, sink, args.full, args.size)
        if proc:
            # pyre-fixme[16]: `Optional` has no attribute `close`.
            sink.close()
            proc.wait()
        return 0

    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)

        eden_log_path = instance.get_log_path()
        if not eden_log_path.exists():
            print(f"No log file found at {eden_log_path}", file=sys.stderr)
            return 1

        if args.path:
            print(eden_log_path, file=sys.stdout)
            return 0

        if args.stdout or args.upload:
            return self.upload_logs(args, instance, eden_log_path)
        else:
            # Display eden's log with the system pager if possible.  We could
            # add a --tail option.
            pager_env = os.getenv("PAGER")
            if pager_env:
                pager_cmd = shlex.split(pager_env)
            else:
                pager_cmd = ["less", "+G"]
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
            args.config = (
                "WARN:default,eden=DBG2; default=stream:stream=stderr,async=true"
            )

        with instance.get_thrift_client_legacy() as client:
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
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client_legacy() as client:
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
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client_legacy() as client:
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
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = cmd_util.require_checkout(args, args.path)

        with instance.get_thrift_client_legacy() as client:
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
            help="The path to an EdenFS mount point. Uses `pwd` by default.",
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
            with instance.get_thrift_client_legacy() as client:
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
    matcher: Callable[[bytes], bool] = (
        (lambda x: True) if pattern is None else cast(Any, pattern.match)
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
        with instance.get_thrift_client_legacy() as client:
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


@debug_cmd("drop-fetch-requests", "Drop all pending source control object fetches")
class DropRequestsCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = cmd_util.get_eden_instance(args)
        with instance.get_thrift_client_legacy() as client:
            num_dropped = client.debugDropAllPendingRequests()
            print(f"Dropped {num_dropped} source control fetch requests")
            return 0


@subcmd_mod.subcmd("debug", "Internal commands for examining EdenFS state")
# pyre-fixme[13]: Attribute `parser` is never initialized.
class DebugCmd(Subcmd):
    parser: argparse.ArgumentParser

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        # The "debug_posix" module contains other debug subcommands that are
        # supported on POSIX platforms (basically, not Windows).  Import this module
        # if we aren't running on Windows.  This will make sure it has registered all of
        # its subcommands in our debug_cmd.commands list.
        if sys.platform != "win32":
            from . import debug_posix  # noqa: F401
        else:
            from . import debug_windows  # noqa: F401

        subcmd_add_list: List[Type[Subcmd]] = []
        # Save the parser so we can use it to print help in run() if we are
        # called with no arguments.
        self.parser = parser
        self.add_subcommands(parser, debug_cmd.commands + subcmd_add_list)

    def run(self, args: argparse.Namespace) -> int:
        self.parser.print_help()
        return 0
