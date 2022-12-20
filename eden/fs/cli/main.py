#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import argparse
import asyncio
import enum
import errno
import inspect
import json
import os
import platform
import shlex
import shutil
import signal
import socket
import subprocess
import sys
import traceback
import typing
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple, Type

import thrift.transport
from eden.fs.cli.buck import get_buck_command, run_buck_command
from eden.fs.cli.telemetry import TelemetrySample
from eden.fs.cli.util import (
    check_health_using_lockfile,
    EdenStartError,
    get_protocol,
    is_apple_silicon,
    wait_for_instance_healthy,
)
from eden.thrift.legacy import EdenClient, EdenNotRunningError
from facebook.eden import EdenService
from facebook.eden.ttypes import MountState
from fb303_core.ttypes import fb303_status

from . import (
    buck,
    config as config_mod,
    daemon,
    daemon_util,
    debug as debug_mod,
    doctor as doctor_mod,
    hg_util,
    mtab,
    prefetch as prefetch_mod,
    prefetch_profile as prefetch_profile_mod,
    rage as rage_mod,
    redirect as redirect_mod,
    stats as stats_mod,
    subcmd as subcmd_mod,
    top as top_mod,
    trace as trace_mod,
    ui,
    util,
    version as version_mod,
)
from .cmd_util import get_eden_instance, prompt_confirmation, require_checkout
from .config import EdenCheckout, EdenInstance, ListMountInfo
from .stats_print import format_size
from .subcmd import Subcmd
from .util import get_environment_suitable_for_subprocess, print_stderr, ShutdownError

try:
    from .facebook.util import (
        get_migration_success_message,
        migration_restart_help,
        stop_internal_processes,
    )
except ImportError:

    migration_restart_help = "This migrates ALL your mounts to a new mount protocol."

    def get_migration_success_message(migrate_to) -> str:
        return f"Successfully migrated all your mounts to {migrate_to}."

    def stop_internal_processes(_: str) -> None:
        pass


if sys.platform != "win32":
    from . import fsck as fsck_mod


subcmd = subcmd_mod.Decorator()

# For a non-unix system (like Windows), we will define our own error codes.
try:
    EX_OK: int = os.EX_OK
    EX_USAGE: int = os.EX_USAGE
    EX_SOFTWARE: int = os.EX_SOFTWARE
    EX_OSFILE: int = os.EX_OSFILE
except AttributeError:  # On a non-unix system
    EX_OK: int = 0
    EX_USAGE: int = 64
    EX_SOFTWARE: int = 70
    EX_OSFILE: int = 72


# We have different mitigations on different platforms due to cmd differences
def _get_unmount_timeout_suggestions(path: str) -> str:
    UNMOUNT_TIMEOUT_SUGGESTIONS = """\
    * `eden stop` -> retry `eden rm`
    * `eden doctor` -> retry `eden rm`
    * Reboot your machine -> retry `eden rm`
    """
    # windows does not have a umount equivalent
    if sys.platform != "win32":
        # umount on macOS does not have the --lazy option
        flags = "-f" if sys.platform == "darwin" else "-lf"
        return (
            f"""\
    * `sudo umount {flags} {path}` -> retry `eden rm`\n"""
            + UNMOUNT_TIMEOUT_SUGGESTIONS
        )
    else:
        return UNMOUNT_TIMEOUT_SUGGESTIONS


def do_version(args: argparse.Namespace, format_json: bool = False) -> int:
    instance = get_eden_instance(args)
    installed_version = version_mod.get_current_version()
    running_version = "-"

    try:
        running_version = instance.get_running_version()
    except EdenNotRunningError:
        if not format_json:
            running_version = "Unknown (EdenFS does not appear to be running)"

    if format_json:
        if installed_version == "-":
            installed_version = None
        if running_version == "-":
            running_version = None
        info = {"installed": installed_version, "running": running_version}
        json.dump(info, sys.stdout, indent=2)
    else:
        print(f"Installed: {installed_version}")
        print(f"Running:   {running_version}")
        if running_version.startswith("-") or running_version.endswith("-"):
            print("(Dev version of EdenFS seems to be running)")

    return 0


@subcmd("version", "Print EdenFS's version information.")
class VersionCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--json",
            action="store_true",
            help="Print the running and installed versions in json format",
        )

    def run(self, args: argparse.Namespace) -> int:
        return do_version(args, args.json)


@subcmd("info", "Get details about a checkout")
class InfoCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "client", default=None, nargs="?", help="Path to the checkout"
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.client)
        info = instance.get_checkout_info_from_checkout(checkout)
        json.dump(info, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0


@subcmd("du", "Show disk space usage for a checkout")
class DiskUsageCmd(Subcmd):

    isatty = sys.stdout and sys.stdout.isatty()
    # Escape sequence to move the cursor left to the start of the line
    # and then clear to the end of that line.
    MOVE_TO_SOL_CLEAR_TO_EOL = "\r\x1b[K" if isatty else ""
    aggregated_usage_counts = {
        "materialized": 0,
        "ignored": 0,
        "redirection": 0,
        "backing": 0,
        "shared": 0,
        "fsck": 0,
        "legacy": 0,
    }
    color_out = ui.get_output()
    hasLFS = False
    hasWorkingCopyBacked = False

    json_mode = False

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "mounts", default=[], nargs="*", help="Names of the mount points"
        )
        group = parser.add_mutually_exclusive_group()
        group.add_argument(
            "--clean", action="store_true", help="Performs automated cleanup"
        )
        group.add_argument(
            "--deep-clean",
            action="store_true",
            help="Performs automated cleanup (--clean) and removes fsck dirs. "
            "Unlike --clean this will destroy unrecoverable data. If you have any local changes you "
            "hope to recover, recover them before you run this command.",
        )
        group.add_argument(
            "--clean-orphaned",
            action="store_true",
            help="Performs automated cleanup of the orphaned redirections. "
            "This is a subset of --clean that is safe to run without affecting "
            "running tools relying on redirections.",
        )
        group.add_argument(
            "--json",
            action="store_true",
            default=False,
            help="Print the output in JSON format",
        )

    def run(self, args: argparse.Namespace) -> int:
        mounts = args.mounts
        clean = args.clean
        deep_clean = args.deep_clean
        if deep_clean:
            clean = True

        self.json_mode = args.json

        color_out = self.color_out

        instance = None

        if not mounts:
            instance = get_eden_instance(args)
            if not instance:
                raise subcmd_mod.CmdError("no EdenFS instance found\n")
            mounts = list(instance.get_mount_paths())
            if not mounts:
                raise subcmd_mod.CmdError("no EdenFS mount found\n")

        if clean:
            self.write_ui(
                """
WARNING: --clean option doesn't remove ignored files.
Materialized files will be de-materialized once committed.
Use `hg status -i` to see Ignored files, `hg clean --all`
to remove them but be careful: it will remove untracked files as well!
It is best to use `eden redirect` or the `mkscratch` utility to relocate
files outside the repo rather than to ignore and clean them up.\n""",
                fg=color_out.YELLOW,
            )

        backing_repos = set()
        all_redirections = set()

        self.write_ui(util.underlined("Mounts"))

        # print all mounts together.
        for mount in mounts:
            self.writeln_ui(mount)

        # loop again because fsck details for each mount
        # are printed if exist
        for mount in mounts:
            instance, checkout, _rel_path = require_checkout(args, mount)
            config = checkout.get_config()
            backing_repos.add(config.backing_repo)

            self.usage_for_mount(mount, args, clean, deep_clean)

        self.write_ui(util.underlined("Redirections"))

        for mount in mounts:
            instance, checkout, _rel_path = require_checkout(args, mount)
            self.usage_for_redirections(checkout, all_redirections, clean, instance)

        if not all_redirections:
            self.writeln_ui("No redirection")

        if not clean and all_redirections:
            for redir in all_redirections:
                self.writeln_ui(redir)
            self.writeln_ui(
                """
To reclaim space from buck-out directories, run `buck clean` from the
parent of the buck-out directory.
"""
            )

        if backing_repos:
            self.write_ui(util.underlined("Backing repos"))
            for backing in backing_repos:
                self.writeln_ui(backing)

            self.write_ui(
                """
CAUTION: You can lose work and break things by manually deleting data
from the backing repo directory!
""",
                fg=color_out.YELLOW,
            )

        lfs_repos = set()
        backed_working_copy_repos = set()

        for backing in backing_repos:
            self.backing_usage(backing, lfs_repos, backed_working_copy_repos)

        if sys.platform != "win32":
            hgcache_path = subprocess.check_output(
                ["hg", "config", "remotefilelog.cachepath"],
                encoding="UTF-8",
                env=get_environment_suitable_for_subprocess(),
            ).rstrip()

            command = f"`rm -rf {hgcache_path}/*`"

            self.writeln_ui(
                f"""
To reclaim space from the hgcache directory, run:

{command}

NOTE: The hgcache should manage its size itself. You should only run the command
above if you are completely out of space and quickly need to reclaim some space
temporarily. This will affect other users if you run this command on a shared machine.
"""
            )

        if backed_working_copy_repos:
            self.writeln_ui(
                """
Working copy detected in backing repo.  This is not generally useful
and just takes up space.  You can make this a bare repo to reclaim
space by running:
"""
            )
            for backed_working_copy in backed_working_copy_repos:
                self.writeln_ui(f"hg -R {backed_working_copy} checkout null")

        if instance:
            self.shared_usage(instance, clean)

        self.make_summary(clean, deep_clean)

        if self.json_mode:
            print(json.dumps(self.aggregated_usage_counts))

        return 0

    def make_summary(self, clean, deep_clean) -> None:
        self.write_ui(util.underlined("Summary"))
        type_labels = {
            "materialized": "Materialized files",
            "redirection": "Redirections",
            "ignored": "Ignored files",
            "backing": "Backing repos",
            "shared": "Shared space",
            "legacy": "Legacy bind mounts",
            "fsck": "Filesystem Check recovered files",
        }
        clean_labels = {
            "materialized": "Not cleaned. Please see WARNING above",
            "redirection": "Cleaned",
            "ignored": "Not cleaned. Please see WARNING above",
            "backing": "Not cleaned. Please see CAUTION above",
            "shared": "Cleaned",
            "legacy": "Not cleaned. Directories listed above."
            + " Check and remove manually",
            "fsck": "Not cleaned. Directories listed above."
            + " Check and remove manually",
        }

        if deep_clean:
            clean_labels["fsck"] = "Cleaned"

        # align colons. type_label for fsck is long, so
        # space for left align is longer when fsck usage
        # is printed.
        if self.aggregated_usage_counts["fsck"]:
            f = "{0:>33}: {1:<10}"
        else:
            f = "{0:>20}: {1:<10}"

        for key, value in self.aggregated_usage_counts.items():
            type_label = type_labels[key]
            clean_label = clean_labels[key] if clean else ""
            if value:
                self.write_ui(f.format(type_label, format_size(value)))
                if clean_label == "Cleaned":
                    self.writeln_ui(clean_label, fg=self.color_out.GREEN)
                else:
                    self.writeln_ui(clean_label, fg=self.color_out.YELLOW)
        self.writeln_ui("")
        if not clean:
            self.writeln_ui("To perform automated cleanup, run `eden du --clean`\n")

    def du(self, path) -> int:
        dev = os.stat(path).st_dev

        def get_size(path) -> int:
            total = 0
            failed_to_check_files = []

            for dirent in os.scandir(path):
                try:
                    if dirent.is_dir(follow_symlinks=False):
                        # Don't recurse onto different filesystems
                        if (
                            sys.platform == "win32"
                            or dirent.stat(follow_symlinks=False).st_dev == dev
                        ):
                            total += get_size(dirent.path)
                    else:
                        stat = dirent.stat(follow_symlinks=False)
                        if sys.platform == "win32":
                            total += stat.st_size
                        else:
                            # Use st_blocks as this represent the actual amount of
                            # disk space allocated by the file, not its apparent
                            # size.
                            total += stat.st_blocks * 512
                except FileNotFoundError:
                    failed_to_check_files.append(dirent.path)
                except PermissionError:
                    failed_to_check_files.append(dirent.path)
            if failed_to_check_files:
                pretry_failed_to_check_files = ", ".join(failed_to_check_files)
                self.write_ui(
                    "Warning: failed to check paths"
                    f" {pretry_failed_to_check_files} due to file not found or"
                    " permission errors. Note that will also not be able to"
                    " clean these paths.\n",
                    fg=self.color_out.YELLOW,
                )
            return total

        return get_size(path)

    def write_ui(self, message, fg=None) -> None:
        if not self.json_mode:
            self.color_out.write(message, fg=fg)

    def writeln_ui(self, message, fg=None) -> None:
        self.write_ui(f"{message}\n")

    def usage_for_dir(
        self, path, usage_type: str, print_label: Optional[str] = None
    ) -> None:
        usage = self.du(path)
        if usage_type in self.aggregated_usage_counts.keys():
            self.aggregated_usage_counts[usage_type] += usage
        if print_label:
            self.writeln_ui(
                f"{self.MOVE_TO_SOL_CLEAR_TO_EOL}{print_label}: {format_size(usage)}"
            )

    def backing_usage(
        self,
        backing_repo: Path,
        lfs_repos: Set[Path],
        backed_working_copy_repos: Set[Path],
    ) -> None:

        self.usage_for_dir(backing_repo, "backing")

        hg_dir = backing_repo / hg_util.sniff_dot_dir(backing_repo)
        if hg_dir.exists():
            lfs_dir = hg_dir / "store" / "lfs"
            if os.path.exists(lfs_dir):
                lfs_repos.add(backing_repo)

            if len(os.listdir(backing_repo)) > 1:
                backed_working_copy_repos.add(backing_repo)

    def shared_usage(self, instance: EdenInstance, clean: bool) -> None:
        logs_dir = instance.state_dir / "logs"
        storage_dir = instance.state_dir / "storage"
        self.write_ui(util.underlined("Shared space"))
        self.usage_for_dir(logs_dir, "shared")
        self.usage_for_dir(storage_dir, "shared")
        if clean:
            self.writeln_ui("Cleaning shared space used by the storage engine...")
            subprocess.check_call(["eden", "gc"])
        else:
            self.writeln_ui(
                "\nRun `eden gc` to reduce the space used by the storage engine."
            )

    def usage_for_redirections(
        self,
        checkout: EdenCheckout,
        redirection_repos: Set[str],
        clean: bool,
        instance: EdenInstance,
    ) -> None:
        redirections = redirect_mod.get_effective_redirections(
            checkout, mtab.new(), instance
        )
        seen_paths = set()
        if len(redirections) > 0:
            for redir in redirections.values():
                target = redir.expand_target_abspath(checkout)
                seen_paths.add(target)
                self.usage_for_dir(target, "redirection")

                dirname, basename = os.path.split(redir.repo_path)
                parent = os.path.join(checkout.path, dirname)
                if basename == "buck-out":
                    redir_full_path = os.path.join(checkout.path, redir.repo_path)
                    redirection_repos.add(redir_full_path)
                    if clean:
                        self.writeln_ui(
                            f"\nReclaiming space from redirection: {redir_full_path}"
                        )
                        result = run_buck_command([get_buck_command(), "clean"], parent)
                        result.check_returncode()
                        self.writeln_ui("Space reclaimed.\n")

        # Deal with any legacy bind mounts that may have been made
        # obsolete by being migrated to redirections
        legacy_bind_mounts_dir = os.path.join(checkout.state_dir, "bind-mounts")
        legacy_dirs: List[str] = []
        if os.path.isdir(legacy_bind_mounts_dir):
            for legacy in os.listdir(legacy_bind_mounts_dir):
                legacy_dir = os.path.join(legacy_bind_mounts_dir, legacy)
                if legacy_dir not in seen_paths:
                    if not legacy_dirs:
                        legacy_dirs.append(legacy_dir)
                    self.usage_for_dir(
                        legacy_dir, "legacy", print_label=f"    {legacy_dir}"
                    )

        if legacy_dirs:
            self.writeln_ui(
                """
Legacy bind mount dirs listed above are unused and can be removed!
"""
            )

    def usage_for_mount(
        self, mount: str, args: argparse.Namespace, clean: bool, deep_clean: bool
    ) -> None:

        instance, checkout, _rel_path = require_checkout(args, mount)

        client_dir = checkout.state_dir
        overlay_dir = os.path.join(client_dir, "local")

        self.usage_for_dir(overlay_dir, "materialized")
        with instance.get_thrift_client_legacy() as client:
            scm_status = client.getScmStatus(
                bytes(checkout.path), True, checkout.get_snapshot()[0].encode()
            )

            for rel_path, _file_status in scm_status.entries.items():
                try:
                    st = os.lstat(os.path.join(bytes(checkout.path), rel_path))
                    self.aggregated_usage_counts["ignored"] += st.st_size
                except FileNotFoundError:
                    # Status can show files that were present in the overlay
                    # before a redirection was mounted over the top of it,
                    # which makes them inaccessible here.  Alternatively,
                    # someone may have raced with us and removed the file
                    # between the status call and our attempt to stat it.
                    # Just absorb the error here and ignore it.
                    pass

        fsck_dir = os.path.join(client_dir, "fsck")
        if os.path.exists(fsck_dir):
            self.usage_for_dir(fsck_dir, "fsck")
            if deep_clean:
                self.writeln_ui(f"\nReclaiming space from directory: {fsck_dir}")
                try:
                    shutil.rmtree(fsck_dir)
                    self.writeln_ui("Space reclaimed. Directory removed.\n")
                except Exception as ex:
                    self.writeln_ui(f"Failed to remove {fsck_dir} : {ex} \n")

            elif clean:
                self.writeln_ui(
                    f"""
A filesytem check recovered data and stored it at:
{fsck_dir}
If you have recovered all that you need from it, you can remove that
directory to reclaim the disk space.

To automatically remove this directory, run `eden du --deep-clean`.
"""
                )


@subcmd("pid", "Print the daemon's process ID if running")
class PidCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        health_info = instance.check_health()
        if health_info.is_healthy():
            print(health_info.pid)
            return 0

        print("edenfs not healthy: {}".format(health_info.detail), file=sys.stderr)
        return 1


@subcmd("status", "Check the health of the EdenFS service", aliases=["health"])
class StatusCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--timeout",
            type=float,
            default=3.0,
            help="Wait up to TIMEOUT seconds for the daemon to respond "
            "(default=%(default)s).",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        health_info = instance.check_health(timeout=args.timeout)
        if health_info.is_healthy():
            print("edenfs running normally (pid {})".format(health_info.pid))
            return 0

        print("edenfs not healthy: {}".format(health_info.detail))
        return 1


@subcmd("list", "List available checkouts")
class ListCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--json",
            action="store_true",
            default=False,
            help="Print the output in JSON format",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)

        mounts = instance.get_mounts()
        out = ui.get_output()
        if args.json:
            self.print_mounts_json(out, mounts)
        else:
            self.print_mounts(out, mounts)
        return 0

    @staticmethod
    def print_mounts_json(
        out: ui.Output, mount_points: Dict[Path, ListMountInfo]
    ) -> None:
        data = {
            mount.path.as_posix(): mount.to_json_dict()
            for mount in mount_points.values()
        }
        json_str = json.dumps(data, sort_keys=True, indent=2)
        out.writeln(json_str)

    @staticmethod
    def print_mounts(out: ui.Output, mount_points: Dict[Path, ListMountInfo]) -> None:
        for path, mount_info in sorted(mount_points.items()):
            if not mount_info.configured:
                suffix = " (unconfigured)"
            else:
                suffix = ""

            if mount_info.state is None:
                state_str = " (not mounted)"
            elif mount_info.state == MountState.RUNNING:
                # For normally running mount points don't print any state information.
                # We only show the state if the mount is in an unusual state.
                state_str = ""
            else:
                state_name = MountState._VALUES_TO_NAMES[mount_info.state]
                state_str = f" ({state_name})"

            out.writeln(f"{path.as_posix()}{state_str}{suffix}")


class RepoError(Exception):
    pass


@subcmd("clone", "Create a clone of a specific repo and check it out")
class CloneCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("repo", help="The path to an existing repository to clone")
        parser.add_argument(
            "path", help="The path where the checkout should be mounted"
        )
        parser.add_argument(
            "--rev", "-r", type=str, help="The initial revision to check out"
        )
        parser.add_argument(
            "--allow-empty-repo",
            "-e",
            action="store_true",
            help="Allow repo with null revision (no revisions)",
        )
        parser.add_argument(
            "--allow-nested-checkout",
            "-n",
            action="store_true",
            help="Allow creation of nested checkout (not recommended)",
        )

        # Optional arguments to control how to start the daemon if clone needs
        # to start edenfs.  We do not show these in --help by default These
        # behave identically to the daemon arguments with the same name.
        parser.add_argument("--daemon-binary", help=argparse.SUPPRESS)
        parser.add_argument(
            "--daemon-args",
            dest="edenfs_args",
            nargs=argparse.REMAINDER,
            help=argparse.SUPPRESS,
        )

        parser.add_argument(
            "--nfs",
            dest="nfs",
            action="store_true",
            default=is_apple_silicon(),
            help=argparse.SUPPRESS,
        )

        # This option only works on Windows for now
        parser.add_argument(
            "--overlay-type",
            choices=("sqlite", "tree"),
            default=None,
            help="(Windows only) specify overlay type",
        )

        parser.add_argument(
            "--backing-store",
            help="Clone path as backing store instead of a source control repository. Currently only support 'recas' (Linux and macOS only) and 'http' (Linux only)",
        )

        parser.add_argument(
            "--re-use-case",
            help="The Remote Execuation use-case to use when --backing-store=recas",
        )

        case_group = parser.add_mutually_exclusive_group()
        case_group.add_argument(
            "--case-sensitive",
            action="store_true",
            default=sys.platform == "linux",
            help=argparse.SUPPRESS,
        )
        case_group.add_argument(
            "--case-insensitive",
            action="store_false",
            dest="case_sensitive",
            default=sys.platform != "linux",
            help=argparse.SUPPRESS,
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)

        # Make sure the destination directory does not exist or is an empty
        # directory.  (We'll check this again later when actually creating the
        # mount, but check this here just to fail early if things look wrong.)
        try:
            for _ in os.listdir(args.path):
                print_stderr(f"error: destination path {args.path} " "is not empty")
                return 1
        except OSError as ex:
            if ex.errno == errno.ENOTDIR:
                print_stderr(
                    f"error: destination path {args.path} " "is not a directory"
                )
                return 1
            elif ex.errno != errno.ENOENT:
                print_stderr(
                    f"error: unable to access destination path " f"{args.path}: {ex}"
                )
                return 1

        def is_nfs_default():
            default_protocol = "PrjFS" if sys.platform == "win32" else "FUSE"
            return (
                instance.get_config_value(
                    "clone.default-mount-protocol", default_protocol
                ).upper()
                == "NFS"
            )

        args.path = os.path.realpath(args.path)
        args.nfs = args.nfs or is_nfs_default()

        # Check if requested path is inside an existing checkout
        instance = EdenInstance(args.config_dir, args.etc_eden_dir, args.home_dir)
        existing_checkout, rel_path = config_mod.detect_nested_checkout(
            args.path,
            instance,
        )
        if existing_checkout is not None and rel_path is not None:
            if args.allow_nested_checkout:
                print(
                    """\
Warning: Creating a nested checkout. This is not recommended because it
may cause `eden doctor` and `eden rm` to encounter spurious behavior."""
                )
            else:
                print_stderr(
                    f"""\
error: destination path {args.path} is within an existing checkout {existing_checkout.path}.

Nested checkouts are usually not intended/recommended and may cause
`eden doctor` and `eden rm` to encounter spurious behavior. If you DO
want nested checkouts, re-run `eden clone` with --allow-nested-checkout or -n."""
                )
                return 1

        if args.case_sensitive and sys.platform != "linux":
            print(
                """\
Warning: Creating a case-sensitive checkout on a platform where the default is
case-insensitive. This is not recommended and is intended only for testing."""
            )
        if not args.case_sensitive and sys.platform == "linux":
            print(
                """\
Warning: Creating a case-insensitive checkout on a platform where the default
is case-sensitive. This is not recommended and is intended only for testing."""
            )
        # Find the repository information
        try:
            repo, repo_config = self._get_repo_info(
                instance,
                args.repo,
                args.rev,
                args.nfs,
                args.case_sensitive,
                args.overlay_type,
                args.backing_store,
                args.re_use_case,
            )
        except RepoError as ex:
            print_stderr("error: {}", ex)
            return 1

        # If it's source control respository
        if not args.backing_store:
            # Find the commit to check out
            if args.rev is not None:
                try:
                    commit = repo.get_commit_hash(args.rev)
                except Exception as ex:
                    print_stderr(
                        f"error: unable to find hash for commit " f"{args.rev!r}: {ex}"
                    )
                    return 1
            else:
                try:
                    commit = repo.get_commit_hash(repo_config.default_revision)
                except Exception as ex:
                    print_stderr(
                        f"error: unable to find hash for commit "
                        f"{repo_config.default_revision!r}: {ex}"
                    )
                    return 1

                NULL_REVISION = "0" * 40
                if commit == NULL_REVISION and not args.allow_empty_repo:
                    print_stderr(
                        f"""\
    error: the initial revision that would be checked out is the empty commit

    The repository at {repo.source} may still be cloning.
    Please make sure cloning completes before running `eden clone`
    If you do want to check out the empty commit,
    re-run `eden clone` with --allow-empty-repo"""
                    )
                    return 1

        elif args.backing_store == "recas":
            if sys.platform == "win32":
                print_stderr(
                    "error: recas backing store was passed but this feature is not available on Windows"
                )
                return 1
            if args.rev is not None:
                commit = args.rev
            else:
                NULL_REVISION = "0" * 40
                # A special digest for RE CAS representing an empty folder
                commit = f"{NULL_REVISION}:0"
        elif args.backing_store == "http":
            if sys.platform != "linux":
                print_stderr(
                    "error: http backing store was passed but this feature is only available on Linux"
                )
                return 1
            if args.rev is None:
                commit = "/"
            else:
                if args.rev.startswith("/") and args.rev.endswith("/"):
                    commit = args.rev
                else:
                    print_stderr(
                        "error: http backing store revision (root path) should begin and end with '/'"
                    )
                    return 1
        else:
            raise RepoError(f"Unsupported backing store {args.backing_store}.")

        # Attempt to start the daemon if it is not already running.
        health_info = instance.check_health()
        if health_info.is_starting():
            print("EdenFS daemon is still starting. Waiting for EdenFS to start ...")
            try:
                wait_for_instance_healthy(instance, 600)
            except EdenStartError as error:
                print(error)
                return 1
            print("EdenFS started.")
        elif not health_info.is_healthy():
            print("edenfs daemon is not currently running. Starting...")
            # Sometimes this returns a non-zero exit code if it does not finish
            # startup within the default timeout.
            exit_code = daemon.start_edenfs_service(
                instance, args.daemon_binary, args.edenfs_args
            )
            if exit_code != 0:
                return exit_code

        print(f"Cloning new repository at {args.path}...")

        try:
            instance.clone(repo_config, args.path, commit)
            print(f"Success.  Checked out commit {commit:.8}")
            # In the future it would probably be nice to fork a background
            # process here to prefetch files that we think the user is likely
            # to want to access soon.
            return 0
        except Exception as ex:
            print_stderr("error: {}", ex)
            return 1

    def _get_enable_tree_overlay(
        self, instance: EdenInstance, overlay_type: Optional[str]
    ) -> bool:
        if overlay_type is None:
            # Tree overlay does not support non-Windows platform yet
            return sys.platform == "win32"

        return overlay_type == "tree"

    def _get_repo_info(
        self,
        instance: EdenInstance,
        repo_arg: str,
        rev: Optional[str],
        nfs: bool,
        case_sensitive: bool,
        overlay_type: Optional[str],
        backing_store_type: Optional[str] = None,
        re_use_case: Optional[str] = None,
    ) -> Tuple[util.Repo, config_mod.CheckoutConfig]:
        # Check to see if repo_arg points to an existing EdenFS mount
        checkout_config = instance.get_checkout_config_for_path(repo_arg)
        if checkout_config is not None:
            repo = util.get_repo(str(checkout_config.backing_repo), backing_store_type)
            if repo is None:
                raise RepoError(
                    "EdenFS mount is configured to use repository "
                    f"{checkout_config.backing_repo} but unable to find a "
                    "repository at that location"
                )
            return repo, checkout_config

        # Confirm that repo_arg looks like an existing repository path.
        repo = util.get_repo(repo_arg, backing_store_type)
        if repo is None:
            raise RepoError(f"{repo_arg!r} does not look like a valid repository")

        mount_protocol = get_protocol(nfs)

        enable_tree_overlay = self._get_enable_tree_overlay(instance, overlay_type)

        # This is a valid repository path.
        # Prepare a CheckoutConfig object for it.
        repo_config = config_mod.CheckoutConfig(
            backing_repo=Path(repo.source),
            scm_type=repo.type,
            guid=str(uuid.uuid4()),
            mount_protocol=mount_protocol,
            case_sensitive=case_sensitive,
            require_utf8_path=True,
            default_revision=config_mod.DEFAULT_REVISION[repo.type],
            redirections={},
            active_prefetch_profiles=[],
            predictive_prefetch_profiles_active=False,
            predictive_prefetch_num_dirs=0,
            enable_tree_overlay=enable_tree_overlay,
            use_write_back_cache=False,
            re_use_case=re_use_case or "buck2-default",
        )

        return repo, repo_config


@subcmd("config", "Query EdenFS configuration")
class ConfigCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        instance.print_full_config(sys.stdout.buffer)
        return 0


@subcmd("doctor", "Debug and fix issues with EdenFS")
class DoctorCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--current-edenfs-only",
            action="store_true",
            default=False,
            help="Only report problems with the current EdenFS instance, and skip "
            "system-wide checks (e.g., low disk space, stale mount points, etc).",
        )
        parser.add_argument(
            "--dry-run",
            "-n",
            action="store_true",
            help="Do not try to fix any issues: only report them.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        doctor = doctor_mod.EdenDoctor(instance, args.dry_run, args.debug)
        if args.current_edenfs_only:
            doctor.run_system_wide_checks = False
        return doctor.cure_what_ails_you()


@subcmd("strace", "Monitor FUSE requests.")
class StraceCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout", default=None, nargs="?", help="Path to the checkout"
        )
        parser.add_argument(
            "--reads",
            action="store_true",
            default=False,
            help="Limit trace to read operations",
        )
        parser.add_argument(
            "--writes",
            action="store_true",
            default=False,
            help="Limit trace to write operations",
        )

    def run(self, args: argparse.Namespace) -> int:
        print_stderr("No longer supported, use `eden trace fs` instead.")
        return 1


@subcmd("top", "Monitor EdenFS accesses by process.")
class TopCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--ephemeral",
            "-e",
            action="store_true",
            help="Don't accumulate data; refresh the screen every update cycle.",
        )
        parser.add_argument(
            "--refresh-rate",
            "-r",
            default=1,
            help="Specify the rate (in seconds) at which eden top updates.",
            type=int,
        )

    def run(self, args: argparse.Namespace) -> int:
        if sys.platform == "win32":
            print_stderr("`edenfsctl top` isn't supported on Windows yet.")
            return 1
        top = top_mod.Top()
        return top.start(args)


@subcmd("minitop", "Simple monitoring of EdenFS accesses by process.")
class MinitopCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        print_stderr(
            "This is not implemented for python edenfsctl. Use `top` subcommand instead."
        )
        return 1


@subcmd("fsck", "Perform a filesystem check for EdenFS")
class FsckCmd(Subcmd):
    EXIT_OK = 0
    EXIT_SKIPPED = 1
    EXIT_WARNINGS = 2
    EXIT_ERRORS = 3

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--force",
            action="store_true",
            default=False,
            help="Force fsck to scan for errors even on checkouts that appear to "
            "currently be mounted.  It will not attempt to fix any problems, but will "
            "only scan and report possible issues.",
        )
        parser.add_argument(
            "-n",
            "--check-only",
            action="store_true",
            default=False,
            help="Only report errors, and do not attempt to fix any problems found.",
        )
        parser.add_argument(
            "-v",
            "--verbose",
            action="store_true",
            default=False,
            help="Print more verbose information about issues found.",
        )
        parser.add_argument(
            "path",
            metavar="CHECKOUT_PATH",
            nargs="*",
            help="The path to an EdenFS checkout to verify.",
        )

    def run(self, args: argparse.Namespace) -> int:
        if sys.platform == "win32":
            print_stderr("`edenfsctl fsck` is not supported on Windows.")
            print_stderr(
                "If you are looking to fix your EdenFS mount, try `edenfsctl doctor`."
            )
            return 1

        if not args.path:
            return_codes = self.check_all(args)
            if not return_codes:
                print_stderr("No EdenFS checkouts are configured.  Nothing to check.")
                return 0
        else:
            return_codes = self.check_explicit_paths(args)

        return max(return_codes)

    def check_explicit_paths(self, args: argparse.Namespace) -> List[int]:
        return_codes: List[int] = []
        for path in args.path:
            # Check to see if this looks like an EdenFS checkout state directory.
            # If this looks like an EdenFS checkout state directory,
            if (Path(path) / "local" / "info").exists() and (
                Path(path) / "config.toml"
            ).exists():
                result = self.check_one(args, Path(path), Path(path))
            else:
                instance, checkout, rel_path = require_checkout(args, path)
                result = self.check_one(args, checkout.path, checkout.state_dir)
            return_codes.append(result)

        return return_codes

    def check_all(self, args: argparse.Namespace) -> List[int]:
        # Check all configured checkouts that are not currently mounted.
        instance = get_eden_instance(args)
        return_codes: List[int] = []
        for checkout in instance.get_checkouts():
            result = self.check_one(args, checkout.path, checkout.state_dir)
            return_codes.append(result)

        return return_codes

    def check_one(
        self, args: argparse.Namespace, checkout_path: Path, state_dir: Path
    ) -> int:
        with fsck_mod.FilesystemChecker(state_dir) as checker:
            if not checker._overlay_locked:
                if args.force:
                    print(
                        f"warning: could not obtain lock on {checkout_path}, but "
                        f"scanning anyway due to --force "
                    )
                else:
                    print(f"Not checking {checkout_path}: mount is currently in use")
                    return self.EXIT_SKIPPED

            print(f"Checking {checkout_path}...")
            checker.scan_for_errors()
            if not checker.errors:
                print("  No issues found")
                return self.EXIT_OK

            num_warnings = 0
            num_errors = 0
            for error in checker.errors:
                self._report_error(args, error)
                if error.level == fsck_mod.ErrorLevel.WARNING:
                    num_warnings += 1
                else:
                    num_errors += 1

            if num_warnings > 0:
                print(f"  {num_warnings} warnings")
            print(f"  {num_errors} errors")

            if args.check_only:
                print("Not fixing errors: --check-only was specified")
            elif not checker._overlay_locked:
                print("Not fixing errors: checkout is currently in use")
            else:
                checker.fix_errors()

            if num_errors == 0:
                return self.EXIT_WARNINGS
            return self.EXIT_ERRORS

    def _report_error(self, args: argparse.Namespace, error: "fsck_mod.Error") -> None:
        print(f"{fsck_mod.ErrorLevel.get_label(error.level)}: {error}")
        if args.verbose:
            details = error.detailed_description()
            if details:
                print("  " + "\n  ".join(details.splitlines()))


@subcmd("gc", "Minimize disk and memory usage by freeing caches")
class GcCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)

        with instance.get_thrift_client_legacy() as client:
            # TODO: unload
            print("Clearing and compacting local caches...", end="", flush=True)
            client.clearAndCompactLocalStore()
            print()
            # TODO: clear kernel caches

        return 0


@subcmd("chown", "Chown an entire EdenFS repository")
class ChownCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", metavar="path", help="The EdenFS checkout to chown")
        parser.add_argument(
            "uid", metavar="uid", help="The uid or unix username to chown to"
        )
        parser.add_argument(
            "gid", metavar="gid", help="The gid or unix group name to chown to"
        )
        parser.add_argument(
            "--skip-redirection",
            action="store_true",
            default=False,
            help="Are redirections also chowned",
        )

    def resolve_uid(self, uid_str: str) -> int:
        try:
            return int(uid_str)
        except ValueError:
            import pwd

            return pwd.getpwnam(uid_str).pw_uid

    def resolve_gid(self, gid_str: str) -> int:
        try:
            return int(gid_str)
        except ValueError:
            import grp

            return grp.getgrnam(gid_str).gr_gid

    def run(self, args: argparse.Namespace) -> int:
        uid = self.resolve_uid(args.uid)
        gid = self.resolve_gid(args.gid)

        instance, checkout, _rel_path = require_checkout(args, args.path)
        with instance.get_thrift_client_legacy() as client:
            print("Chowning EdenFS repository...", end="", flush=True)
            client.chown(args.path, uid, gid)
            print("done")

        if not args.skip_redirection:
            for redir in redirect_mod.get_effective_redirections(
                checkout, mtab.new(), instance
            ).values():
                target = redir.expand_target_abspath(checkout)
                print(f"Chowning redirection: {redir.repo_path}...", end="", flush=True)
                subprocess.run(["sudo", "chown", "-R", f"{uid}:{gid}", str(target)])
                subprocess.run(
                    [
                        "sudo",
                        "chown",
                        f"{uid}:{gid}",
                        str(checkout.path / redir.repo_path),
                    ]
                )
                print("done")

        return 0


@subcmd(
    "mount",
    "Remount an existing checkout (for instance, after it was manually unmounted)",
)
class MountCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "paths", nargs="+", metavar="path", help="The checkout mount path"
        )
        parser.add_argument(
            "--read-only", action="store_true", dest="read_only", help="Read only mount"
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        for path in args.paths:
            try:
                exitcode = instance.mount(path, args.read_only)
                if exitcode:
                    return exitcode
            except (EdenService.EdenError, EdenNotRunningError) as ex:
                print_stderr("error: {}", ex)
                return 1
        return 0


# Types of removal
#
# * ACTIVE_MOUNT: removing a mounted repository, we need to talk with EdenFS
# daemon to get it unmounted.
# * INACTIVE_MOUNT: removing an unmounted repository, we can simply delete
# its configuration.
# * CLEANUP_ONLY: removing an unknown directory, it might be an old EdenFS
# mount that failed to clean up. We try to clean it up again in this case.
class RemoveType(enum.Enum):
    ACTIVE_MOUNT = 0
    INACTIVE_MOUNT = 1
    CLEANUP_ONLY = 2


@subcmd("remove", "Remove an EdenFS checkout", aliases=["rm"])
class RemoveCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-y",
            "--yes",
            "--no-prompt",
            dest="prompt",
            default=True,
            action="store_false",
            help="Do not prompt for confirmation before removing the checkouts.",
        )
        parser.add_argument(
            "--preserve-mount-point",
            default=False,
            action="store_true",
            help=argparse.SUPPRESS,
        )
        parser.add_argument(
            "paths", nargs="+", metavar="path", help="The EdenFS checkout(s) to remove"
        )

    def is_prjfs_path(self, path: str) -> bool:
        if platform.system() != "Windows":
            return False
        try:
            return (Path(path) / ".EDEN_TEST_NONEXISTENT_PATH").exists()
        except OSError as e:
            # HACK: similar to how we test if EdenFS is running, we will get
            # this 369 error for partial removal because EdenFS is no longer
            # serving this mount point. As a result, Windows will return this
            # error for stat.
            # Errno 369 is not documented but it is "The provider that supports
            # file system virtualization is temporarily unavailable".

            # pyre-ignore[16]: winerror is Windows only.
            if e.winerror == 369:
                return True
            return False
        except Exception:
            return False

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        configured_mounts = list(instance.get_mount_paths())

        # First translate the list of paths into canonical checkout paths
        # We also track a bool indicating if this checkout is currently mounted
        mounts: List[Tuple[str, RemoveType]] = []
        for path in args.paths:
            if not os.path.exists(path):
                print(
                    f"error: {path} is neither an EdenFS mount nor an existing directory",
                    file=sys.stderr,
                )
                return 1
            try:
                mount_path = util.get_eden_mount_name(path)
                remove_type = RemoveType.ACTIVE_MOUNT
            except util.NotAnEdenMountError as ex:
                # This is not an active mount point.
                # Check for it by name in the config file anyway, in case it is
                # listed in the config file but not currently mounted.
                mount_path = os.path.realpath(path)
                if mount_path in configured_mounts:
                    remove_type = RemoveType.INACTIVE_MOUNT
                elif self.is_prjfs_path(path):
                    remove_type = RemoveType.CLEANUP_ONLY
                else:
                    # This is not located in the config file either, but it
                    # may be leftover from a failed `eden rm` attempt. The
                    # user may want us to delete it anyway, so let's ask.
                    if args.prompt and sys.stdin.isatty():
                        prompt = f"""\
Warning: The following is not an EdenFS Mount: {path}. Any files in this \
directory will be lost forever. Do you stil want to delete {path}?"""
                        if not prompt_confirmation(prompt):
                            return 2
                        else:
                            try:
                                path = Path(path)
                                path.chmod(0o755)
                                for child in path.iterdir():
                                    if child.is_dir():
                                        shutil.rmtree(child)
                                    else:
                                        child.unlink()
                                if not args.preserve_mount_point:
                                    path.rmdir()
                                return 0
                            except Exception as ex:
                                print(f"error: cannot remove contents of {path}: {ex}")
                                return 1
                    else:
                        # We can't ask the user what their true intentions are,
                        # so let's fail by default.
                        print(f"error: {ex}")
                        return 1

            except Exception as ex:
                print(f"error: cannot determine mount point for {path}: {ex}")
                return 1

            if os.path.realpath(mount_path) != os.path.realpath(path):
                print(
                    f"error: {path} is not the root of checkout "
                    f"{mount_path}, not deleting"
                )
                return 1
            mounts.append((mount_path, remove_type))

        # Warn the user since this operation permanently destroys data
        if args.prompt and sys.stdin.isatty():
            mounts_list = "\n  ".join(path for path, _ in mounts)
            print(
                f"""\
Warning: this operation will permanently delete the following checkouts:
  {mounts_list}

Any uncommitted changes and shelves in this checkout will be lost forever."""
            )
            if not prompt_confirmation("Proceed?"):
                print("Not confirmed")
                return 2

        # Unmount and destroy everything
        exit_code = 0
        for mount, remove_type in mounts:
            print(f"Removing {mount}...")
            if remove_type == RemoveType.ACTIVE_MOUNT:
                try:
                    # We don't bother complaining about removing redirections on Windows
                    # since redirections are symlinks on Windows anyway, so the removal
                    # of the repo will remove them. This would usually happen because
                    # the daemon is not running, so this is oftenjust extra spam for users.
                    print(f"Stopping aux processes for {mount}...")
                    stop_aux_processes_for_path(
                        mount,
                        complain_about_failing_to_unmount_redirs=(
                            sys.platform != "win32"
                        ),
                    )
                except Exception as ex:
                    print_stderr(f"error stopping auxiliary processes {mount}: {ex}")
                    exit_code = 1
                    # We intentionally fall through here and remove the mount point
                    # so that the eden daemon will attempt to unmount it.
                    # unmounting could still timeout, though we unmount with -f,
                    # so should this theoretically should not happen.
                try:
                    print(
                        f"Unmounting `{mount}`. Please be patient: this can take up to 1 minute!"
                    )
                    instance.unmount(mount)
                except EdenNotRunningError:
                    # Its fine if we could not tell the daemon to unmount
                    # because the daemon is not running. There is just no
                    # daemon we need to tell. Let's just perform the rest of
                    # the clean up and this will remove the mount as expected.
                    pass
                except Exception as ex:
                    # We used to intentionally fall through here and remove the
                    # mount point from the config file. The most likely cause
                    # of failure is if edenfs times out performing the unmount.
                    # In this case, we don't want to start modifying state and
                    # configs underneath a running EdenFS mount, so let's return
                    # early and give possible mitigations for unmount timeouts.
                    print_stderr(f"\nerror unmounting {mount}: {ex}\n\n")
                    print_stderr(
                        f"For unmount timeouts, you can try:\n{_get_unmount_timeout_suggestions(mount)}"
                    )
                    return 1

            try:
                if remove_type != RemoveType.CLEANUP_ONLY:
                    print(f"Deleting mount {mount}")
                    instance.destroy_mount(mount, args.preserve_mount_point)
            except Exception as ex:
                print_stderr(f"error deleting configuration for {mount}: {ex}")
                exit_code = 1
                if args.debug:
                    traceback.print_exc()
            else:
                try:
                    print(f"Cleaning up mount {mount}")
                    instance.cleanup_mount(Path(mount), args.preserve_mount_point)
                except Exception as ex:
                    print_stderr(f"error cleaning up mount {mount}: {ex}")
                    exit_code = 1
                    if args.debug:
                        traceback.print_exc()
                # Continue around the loop removing any other mount points

        if exit_code == 0:
            print("Success")
        return exit_code


#
# Most users should not need the "unmount" command in most circumstances.
# Maybe we should deprecate or remove it in the future.
#
# - "eden unmount --destroy" used to be the way to remove a checkout, but this has been
#   replaced by "eden rm".
# - I can't think of many situations where users would need to temporarily unmount a
#   checkout.  However, "/bin/umount" can be used to accomplish this.  The only
#   potential advantage of "eden umount" over "/bin/umount" is that "eden unmount" does
#   not require root privileges.
#
@subcmd("unmount", "Temporarily unmount a specific checkout")
class UnmountCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--destroy", action="store_true", help=argparse.SUPPRESS)
        parser.add_argument(
            "paths",
            nargs="+",
            metavar="path",
            help="Path where checkout should be unmounted from",
        )

    def run(self, args: argparse.Namespace) -> int:
        if args.destroy:
            print_stderr(
                'note: "eden unmount --destroy" is deprecated; '
                'prefer using "eden rm" instead'
            )

        instance = get_eden_instance(args)
        for path in args.paths:
            path = normalize_path_arg(path)
            try:
                instance.unmount(path)
                if args.destroy:
                    instance.destroy_mount(path)
            except (EdenService.EdenError, EdenNotRunningError) as ex:
                print_stderr(f"error: {ex}")
                return 1
        return 0


@subcmd("start", "Start the EdenFS service", aliases=["daemon"])
class StartCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--daemon-binary", help="Path to the binary for the edenfs daemon."
        )
        parser.add_argument(
            "--if-necessary",
            action="store_true",
            help="Only start edenfs daemon if there are EdenFS checkouts configured.",
        )
        parser.add_argument(
            "--if-not-running",
            action="store_true",
            help="Exit successfully if edenfs daemon is already running.",
        )
        parser.add_argument(
            "--foreground",
            "-F",
            action="store_true",
            help="Run edenfs in the foreground, rather than daemonizing",
        )

        if sys.platform != "win32":
            parser.add_argument(
                "--takeover",
                "-t",
                action="store_true",
                help="If an existing edenfs daemon is running, gracefully take "
                "over its mount points.",
            )
            parser.add_argument(
                "--gdb", "-g", action="store_true", help="Run under gdb"
            )
            parser.add_argument(
                "--gdb-arg",
                action="append",
                default=[],
                help="Extra arguments to pass to gdb",
            )
            parser.add_argument(
                "--strace",
                "-s",
                metavar="FILE",
                help="Run edenfs under strace, and write strace output to FILE",
            )

        parser.add_argument(
            "edenfs_args",
            nargs=argparse.REMAINDER,
            help='Any extra arguments after an "--" argument will be passed '
            "to the edenfs daemon.",
        )

    def run(self, args: argparse.Namespace) -> int:
        # If the user put an "--" argument before the edenfs args, argparse passes
        # that through to us.  Strip it out.
        try:
            args.edenfs_args.remove("--")
        except ValueError:
            pass

        if sys.platform != "win32":
            is_takeover = bool(args.takeover)
            if args.takeover and args.if_not_running:
                raise config_mod.UsageError(
                    "the --takeover and --if-not-running flags cannot be combined"
                )
            if args.gdb or args.strace:
                if args.gdb and args.strace is not None:
                    msg = "error: cannot run eden under gdb and strace together"
                    raise subcmd_mod.CmdError(msg)
                # --gdb or --strace imply foreground mode
                args.foreground = True
        else:
            is_takeover: bool = False

        instance = get_eden_instance(args)
        if args.if_necessary and not instance.get_mount_paths():
            print("No EdenFS mount points configured.")
            return 0

        daemon_binary = daemon_util.find_daemon_binary(args.daemon_binary)

        # Check to see if edenfs is already running
        health_info = instance.check_health()
        if not is_takeover:
            msg = None
            if health_info.is_healthy():
                msg = f"EdenFS is already running (pid {health_info.pid})"
            elif health_info.is_starting():
                msg = f"EdenFS is already starting (pid {health_info.pid})"

            if msg:
                if args.if_not_running:
                    print(msg)
                    return 0
                raise subcmd_mod.CmdError(msg)

        if args.foreground:
            return self.start_in_foreground(instance, daemon_binary, args)

        if is_takeover and health_info.is_healthy():
            daemon.gracefully_restart_edenfs_service(
                instance, daemon_binary, args.edenfs_args
            )

        if config_mod.should_migrate_mount_protocol_to_nfs(instance):
            config_mod._do_nfs_migration(instance, get_migration_success_message)
        return daemon.start_edenfs_service(instance, daemon_binary, args.edenfs_args)

    def start_in_foreground(
        self, instance: EdenInstance, daemon_binary: str, args: argparse.Namespace
    ) -> int:
        # Build the core command
        cmd, privhelper = daemon.get_edenfs_cmd(instance, daemon_binary)
        cmd.append("--foreground")
        if sys.platform != "win32" and args.takeover:
            cmd.append("--takeover")
        if args.edenfs_args:
            cmd.extend(args.edenfs_args)

        # Update the command with additional arguments
        if sys.platform != "win32":
            if args.gdb:
                cmd = ["gdb"] + args.gdb_arg + ["--args"] + cmd
            if args.strace is not None:
                cmd = ["strace", "-fttT", "-o", args.strace] + cmd

        # Wrap the command in sudo, if necessary
        eden_env = daemon.get_edenfs_environment()
        cmd, eden_env = daemon.prepare_edenfs_privileges(
            daemon_binary, cmd, eden_env, privhelper
        )

        if sys.platform == "win32":
            return subprocess.call(cmd, env=eden_env)
        else:
            os.execve(cmd[0], cmd, env=eden_env)
            # Throw an exception just to let mypy know that we should never reach here
            # and will never return normally.
            raise Exception("execve should never return")


def unmount_redirections_for_path(
    repo_path: str, complain_about_failing_to_unmount_redirs: bool
) -> None:
    parser = create_parser()
    args = parser.parse_args(["redirect", "unmount", "--mount", repo_path])
    try:
        args.func(args)
    except Exception as exc:
        if complain_about_failing_to_unmount_redirs:
            print(
                f"ignoring error while unmounting bind mounts: {exc}", file=sys.stderr
            )


def stop_aux_processes_for_path(
    repo_path: str, complain_about_failing_to_unmount_redirs: bool = True
) -> None:
    """Tear down processes that will hold onto file handles and prevent shutdown
    for a given mount point/repo"""
    buck.stop_buckd_for_repo(repo_path)
    unmount_redirections_for_path(repo_path, complain_about_failing_to_unmount_redirs)
    stop_internal_processes(repo_path)


def stop_aux_processes(client: EdenClient) -> None:
    """Tear down processes that will hold onto file handles and prevent shutdown
    for all mounts"""

    active_mount_points: Set[Optional[str]] = {
        os.fsdecode(mount.mountPoint) for mount in client.listMounts()
    }

    for repo in active_mount_points:
        if repo is not None:
            stop_aux_processes_for_path(repo)

    # TODO: intelligently stop nuclide-server associated with eden
    # print('Stopping nuclide-server...')
    # subprocess.run(['pkill', '-f', 'nuclide-main'])


RESTART_MODE_FULL = "full"
RESTART_MODE_GRACEFUL = "graceful"
RESTART_MODE_FORCE = "force"


@subcmd("restart", "Restart the EdenFS service")
# pyre-fixme[13]: Attribute `args` is never initialized.
class RestartCmd(Subcmd):
    args: argparse.Namespace

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        mode_group = parser.add_mutually_exclusive_group()
        mode_group.add_argument(
            "--full",
            action="store_const",
            const=RESTART_MODE_FULL,
            dest="restart_type",
            help="Completely shut down edenfs daemon before restarting it.  This "
            "will unmount and remount the EdenFS mounts, requiring processes "
            "using them to re-open any files and directories they are using.",
        )
        mode_group.add_argument(
            "--graceful",
            action="store_const",
            const=RESTART_MODE_GRACEFUL,
            dest="restart_type",
            help="Perform a graceful restart. The new edenfs daemon will "
            "take over the existing mount points with minimal "
            "disruption to clients. Open file handles will continue to work "
            "across the restart.",
        )

        parser.add_argument(
            "--force",
            dest="force_restart",
            default=False,
            action="store_true",
            help="Force a full restart, even if the existing edenfs daemon is "
            "still in the middle of starting or stopping.",
        )
        parser.add_argument(
            "--daemon-binary", help="Path to the binary for the edenfs daemon."
        )
        parser.add_argument(
            "--shutdown-timeout",
            type=float,
            default=None,
            help="How long to wait for the old edenfs process to exit when "
            "performing a full restart.",
        )

        parser.add_argument(
            "--only-if-running",
            action="store_true",
            default=False,
            help="Only perform a restart if there is already an EdenFS instance"
            "running.",
        )
        migration_group = parser.add_mutually_exclusive_group()
        migration_group.add_argument(
            "--migrate-to",
            type=str,
            default=None,
            choices=["fuse", "nfs"],
            help=migration_restart_help,
        )

    def run(self, args: argparse.Namespace) -> int:
        self.args = args
        if args.restart_type is None:
            # Default to a full restart for now
            args.restart_type = RESTART_MODE_FULL

        if args.migrate_to is not None:
            if args.restart_type == RESTART_MODE_GRACEFUL:
                print("Migration cannot be performed with a graceful restart.")
                return 1

            if sys.platform == "win32":
                print("Migration not supported on Windows.")
                return 1

            if sys.platform != "darwin":
                print("Migration only intended for macOS. ")
                if sys.stdin.isatty():
                    if not prompt_confirmation("Proceed?"):
                        print("Not confirmed.")
                        return 1

        instance = get_eden_instance(self.args)

        health = instance.check_health()
        edenfs_pid = health.pid
        if health.is_healthy():
            assert edenfs_pid is not None
            if self.args.restart_type == RESTART_MODE_GRACEFUL:
                return self._graceful_restart(instance)
            else:
                status = self._full_restart(instance, edenfs_pid, args.migrate_to)
                success = status == 0
                instance.log_sample("full_restart", success=success)
                return status
        elif edenfs_pid is None:
            # The daemon is not running
            if args.only_if_running:
                print("EdenFS not running; not starting EdenFS")
                return 0
            else:
                return self._start(instance)
        else:
            if health.status == fb303_status.STARTING:
                print(
                    f"The current edenfs daemon (pid {health.pid}) is still starting."
                )
                # Give the daemon a little extra time to hopefully finish starting
                # before we time out and kill it.
                stop_timeout = 30
            elif health.status == fb303_status.STOPPING:
                print(
                    f"The current edenfs daemon (pid {health.pid}) is in the middle "
                    "of stopping."
                )
                # Use a reduced stopping timeout.  If the user is using --force
                # then the daemon is probably stuck or something, and we'll likely need
                # to kill it anyway.
                stop_timeout = 5
            else:
                # The only other status value we generally expect to receive here is
                # fb303_status.STOPPED.  This is returned if we found an existing edenfs
                # process but it is not responding to thrift calls.
                print(
                    f"Found an existing edenfs daemon (pid {health.pid} that does not "
                    "seem to be responding to thrift calls."
                )
                # Don't attempt to ask the daemon to stop at all in this case;
                # just kill it.
                stop_timeout = 0

            if not self.args.force_restart:
                print(
                    "Use `eden restart --force` if you want to forcibly restart the current daemon"
                )
                return 4
            return self._force_restart(instance, edenfs_pid, stop_timeout)

    def _recover_after_failed_graceful_restart(
        self, instance: EdenInstance, telemetry_sample: TelemetrySample
    ) -> int:
        health = instance.check_health()
        edenfs_pid = health.pid
        if edenfs_pid is None:
            # The daemon is not running
            print(
                "The daemon is not running after failed graceful restart, "
                "starting it"
            )
            telemetry_sample.fail("EdenFS was not running after graceful restart")
            return self._start(instance)

        print(
            "Attempting to recover the current edenfs daemon "
            f"(pid {edenfs_pid}) after a failed graceful restart"
        )
        try:
            # We will give ourselves a fairly long period (10 m)
            # to recover from a failed graceful restart before
            # forcing a restart. We could hit this case if we are
            # waiting for in process thrift calls to finish, so we
            # want to wait for a reasonable time before we force
            # kill the process (if it is stuck somewhere)
            wait_for_instance_healthy(instance, 600)
            telemetry_sample.fail(
                "Graceful restart failed, and old EdenFS process resumed"
            )
            print(
                "error: failed to perform graceful restart. The old "
                "edenfs daemon has resumed processing and was not restarted.",
                file=sys.stderr,
            )
            return 1
        except Exception:
            # If we timed out waiting to become healthy, just pass
            # and continue with a force restart
            pass

        print("Recovery unsucessful, forcing a full restart by sending SIGTERM")
        try:
            os.kill(edenfs_pid, signal.SIGTERM)
            self._wait_for_stop(instance, edenfs_pid, timeout=5)
        except Exception:
            # In case we race and the process does not exist by the time we
            # timeout waiting and by the time we call os.kill, just
            # continue on with the restart
            pass
        if self._finish_restart(instance) == 0:
            telemetry_sample.fail(
                "EdenFS was not healthy after graceful restart; performed a "
                "hard restart"
            )
            return 2
        else:
            telemetry_sample.fail(
                "EdenFS was not healthy after graceful restart, and we failed "
                "to restart it"
            )
            return 3

    def _graceful_restart(self, instance: EdenInstance) -> int:
        print("Performing a graceful restart...")
        with instance.get_telemetry_logger().new_sample(
            "graceful_restart"
        ) as telemetry_sample:
            # The status here is returned by the exit status of the startup
            # logger. If this is successful, we will ensure the new process
            # itself starts. If this was not successful, we will assume that
            # the process didn't start up correctly and continue directly to
            # our recovery logic.
            status = daemon.gracefully_restart_edenfs_service(
                instance, daemon_binary=self.args.daemon_binary
            )
            success = status == 0
            if success:
                print("Successful graceful restart")
                return 0

            # After this point, the initial graceful restart was unsuccessful.
            # Make sure that the old process recovers. If it does not recover,
            # run start to make sure that an EdenFS process is running.
            return self._recover_after_failed_graceful_restart(
                instance, telemetry_sample
            )

    def _start(self, instance: EdenInstance) -> int:
        print("edenfs daemon is not currently running. Starting...")
        return daemon.start_edenfs_service(
            instance, daemon_binary=self.args.daemon_binary
        )

    def _full_restart(
        self, instance: EdenInstance, old_pid: int, migrate_to: Optional[str]
    ) -> int:
        print(
            """\
About to perform a full restart of EdenFS.
Note: this will temporarily disrupt access to your EdenFS-managed repositories.
Any programs using files or directories inside the EdenFS mounts will need to
re-open these files after EdenFS is restarted.
"""
        )
        if not self.args.force_restart and sys.stdin.isatty():
            if not prompt_confirmation("Proceed?"):
                print("Not confirmed.")
                return 1

        self._do_stop(instance, old_pid, timeout=15)
        if migrate_to is not None:
            config_mod._do_manual_migration(
                instance, migrate_to, get_migration_success_message
            )
        elif config_mod.should_migrate_mount_protocol_to_nfs(instance):
            config_mod._do_nfs_migration(instance, get_migration_success_message)
        return self._finish_restart(instance)

    def _force_restart(
        self, instance: EdenInstance, old_pid: int, stop_timeout: int
    ) -> int:
        print("Forcing a full restart...")
        if stop_timeout <= 0:
            print("Sending SIGTERM...")
            os.kill(old_pid, signal.SIGTERM)
            self._wait_for_stop(instance, old_pid, timeout=5)
        else:
            self._do_stop(instance, old_pid, stop_timeout)

        return self._finish_restart(instance)

    def _wait_for_stop(self, instance: EdenInstance, pid: int, timeout: float) -> None:
        # If --shutdown-timeout was specified on the command line that always takes
        # precedence over the default timeout passed in by our caller.
        if self.args.shutdown_timeout is not None:
            timeout = typing.cast(float, self.args.shutdown_timeout)
        daemon.wait_for_shutdown(pid, timeout=timeout)

    def _do_stop(self, instance: EdenInstance, pid: int, timeout: int) -> None:
        with instance.get_thrift_client_legacy(timeout=timeout) as client:
            try:
                stop_aux_processes(client)
            except Exception:
                pass
            try:
                message = f"`eden restart --force` requested by pid={os.getpid()}"
                if sys.platform != "win32":
                    message += f" uid={os.getuid()}"
                client.initiateShutdown(message)
            except Exception:
                print("Sending SIGTERM...")
                os.kill(pid, signal.SIGTERM)
        self._wait_for_stop(instance, pid, timeout)

    def _finish_restart(self, instance: EdenInstance) -> int:
        exit_code = daemon.start_edenfs_service(
            instance, daemon_binary=self.args.daemon_binary
        )
        if exit_code != 0:
            print("Failed to start edenfs daemon!", file=sys.stderr)
            return exit_code

        print()
        print("Successfully restarted EdenFS.")

        if sys.platform != "win32":
            print()
            print(
                """\
Note: any programs running inside of an EdenFS-managed directory will need to cd
out of and back into the repository to pick up the new working directory state.
If you see "Transport endpoint not connected" errors from any program this
means it is still attempting to use the old mount point from the previous edenfs
process, and if you see this in your terminal, you should run "cd / && cd -" to
update your shell's working directory."""
            )
        return 0


@subcmd("rage", "Gather diagnostic information about EdenFS")
class RageCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--dry-run",
            action="store_true",
            help="Print the rage without any side-effects (i.e. creating a paste)",
        )
        parser.add_argument(
            "--stdout",
            action="store_true",
            help="Print the rage report to stdout: ignore reporter.",
        )
        parser.add_argument(
            "--stderr",
            action="store_true",
            help="Print the rage report to stderr: ignore reporter.",
        )
        parser.add_argument(
            "--report",
            action="store_true",
            help="Ask the user for additional information and upload a report",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        instance.log_sample("eden_rage")
        rage_processor = instance.get_config_value("rage.reporter", default="")

        # Allow "{hostname}" substitution in rage.reporter config.
        rage_processor = rage_processor.format(hostname=socket.getfqdn())

        if args.report:
            rage_mod.report_edenfs_bug(instance, rage_processor)
            return 0
        else:
            if args.dry_run:
                rage_processor = None

            proc: Optional[subprocess.Popen] = None
            sink: typing.IO[bytes]
            # potential "deadlock" here. This works because rage reporters are not expected
            # to produce any stdout until they've taken all of their stdin. But if they
            # violate that, then the proc.wait() could fail if its stdout pipe was full,
            # since we don't consume it until afterwards.
            if rage_processor:
                proc = subprocess.Popen(
                    shlex.split(rage_processor),
                    stdin=subprocess.PIPE,
                )
                sink = typing.cast(typing.IO[bytes], proc.stdin)
            elif args.stderr:
                proc = None
                sink = sys.stderr.buffer
            else:
                proc = None
                sink = sys.stdout.buffer

            rage_mod.print_diagnostic_info(instance, sink, args.dry_run)

            if proc:
                sink.close()
                proc.wait()

            return 0


@subcmd("uptime", "Determine uptime of the EdenFS service")
class UptimeCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        instance.do_uptime(pretty=True)
        return 0


SHUTDOWN_EXIT_CODE_NORMAL = 0
SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN = 0
SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR = 2
SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL = 3
SHUTDOWN_EXIT_CODE_ERROR = 4


@subcmd("stop", "Shutdown the EdenFS service", aliases=["shutdown"])
class StopCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-t",
            "--timeout",
            type=float,
            default=15.0,
            help="Wait up to TIMEOUT seconds for the daemon to exit "
            "(default=%(default)s). If it does not exit within the timeout, "
            "then SIGKILL will be sent. If timeout is 0, then do not wait at "
            "all and do not send SIGKILL.",
        )
        parser.add_argument(
            "--kill",
            action="store_true",
            default=False,
            help="Forcibly kill edenfs daemon with SIGKILL, rather than attempting to "
            "shut down cleanly.  Not that EdenFS will normally need to re-scan its "
            "data the next time the daemon starts after an unclean shutdown.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        if args.kill:
            return self._kill(instance, args)
        else:
            return self._stop(instance, args)

    def _stop(self, instance: EdenInstance, args: argparse.Namespace) -> int:
        pid = None
        try:
            try:
                with instance.get_thrift_client_legacy(
                    timeout=self.__thrift_timeout(args)
                ) as client:
                    pid = client.getPid()
                    stop_aux_processes(client)
                    # Ask the client to shutdown
                    print(f"Stopping edenfs daemon (pid {pid})...")
                    request_info = f"pid={os.getpid()}"
                    if sys.platform != "win32":
                        # os.getuid() is not available on Windows
                        request_info += f" uid={os.getuid()}"
                    client.initiateShutdown(f"`eden stop` requested by {request_info}")
            except thrift.transport.TTransport.TTransportException as e:
                print_stderr(f"warning: edenfs daemon is not responding: {e}")
                if pid is None:
                    pid = check_health_using_lockfile(instance.state_dir).pid
                    if pid is None:
                        raise EdenNotRunningError(str(instance.state_dir)) from e
        except EdenNotRunningError:
            print_stderr("error: edenfs is not running")
            return SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR

        if args.timeout == 0:
            print_stderr("Sent async shutdown request to edenfs.")
            return SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN

        try:
            if daemon.wait_for_shutdown(pid, timeout=args.timeout):
                print_stderr("edenfs exited cleanly.")
                return SHUTDOWN_EXIT_CODE_NORMAL
            else:
                instance.log_sample("cli_stop", success=False)
                print_stderr("Terminated edenfs with SIGKILL.")
                return SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
        except ShutdownError as ex:
            print_stderr("Error: " + str(ex))
            return SHUTDOWN_EXIT_CODE_ERROR

    def _kill(self, instance: EdenInstance, args: argparse.Namespace) -> int:
        # Get the pid from the lock file rather than trying to query it over thrift.
        # If the user is forcibly trying to kill EdenFS we assume something is probably
        # wrong to start with and the thrift server might not be functional for some
        # reason.
        pid = check_health_using_lockfile(instance.state_dir).pid
        if pid is None:
            print_stderr("error: edenfs is not running")
            return SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR

        try:
            daemon.sigkill_process(pid, timeout=args.timeout)
            print_stderr("Terminated edenfs with SIGKILL.")
            return SHUTDOWN_EXIT_CODE_NORMAL
        except ShutdownError as ex:
            print_stderr("Error: " + str(ex))
            return SHUTDOWN_EXIT_CODE_ERROR

    def __thrift_timeout(self, args: argparse.Namespace) -> float:
        if args.timeout == 0:
            # Default to a 15 second timeout on the thrift call
            return 15.0
        else:
            return args.timeout


def create_parser() -> argparse.ArgumentParser:
    """Returns a parser"""
    parser = argparse.ArgumentParser(
        prog="edenfsctl", description="Manage EdenFS checkouts."
    )
    # TODO: We should probably rename this argument to --state-dir.
    # This directory contains materialized file state and the list of managed checkouts,
    # but doesn't really contain configuration.
    parser.add_argument(
        "--config-dir",
        help="The path to the directory where EdenFS stores its internal state.",
    )
    parser.add_argument(
        "--etc-eden-dir",
        help="Path to directory that holds the system configuration files.",
    )
    parser.add_argument(
        "--home-dir", help="Path to directory where .edenrc config file is stored."
    )
    parser.add_argument("--checkout-dir", help=argparse.SUPPRESS)
    parser.add_argument(
        "--version", "-v", action="store_true", help="Print EdenFS version."
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Enable debug mode (more verbose logging, traceback, etc..)",
    )
    parser.add_argument(
        "--press-to-continue",
        action="store_true",
        help=argparse.SUPPRESS,
    )

    subcmd_add_list: List[Type[Subcmd]] = [
        subcmd_mod.HelpCmd,
        stats_mod.StatsCmd,
        trace_mod.TraceCmd,
        redirect_mod.RedirectCmd,
        prefetch_mod.GlobCmd,
        prefetch_mod.PrefetchCmd,
        prefetch_profile_mod.PrefetchProfileCmd,
    ]

    subcmd_add_list.append(debug_mod.DebugCmd)

    subcmd_mod.add_subcommands(parser, subcmd.commands + subcmd_add_list)

    return parser


def normalize_path_arg(path_arg: str, may_need_tilde_expansion: bool = False) -> str:
    """Normalizes a path by using os.path.realpath().

    Note that this function is expected to be used with command-line arguments.
    If the argument comes from a config file or GUI where tilde expansion is not
    done by the shell, then may_need_tilde_expansion=True should be specified.
    """
    if path_arg:
        if may_need_tilde_expansion:
            path_arg = os.path.expanduser(path_arg)

        # Use the canonical version of the path.
        path_arg = os.path.realpath(path_arg)
    return path_arg


def set_working_directory(args: argparse.Namespace) -> Optional[int]:
    if args.checkout_dir is None:
        return

    try:
        os.chdir(args.checkout_dir)
    except OSError as e:
        print(f"Unable to change to checkout directory: {e}", file=sys.stderr)
        return EX_OSFILE


def is_working_directory_stale() -> bool:
    try:
        os.getcwd()
        return False
    except OSError as ex:
        if ex.errno == errno.ENOTCONN:
            return True
        raise


def check_for_stale_working_directory() -> Optional[int]:
    try:
        if not is_working_directory_stale():
            return None
    except OSError as ex:
        print(
            f"error: unable to determine current working directory: {ex}",
            file=sys.stderr,
        )
        return EX_OSFILE

    # See if we can figure out what the current working directory should be
    # based on the $PWD environment variable that is normally set by most shells.
    #
    # If we have a valid $PWD, cd to it and try to continue using it.
    # This lets commands like "eden doctor" work and report useful data even if
    # the user is running it from a stale directory.
    can_continue = False
    cwd = os.environ.get("PWD")
    if cwd is not None:
        try:
            os.chdir(cwd)
            can_continue = True
        except OSError:
            pass

    msg = """\
Your current working directory appears to be a stale EdenFS
mount point from a previous edenfs daemon instance.
Please run "cd / && cd -" to update your shell's working directory."""
    if not can_continue:
        print(f"Error: {msg}", file=sys.stderr)
        return EX_OSFILE

    print(f"Warning: {msg}", file=sys.stderr)
    doctor_mod.working_directory_was_stale = True
    return None


async def async_main(parser: argparse.ArgumentParser, args: argparse.Namespace) -> int:
    set_return_code = set_working_directory(args)
    if set_return_code is not None:
        return set_return_code

    # Before doing anything else check that the current working directory is valid.
    # This helps catch the case where a user is trying to run the EdenFS CLI inside
    # a stale eden mount point.
    stale_return_code = check_for_stale_working_directory()
    if stale_return_code is not None:
        return stale_return_code

    if args.version:
        return do_version(args)
    if getattr(args, "func", None) is None:
        parser.print_help()
        return EX_OK

    try:
        result = args.func(args)
        if inspect.isawaitable(result):
            result = await result
        return result
    except subcmd_mod.CmdError as ex:
        print(f"error: {ex}", file=sys.stderr)
        return EX_SOFTWARE
    except daemon_util.DaemonBinaryNotFound as ex:
        print(f"error: {ex}", file=sys.stderr)
        return EX_SOFTWARE
    except config_mod.UsageError as ex:
        print(f"error: {ex}", file=sys.stderr)
        return EX_USAGE


# TODO: Remove when we can rely on Python 3.7 everywhere.
try:
    asyncio_run = asyncio.run
except AttributeError:
    asyncio_run = asyncio.get_event_loop().run_until_complete


def main() -> int:
    parser = create_parser()
    args = parser.parse_args()

    # The default event loop on 3.8+ will cause an ugly backtrace when
    # edenfsctl is interrupted. Switching back to the selector event loop.
    if (
        platform.system() == "Windows"
        # pyre-fixme[16]: Windows only
        and getattr(asyncio, "WindowsSelectorEventLoopPolicy", None) is not None
    ):
        # pyre-fixme[16]: Windows only
        asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())

    try:
        ret = asyncio_run(async_main(parser, args))
        if args.press_to_continue:
            input("\n\nPress enter to continue...")
        return ret
    except KeyboardInterrupt:
        # If a Thrift stream is interrupted, Folly EventBus/NotificationQueue
        # gets into a wonky state, and attempting to garbage collect the
        # corresponding Python objects will hang the process. Flush output
        # streams and skip the rest of process shutdown.
        sys.stdout.flush()
        sys.stderr.flush()

        if args.debug:
            raise

        os._exit(130)
        # Pyre doesn't understand that os._exit is noreturn.
        return 130


def zipapp_main() -> None:
    """zipapp_main() is used when running edenfsctl as a Python 3 zipapp executable.
    The zipapp module expects the main function to call sys.exit() on its own if it
    wants to exit with a non-zero status."""
    retcode = main()
    sys.exit(retcode)


if __name__ == "__main__":
    zipapp_main()
