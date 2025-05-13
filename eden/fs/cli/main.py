#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


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
from datetime import datetime, timedelta
from enum import Enum
from pathlib import Path
from typing import Dict, List, Optional, Set, Tuple, Type

from eden.fs.cli.doctor.check_filesystems import check_disk_usage

from eden.fs.cli.util import get_chef_log_path

# Constants
CHEF_LOG_TIMESTAMP_KEY = "chef.run_success_timestamp"
# It is very common for users to get chef warnings on Mondays if this
# time period is shorter than 4 days. So, we report a problem after two weeks.
CHEF_RUN_AGE_PROBLEM = timedelta(days=14)

import thrift.transport
from eden.fs.cli.version import VersionInfo

try:
    from cli.py import usage
except ImportError:
    # in OSS define a stub
    class ParTelemetryStub:
        def set_sample_rate(self, automation: int) -> None:
            pass

    # pyre-fixme[31]: Expression `eden.fs.cli.main.ParTelemetryStub()` is not a
    #  valid type.
    usage = ParTelemetryStub()
try:
    from eden.fs.cli.facebook.hostcaps import is_on_demand
except ImportError:
    # in OSS define a stub
    def is_on_demand() -> bool:
        return False


from eden.fs.cli.buck import get_buck_command, run_buck_command
from eden.fs.cli.config import HG_REPO_TYPES
from eden.fs.cli.doctor.facebook import check_x509
from eden.fs.cli.telemetry import TelemetrySample
from eden.fs.cli.util import (
    check_health_using_lockfile,
    EdenStartError,
    is_apple_silicon,
    wait_for_instance_healthy,
)
from eden.thrift.legacy import EdenClient, EdenNotRunningError

from facebook.eden import EdenService
from facebook.eden.ttypes import (
    ChangeOwnershipRequest,
    MountState,
    SendNotificationRequest,
)
from fb303_core.ttypes import fb303_status

from . import (
    config as config_mod,
    daemon,
    daemon_util,
    debug as debug_mod,
    doctor as doctor_mod,
    filesystem as fs_mod,
    hg_util,
    mtab,
    prefetch as prefetch_mod,
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
from .cmd_util import get_eden_instance, get_fsck_command, require_checkout
from .config import EdenCheckout, EdenInstance, ListMountInfo
from .constants import (
    SHUTDOWN_EXIT_CODE_ERROR,
    SHUTDOWN_EXIT_CODE_NORMAL,
    SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR,
    SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN,
    SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL,
)
from .doctor.problem import ProblemSeverity

if sys.platform == "win32":
    from .file_handler_tools import WinFileHandlerReleaser
from .prompt import prompt_confirmation
from .stats_print import format_size
from .subcmd import Subcmd
from .util import (
    can_enable_windows_symlinks,
    get_environment_suitable_for_subprocess,
    print_stderr,
    ShutdownError,
)

try:
    from .facebook.util import (
        get_migration_success_message,
        migration_restart_help,
        stop_internal_processes,
    )
except ImportError:
    migration_restart_help = "This migrates ALL your mounts to a new mount protocol."

    # pyre-fixme[2]: Parameter must be annotated.
    def get_migration_success_message(migrate_to) -> str:
        return f"Successfully migrated all your mounts to {migrate_to}."

    def stop_internal_processes(_: str) -> None:
        pass


subcmd = subcmd_mod.Decorator()

# For a non-unix system (like Windows), we will define our own error codes.
try:
    EX_OK: int = os.EX_OK
    EX_SOFTWARE: int = os.EX_SOFTWARE
    EX_OSFILE: int = os.EX_OSFILE
except AttributeError:  # On a non-unix system
    EX_OK: int = 0
    EX_SOFTWARE: int = 70
    EX_OSFILE: int = 72

# The Rust CLI depends on this value staying constant. Instead of fetching it
# from the os library, let's just define it here.
EX_USAGE: int = 64


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


def do_version(
    args: argparse.Namespace, format_json: bool = False, verbose: bool = False
) -> int:
    def notNone(x: Optional[str], y: str) -> str:
        return y if x is None else x

    instance = get_eden_instance(args)
    running_version = None
    try:
        running_version = instance.get_running_version()
    except EdenNotRunningError:
        pass

    versions_info = version_mod.get_version_info(running_version)

    if format_json:
        if versions_info.installed_version == "-":
            versions_info.installed_version = None
        if versions_info.running_version == "-":
            versions_info.running_version = None
        info = {
            "installed": versions_info.installed_version,
            "running": versions_info.running_version,
        }
        json.dump(info, sys.stdout, indent=2)
    else:
        if not versions_info.is_eden_running:
            versions_info.running_version = (
                "Unknown (EdenFS does not appear to be running)"
            )

        if verbose:
            inst_days_old = (
                None
                if versions_info.installed_version_age is None
                else f" ({versions_info.installed_version_age} days old)"
            )
            print(
                f"Installed: {versions_info.installed_version}{notNone(inst_days_old, '')}"
            )
            running_days_old = (
                None
                if versions_info.running_version_age is None
                else f" ({versions_info.running_version_age} days old)"
            )
            print(
                f"Running:   {versions_info.running_version}{notNone(running_days_old, '')}"
            )
            if versions_info.ages_deltas:
                print(
                    f"Running version is {versions_info.ages_deltas} days older than installed"
                )
        else:
            print(f"Installed: {notNone(versions_info.installed_version, '-')}")
            print(f"Running:   {notNone(versions_info.running_version, '-')}")

        if versions_info.is_dev:
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
        parser.add_argument(
            "--verbose",
            action="store_true",
            help="Add more info to the output, such as elapsed days since each version and delta between them",
        )

    def run(self, args: argparse.Namespace) -> int:
        return do_version(args, args.json, args.verbose)


@subcmd("info", "Get details about a checkout")
class InfoCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "client", default=None, nargs=argparse.OPTIONAL, help="Path to the checkout"
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.client)
        info = instance.get_checkout_info_from_checkout(checkout)
        json.dump(info, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0


@subcmd("du", "Show disk space usage for a checkout")
class DiskUsageCmd(Subcmd):
    # pyre-fixme[4]: Attribute must be annotated.
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
    # pyre-fixme[4]: Attribute must be annotated.
    color_out = ui.get_output()
    hasLFS = False
    hasWorkingCopyBacked = False

    json_mode = False

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "mounts",
            default=[],
            nargs=argparse.ZERO_OR_MORE,
            help="Names of the mount points",
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

    # pyre-fixme[2]: Parameter must be annotated.
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

    # pyre-fixme[2]: Parameter must be annotated.
    def du(self, path) -> int:
        dev = os.stat(path).st_dev

        # pyre-fixme[53]: Captured variable `dev` is not annotated.
        # pyre-fixme[2]: Parameter must be annotated.
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

    # pyre-fixme[2]: Parameter must be annotated.
    def write_ui(self, message, fg=None) -> None:
        if not self.json_mode:
            self.color_out.write(message, fg=fg)

    # pyre-fixme[2]: Parameter must be annotated.
    def writeln_ui(self, message, fg=None) -> None:
        self.write_ui(f"{message}\n")

    def usage_for_dir(
        self,
        # pyre-fixme[2]: Parameter must be annotated.
        path,
        usage_type: str,
        print_label: Optional[str] = None,
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
A filesystem check recovered data and stored it at:
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


@subcmd("socket", "Print the daemon's socket path if it exists")
class SocketCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        raise NotImplementedError("Stub -- only implemented in Rust")


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
        parser.add_argument("--daemon-binary", "-d", help=argparse.SUPPRESS)
        parser.add_argument(
            "--daemon-args",
            dest="edenfs_args",
            nargs=argparse.REMAINDER,
            help=argparse.SUPPRESS,
        )
        parser.add_argument(
            "--preserved-vars",
            "-p",
            nargs=argparse.ZERO_OR_MORE,
            help=argparse.SUPPRESS,
        )

        parser.add_argument(
            "--nfs",
            dest="nfs",
            action="store_true",
            default=is_apple_silicon(),
            help=argparse.SUPPRESS,
        )

        parser.add_argument(
            "--overlay-type",
            choices=sorted(config_mod.SUPPORTED_INODE_CATALOG_TYPES),
            default=None,
            help="Specify overlay type",
        )

        parser.add_argument(
            "--backing-store",
            help=(
                "Clone the path with a specified Backing Store implementation. "
                "Currently only supports 'filteredhg' (all), 'recas' (Linux), "
                "and 'http' (Linux). Takes precedent over the inferred backing"
                "store type from the existing repository we're cloning from."
            ),
        )

        parser.add_argument(
            "--re-use-case",
            help="The Remote Execution use-case to use when --backing-store=recas",
        )

        parser.add_argument(
            "--enable-windows-symlinks",
            action="store_true",
            help="Enable symlink support for the cloned mount",
        )

        parser.add_argument(
            "--filter-path",
            help=(
                "The FilteredFS filter to activate when "
                "--backing-store=filteredhg. When this option is omitted, no "
                "filter is applied to the repo but FilteredFS is still used. "
                'Passing "" or "null" will result in similar behavior.'
            ),
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
                print_stderr(f"error: destination path {args.path} is not empty")
                return 1
        except OSError as ex:
            if ex.errno == errno.ENOTDIR:
                print_stderr(f"error: destination path {args.path} is not a directory")
                return 1
            elif ex.errno != errno.ENOENT:
                print_stderr(
                    f"error: unable to access destination path {args.path}: {ex}"
                )
                return 1

        # pyre-fixme[53]: Captured variable `instance` is not annotated.
        # pyre-fixme[3]: Return type must be annotated.
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

        # Check if requested path is inside an existing checkout or backing_repo of existing checkout
        instance = EdenInstance(args.config_dir, args.etc_eden_dir, args.home_dir)
        problem_type, existing_checkout = config_mod.detect_checkout_path_problem(
            args.path,
            instance,
        )
        if problem_type is not None and existing_checkout is not None:
            if problem_type == config_mod.CheckoutPathProblemType.NESTED_CHECKOUT:
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
            if problem_type == config_mod.CheckoutPathProblemType.INSIDE_BACKING_REPO:
                print_stderr(
                    f"""\
    error: destination path {args.path} is being created within backing repo of an existing checkout
    {existing_checkout.path} located at {existing_checkout.get_backing_repo_path()}.

    Checkouts inside backing repo are usually not intended/recommended and may cause
    `eden doctor` and `eden rm` to encounter spurious behavior and may also degrade performance
    of source control operations."""
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

        # The null (empty) filter can be specified by:
        #   1) Not supplying the --filter-path arg
        #   2) Supplying an empty string to --filter-path
        #   3) Specifying the --filter-path as "null" (or any variation)
        filter_path = args.filter_path or ""
        if filter_path == "" or filter_path.lower() == "null":
            filter_path = None

        # Filters are only valid for repos using FilteredFS
        if filter_path and not args.backing_store == "filteredhg":
            print_stderr(
                "error: --filter-path can only be used with --backing-store=filteredhg"
            )
            return 1

        # Find the repository information
        try:
            enable_windows_symlinks = (
                args.enable_windows_symlinks
                or instance.get_config_bool("experimental.windows-symlinks", False)
            ) and can_enable_windows_symlinks()
            repo, repo_config = config_mod.get_repo_info(
                instance,
                args.repo,
                args.rev,
                args.nfs,
                args.case_sensitive,
                overlay_type=args.overlay_type,
                backing_store_type=args.backing_store,
                re_use_case=args.re_use_case,
                enable_windows_symlinks=enable_windows_symlinks,
                off_mount_repo_dir=instance.get_config_bool(
                    "clone.off-mount-repo-dir",
                    # Enable by default in tests.
                    any(v in os.environ for v in ("INTEGRATION_TEST", "TESTTMP")),
                ),
            )
        except util.RepoError as ex:
            print_stderr("error: {}", ex)
            return 1

        # If it's source control repository
        if not args.backing_store or args.backing_store in HG_REPO_TYPES:
            # Find the commit to check out
            if args.rev is not None:
                try:
                    commit = repo.get_commit_hash(args.rev)
                except Exception as ex:
                    print_stderr(
                        f"error: unable to find hash for commit {args.rev!r}: {ex}"
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
            if sys.platform != "linux":
                print_stderr(
                    "error: recas backing store was passed but this feature is only available on Linux"
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
            raise util.RepoError(f"Unsupported backing store {args.backing_store}.")

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
                instance,
                args.daemon_binary,
                args.edenfs_args,
                args.preserved_vars,
            )
            if exit_code != 0:
                return exit_code

        print(f"Cloning new repository at {args.path}...")

        try:
            instance.clone(repo_config, args.path, commit, filter_path)
            print(f"Success.  Checked out commit {commit:.8}")
            # In the future it would probably be nice to fork a background
            # process here to prefetch files that we think the user is likely
            # to want to access soon.
            return 0
        except Exception as ex:
            print_stderr("error: {}", ex)
            return 1


@subcmd("config", "Query EdenFS CLI configuration")
class ConfigCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        raise NotImplementedError("Stub -- only implemented in Rust")


if sys.platform == "win32":

    @subcmd("handles", "Get list of open file handles for a checkout")
    class HandlesCmd(Subcmd):
        def run(self, args: argparse.Namespace) -> int:
            raise NotImplementedError("Stub -- only implemented in Rust")


@subcmd("fsconfig", "Query EdenFS daemon configuration")
class FsConfigCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--all",
            action="store_true",
            default=False,
            help="Show all configuration values",
        )

    def run(self, args: argparse.Namespace) -> int:
        raise NotImplementedError("Stub -- only implemented in Rust")


@subcmd(
    "reloadconfig",
    "Reload EdenFS dynamic configs. This invokes edenfs_config_manager under the hood",
)
class ReloadConfigCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "-n",
            "--dry-run",
            action="store_true",
            help="Dry run mode. Just print the config to stdout instead of writing it to disk",
        )
        parser.add_argument(
            "--local-telemetry",
            metavar="PATH",
            help="Log telemetry samples to a local file rather than to scuba (mainly for "
            "debugging and development)",
        )
        parser.add_argument(
            "--out",
            metavar="PATH",
            help="Write filtered config file to custom location",
        )
        parser.add_argument(
            "--raw-out",
            metavar="PATH",
            help="Read and write location of the raw config which will be used if Configerator sends back an `edenfs_uptodate` response",
        )
        parser.add_argument(
            "-t",
            "--timeout",
            default=5,
            type=int,
            action="store",
            help="Number of seconds to wait for HTTP post response while fetching configs",
        )
        parser.add_argument(
            "-c",
            "--local-cfgr-root",
            type=str,
            action="store",
            help="Load configs from the given local configerator repo instead of reading from remote. This is useful for testing changes locally without having to push them to production",
        )
        parser.add_argument(
            "-v", "--verbose", action="store_true", help="Enable more verbose logging"
        )

    def run(self, args: argparse.Namespace) -> int:
        raise NotImplementedError("Stub -- only implemented in Rust")


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
        parser.add_argument(
            "--fast",
            action="store_true",
            help="Only run fast doctor checker, may miss some issues. This is "
            "intended to be run by tools running doctor in a continuous manner "
            "such as IDEs.",
        )
        parser.add_argument(
            "--no-warnings",
            action="store_true",
            help="Don't show warnings (but still log them to scuba). ",
        )
        parser.add_argument(
            "--wait",
            action="store_true",
            default=False,
            help="Wait to run if other instances of doctor are running. Without this flag, doctor times out after 10s. ",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        doctor = doctor_mod.EdenDoctor(
            instance,
            args.dry_run,
            args.debug,
            args.fast,
            args.wait,
            (
                ProblemSeverity.POTENTIALLY_SERIOUS
                if args.no_warnings
                else ProblemSeverity.ALL
            ),
        )
        if args.current_edenfs_only:
            doctor.run_system_wide_checks = False
        return doctor.cure_what_ails_you()


@subcmd("health-report", "Notify critical eden issues")
class HealthReportCmd(Subcmd):
    class ErrorCode(Enum):
        EDEN_NOT_RUNNING = 1
        STALE_EDEN_VERSION = 2
        INVALID_CERTS = 3
        NO_REPO_MOUNT_FOUND = 4
        CHEF_NOT_RUNNING = 5
        LOW_DISK_SPACE = 6

        def summary(self) -> str:
            summary = {
                self.EDEN_NOT_RUNNING: "Eden not running properly",
                self.STALE_EDEN_VERSION: "Stale Eden version running",
                self.INVALID_CERTS: "Invalid/Expired user certs detected",
                self.NO_REPO_MOUNT_FOUND: "Eden checkouts not mounted properly",
                self.CHEF_NOT_RUNNING: "Chef doesn't seem to be running properly",
                self.LOW_DISK_SPACE: "Low disk space for EdenFS checkouts",
            }
            return summary[self]

        def remediation(self) -> str:
            remediation = {
                self.EDEN_NOT_RUNNING: "Please run `Restart EdenFS` from E-menu",
                self.STALE_EDEN_VERSION: "Please run `Restart EdenFS` from E-menu",
                self.INVALID_CERTS: "Please `Renew SKS-Backed Certificates` from F-menu",
                self.NO_REPO_MOUNT_FOUND: "Please run `Eden Doctor` from E-menu -> Diagnostics",
                self.CHEF_NOT_RUNNING: "Please run `Fix My Server/Mac/Windows` from F-menu",
                self.LOW_DISK_SPACE: "Please run `Clean EdenFs Disk (du --clean)` from E-menu -> Diagnostics",
            }
            return remediation[self]

    running_version: str = ""
    # a value of 0 turns off the stale version check
    running_version_days_threshold = 30
    version_info: VersionInfo = VersionInfo()
    error_codes: Dict[ErrorCode, str] = {}

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--mounts",
            default=[],
            nargs=argparse.ZERO_OR_MORE,
            help="path of the mount points",
            dest="mounts",
        )
        parser.add_argument(
            "--notify",
            default=False,
            action="store_true",
            dest="notify",
            help="If set, this command will ask the server to notify the user of errors encountered during health-check. This is currently only available on Windows",
        )

    def is_eden_running(self, instance: EdenInstance) -> bool:
        health_info = instance.check_health()
        if health_info.is_starting():
            print("EdenFS daemon is still starting. Waiting for EdenFS to start ...")
            try:
                wait_for_instance_healthy(instance, 600)
            except Exception as error:
                self.error_codes[HealthReportCmd.ErrorCode.EDEN_NOT_RUNNING] = (
                    "EdenFS start error: " + error.args[0]
                )
                return False

        elif not health_info.is_healthy():
            self.error_codes[HealthReportCmd.ErrorCode.EDEN_NOT_RUNNING] = (
                "Failed to find EdenFS daemon pid."
            )
            return False

        try:
            self.running_version = instance.get_running_version()
        except EdenNotRunningError:
            self.error_codes[HealthReportCmd.ErrorCode.EDEN_NOT_RUNNING] = (
                "Failed to retrieve EdenFS running version."
            )
            return False

        self.version_info = version_mod.get_version_info(self.running_version)
        if not self.version_info.is_eden_running:
            self.error_codes[HealthReportCmd.ErrorCode.EDEN_NOT_RUNNING] = (
                "Failed to retrieve EdenFS running version."
            )
            return False
        return True

    def is_eden_up_to_date(self) -> bool:
        """Checks if running version is newer than a pre-configured threshold.
        If provided threshold is 0, the check is skipped and returns True."""
        if (
            self.running_version_days_threshold != 0
            and self.version_info.ages_deltas is not None
            and self.version_info.ages_deltas >= self.running_version_days_threshold
        ):
            self.error_codes[HealthReportCmd.ErrorCode.STALE_EDEN_VERSION] = (
                "Running EdenFS version: "
                + (self.version_info.running_version or "")
                + ", installed EdenFS version: "
                + (self.version_info.installed_version or "")
                + f". The running EdenFS daemon is over {self.running_version_days_threshold} days out-of-date."
            )
            return False
        return True

    def are_certs_valid(self) -> bool:
        if util.is_sandcastle() or util.x2p_enabled():
            return True

        if (cert := check_x509.find_x509_path()) and check_x509.validate_x509(cert):
            return True
        # cert error!
        self.error_codes[HealthReportCmd.ErrorCode.INVALID_CERTS] = (
            "Failed to validate x509 certificates."
        )
        return False

    def is_repo_mounted(self, instance: EdenInstance, mounts: List[str]) -> bool:
        try:
            checkouts_info = doctor_mod.get_checkouts_info(instance)
            unmounted_repos = {
                str(checkout.path)
                for path, checkout in checkouts_info.items()
                if checkout.state is None and (not mounts or str(path) in mounts)
            }

            if unmounted_repos:
                self.error_codes[HealthReportCmd.ErrorCode.NO_REPO_MOUNT_FOUND] = (
                    ", ".join(unmounted_repos) + " not mounted correctly."
                )
                return False

            return True

        except Exception as ex:
            self.error_codes[HealthReportCmd.ErrorCode.NO_REPO_MOUNT_FOUND] = " ".join(
                ["Failed to retrieve EdenFS checkouts info: ", ex.args[0]]
            )
            return False

    def is_chef_running(self) -> bool:
        """Examine the status of Chef runs."""
        if is_on_demand() or get_chef_log_path(platform.system()) is None:
            print("Skipping chef run check for unsupported platform.")
            return True

        chef_log_path = get_chef_log_path(platform.system())

        try:
            with open(str(chef_log_path), "r") as f:
                chef_log_raw = f.read()
                chef_log = json.loads(chef_log_raw)
                last_chef_run_sec = chef_log[CHEF_LOG_TIMESTAMP_KEY]

                if not isinstance(last_chef_run_sec, (int, float)):
                    self.error_codes[HealthReportCmd.ErrorCode.CHEF_NOT_RUNNING] = (
                        "Invalid/missing timestamp in "
                        + CHEF_LOG_TIMESTAMP_KEY
                        + ". Chef doesn't seem to be running on your machine."
                    )
                    return False

                last_chef_run = datetime.fromtimestamp(last_chef_run_sec)

                ms_since_last_run = (
                    datetime.now() - last_chef_run
                ).total_seconds() * 1000

                if ms_since_last_run >= CHEF_RUN_AGE_PROBLEM.total_seconds() * 1000:
                    self.error_codes[HealthReportCmd.ErrorCode.CHEF_NOT_RUNNING] = (
                        "Last run was "
                        + str((ms_since_last_run / 3600000))
                        + " hours ago."
                        + " Chef doesn't seem to be running on your machine."
                    )
                    return False
                return True
        except Exception as e:
            self.error_codes[HealthReportCmd.ErrorCode.CHEF_NOT_RUNNING] = (
                "Failed to load chef log at "
                + str(chef_log_path)
                + " with error: "
                + str(e.args[0])
            )
            return False

    def has_enough_disk_space(self, instance: EdenInstance, mounts: List[str]) -> bool:
        try:
            mount_paths = mounts or instance.get_mount_paths()
            disk_space_issues = check_disk_usage(
                None, mount_paths, instance, fs_mod.new()
            )
            if disk_space_issues is not None:
                self.error_codes[HealthReportCmd.ErrorCode.LOW_DISK_SPACE] = (
                    disk_space_issues
                )
                return False
        except Exception as e:
            self.error_codes[HealthReportCmd.ErrorCode.LOW_DISK_SPACE] = (
                "Failed to check disk space usage: " + str(e.args[0])
            )
            return False
        return True

    @staticmethod
    def print_and_notify_errors(
        instance: EdenInstance, out: ui.Output, notify: bool
    ) -> None:
        """
        Serialize and print error codes in JSON format. Optionally send notifications.
        Args:
            instance (EdenInstance): The instance to use for sending notifications.
            out (ui.Output): Output stream to write the JSON data to.
            notify (bool): Whether to send notifications.
        """
        # Serialize error codes
        data = [
            {"error": error_code.name, "description": error_additional_info}
            for error_code, error_additional_info in HealthReportCmd.error_codes.items()
        ]

        # Print JSON data
        json_str = json.dumps(data, indent=2)
        out.writeln(json_str)

        config_notify_health_report = instance.get_config_bool(
            "notifications.notify-health-report-issues", False
        )
        # Send notifications if enabled and on Windows platform
        if notify and config_notify_health_report and sys.platform == "win32":
            for error_code in HealthReportCmd.error_codes.keys():
                try:
                    with instance.get_thrift_client_legacy() as client:
                        request = SendNotificationRequest(
                            title=error_code.summary(),
                            description=error_code.remediation(),
                        )
                        client.sendNotification(request)
                except thrift.transport.TTransport.TTransportException as e:
                    # Ignore TTransportException if it is a UNKNOWN_METHOD error, this can
                    # happen if the running version predates this endpoint
                    if e.type != thrift.Thrift.TApplicationException.UNKNOWN_METHOD:
                        print_stderr(f"warning: edenfs daemon is not responding: {e}")
                except EdenNotRunningError:
                    print_stderr("error: edenfs is not running")
                except Exception as e:
                    print_stderr(f"error: {e}")

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        mounts = args.mounts or []
        self.running_version_days_threshold = instance.get_config_int(
            "notifications.health-report-stale-version-threshold-days",
            self.running_version_days_threshold,
        )

        # don't run health-report if there are no eden mounts
        if not instance.get_mount_paths():
            return 0

        out = ui.get_output()
        exit_code = 0

        with instance.get_telemetry_logger().new_sample(
            "health-report"
        ) as health_report_logger:
            try:
                if (
                    not self.is_eden_running(instance)
                    or not self.is_repo_mounted(instance, mounts)
                    or not all(
                        f()
                        for f in [
                            self.is_eden_up_to_date,
                            self.are_certs_valid,
                            self.is_chef_running,
                        ]
                    )
                    or not self.has_enough_disk_space(instance, mounts)
                ):
                    exit_code = 1

                health_report_logger.add_int(
                    "num_detected_issues", len(HealthReportCmd.error_codes)
                )
                health_report_logger.add_normvector(
                    "detected_issues",
                    [
                        str(error_code)
                        for error_code in HealthReportCmd.error_codes.keys()
                    ],
                )
                health_report_logger.add_tags(
                    "detected_issues_descriptions",
                    set(HealthReportCmd.error_codes.values()),
                )
            except Exception as ex:
                print(f"Failed to run eden health report: {str(ex)}", file=sys.stderr)
                health_report_logger.add_int("num_detected_issues", -1)
                health_report_logger.add_tags(
                    "detected_issues_descriptions",
                    {str(ex)},
                )
                # Don't attempt to print and notify errors if we hit an exception while executing checks, just return 255 immediately
                return 255

        try:
            self.print_and_notify_errors(instance, out, args.notify)
        except Exception as ex:
            print(
                f"Failed to print and notify after eden health report: {str(ex)}",
                file=sys.stderr,
            )
            exit_code = 255

        return exit_code


@subcmd("strace", "Monitor FUSE requests.")
class StraceCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "checkout",
            default=None,
            nargs=argparse.OPTIONAL,
            help="Path to the checkout",
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
        return EX_USAGE


@subcmd("notify", "Provides a list of filesystem changes since the specified position")
class NotifyCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        print_stderr("This is not implemented for python edenfsctl.")
        return EX_USAGE


@subcmd(
    "prefetch-profile",
    "Create, manage, and use Prefetch Profiles. Use `eden prefetch-profile help` to see more detailed help text.",
    aliases=["pp"],
)
class PrefetchProfileCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        print_stderr("This is not implemented for python edenfsctl.")
        return EX_USAGE


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
            nargs=argparse.ZERO_OR_MORE,
            help="The path to an EdenFS checkout to verify.",
        )
        parser.add_argument(
            "--num-error-discovery-threads",
            default=4,
            help="Specifies the number of threads that the OverlayChecker will use for error discovery",
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
                result = self.check_one(
                    args, get_eden_instance(args), Path(path), Path(path)
                )
            else:
                instance, checkout, rel_path = require_checkout(args, path)
                result = self.check_one(
                    args, instance, checkout.path, checkout.state_dir
                )
            return_codes.append(result)

        return return_codes

    def check_all(self, args: argparse.Namespace) -> List[int]:
        # Check all configured checkouts that are not currently mounted.
        instance = get_eden_instance(args)
        return_codes: List[int] = []
        for checkout in instance.get_checkouts():
            result = self.check_one(
                args,
                instance,
                checkout.path,
                checkout.state_dir,
            )
            return_codes.append(result)

        return return_codes

    def check_one(
        self,
        args: argparse.Namespace,
        instance: EdenInstance,
        checkout_path: Path,
        state_dir: Path,
    ) -> int:
        print(f"Checking {checkout_path}...")
        overlay_path = state_dir / "local"
        num_threads = args.num_error_discovery_threads
        instance.get_config_int("fsck.num-error-discovery-threads", num_threads)
        return subprocess.call(
            [
                get_fsck_command(),
                overlay_path,
                f"--dry-run={'true' if args.check_only else 'false'}",
                f"--force={'true' if args.force else 'false'}",
                f"--num_error_discovery_threads={num_threads}",
            ]
        )


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
            try:
                request = ChangeOwnershipRequest(mountPoint=args.path, uid=uid, gid=gid)
                client.changeOwnership(request)
            except thrift.Thrift.TApplicationException as exc:
                if exc.type == thrift.Thrift.TApplicationException.UNKNOWN_METHOD:
                    client.chown(args.path, uid, gid)
                else:
                    raise exc
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
            "paths",
            nargs=argparse.ZERO_OR_MORE,
            metavar="path",
            help="The checkout mount path",
        )
        parser.add_argument(
            "--read-only", action="store_true", dest="read_only", help="Read only mount"
        )
        parser.add_argument(
            "--all",
            action="store_true",
            dest="mount_all",
            help="Remount all unmounted checkouts",
        )

    def run(self, args: argparse.Namespace) -> int:
        # validation
        if not args.mount_all and len(args.paths) == 0:
            raise config_mod.UsageError(
                "Use either '--all' or path arguments to specify what to mount"
            )

        if args.mount_all and len(args.paths) > 0:
            raise config_mod.UsageError(
                "Flag '--all' and path arguments cannot be combined"
            )

        instance = get_eden_instance(args)

        exitcode = 0

        for path in args.paths:
            try:
                exitcode = instance.mount(path, args.read_only)
                if exitcode:
                    return exitcode
            except (EdenService.EdenError, EdenNotRunningError) as ex:
                print_stderr("error: {}", ex)
                return 1

        if args.mount_all:
            exitcode = self.remount_checkouts(instance, args.read_only)

        return exitcode

    def remount_checkouts(self, instance: EdenInstance, read_only: bool) -> int:
        exitcode = 0
        mounts = instance.get_mounts()
        for path, mount_info in sorted(mounts.items()):
            if mount_info.state is None:
                print(f"Found unmounted checkout at {path}, attempting to mount")
                try:
                    mount_exitcode = instance.mount(path, read_only)
                    if mount_exitcode:
                        print(
                            f"Remount {path} failed. exit_code={mount_exitcode}. Please run `eden doctor` to fix it"
                        )
                        exitcode = 1
                    else:
                        print(f"Mount succeeded, path: {path}")
                except Exception as ex:
                    print(
                        f"Failed to mount {path}:\n{ex}\nPlease run 'eden doctor' to fix it"
                    )
                    exitcode = 1
        return exitcode


# Types of removal
#
# * ACTIVE_MOUNT: removing a mounted repository, we need to talk with EdenFS
# daemon to get it unmounted.
# * INACTIVE_MOUNT: removing an unmounted repository, we can simply delete
# its configuration.
# * CLEANUP_ONLY: removing an unknown directory, it might be an old EdenFS
# mount that failed to clean up. We try to clean it up again in this case.
# NON_EDEN: removing a non-EdenFS directory or file using eden rm - in this
# case we want to skip the Eden housekeeping after removing it
class RemoveType(enum.Enum):
    ACTIVE_MOUNT = 0
    INACTIVE_MOUNT = 1
    CLEANUP_ONLY = 2
    NON_EDEN = 3


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
            "paths",
            nargs=argparse.ONE_OR_MORE,
            metavar="path",
            help="The EdenFS checkout(s) to remove",
        )
        parser.add_argument(
            "--no-force",
            default=False,
            action="store_true",
            help=argparse.SUPPRESS,
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

    # pyre-fixme[3]: Return type must be annotated.
    def optional_traceback(self, ex: Exception, debug: bool):
        if debug:
            traceback.print_exception(ex)
        else:
            print_stderr(
                "Rerun 'eden remove' with --debug to see full traceback (required to report issues; please do so if the error is unexpected or ambiguous)"
            )

    def delete_file_with_confirmation(self, path: str) -> int:
        prompt = f"""\
Warning: the following is a file, not a directory or an EdenFS mount: {path}
Do you still want to delete {path}?"""
        if not prompt_confirmation(prompt):
            return 2
        try:
            os.unlink(path)
        except Exception as ex:
            print(f"error: cannot remove contents of {path}: {ex}")
            return 1
        return 0

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        configured_mounts = list(instance.get_mount_paths())

        # First translate the list of paths into canonical checkout paths
        # We also track a bool indicating if this checkout is currently mounted
        mounts: List[Tuple[str, RemoveType]] = []
        for path in args.paths:
            if os.path.isfile(path):
                if args.prompt and sys.stdin.isatty():
                    return self.delete_file_with_confirmation(path)
                else:
                    print(
                        f"error: {path} exists but it's a file; remove it manually.",
                        file=sys.stderr,
                    )
                    return 1

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
                remove_type = RemoveType.NON_EDEN
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
Warning: the following is not an EdenFS mount: {path}
Any files in this directory will be lost forever.
Do you still want to delete {path}?"""
                        if not prompt_confirmation(prompt):
                            return 2
                        else:
                            try:
                                fs_mod.new().rmdir(path, args.preserve_mount_point)
                                return 0
                            except Exception as ex:
                                if sys.platform != "win32":
                                    print(
                                        f"Error: cannot remove contents of {path}: {ex}"
                                    )
                                    return 1
                                else:
                                    winhr = WinFileHandlerReleaser(instance)
                                    maybe_succeeded = winhr.try_release(path)
                                    try:
                                        # Try again after try_release
                                        if maybe_succeeded:
                                            fs_mod.new().rmdir(
                                                path, args.preserve_mount_point
                                            )
                                        else:
                                            return 1
                                    except Exception as ex:
                                        print(
                                            f"Error: cannot remove contents of {path} even after trying to kill processes holding resources: {ex}"
                                        )
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
            if remove_type != RemoveType.NON_EDEN:
                mounts.append((mount_path, remove_type))

        # Warn the user since this operation permanently destroys data
        if args.prompt and sys.stdin.isatty() and len(mounts) > 0:
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
            # Removing reidrection targets from checkout config to allow deletion of redirected paths
            instance, checkout, _rel_path = require_checkout(args, mount)
            config = checkout.get_config()
            config._replace(
                redirection_targets={},
            )
            checkout.save_config(config)

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
                    instance.unmount(mount, use_force=not args.no_force)
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
                self.optional_traceback(ex, args.debug)
            else:
                try:
                    print(f"Cleaning up mount {mount}")
                    instance.cleanup_mount(
                        Path(mount), args.preserve_mount_point, args.debug
                    )
                except Exception as ex:
                    print_stderr(f"error cleaning up mount {mount}: {ex}")
                    exit_code = 1
                    self.optional_traceback(ex, args.debug)

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
            nargs=argparse.ONE_OR_MORE,
            metavar="path",
            help="Path where checkout should be unmounted from",
        )
        parser.add_argument(
            "--no-force", default=False, action="store_true", help=argparse.SUPPRESS
        )
        parser.add_argument(
            "--debug-mark-unintentional",
            default=False,
            action="store_true",
            help=argparse.SUPPRESS,
        )

    def run(self, args: argparse.Namespace) -> int:
        if args.destroy:
            print_stderr(
                'note: "eden unmount --destroy" is deprecated; '
                'prefer using "eden rm" instead'
            )

        for path in args.paths:
            # Removing redirection targets from checkout config to allow deletion of redirected paths
            instance, checkout, _rel_path = require_checkout(args, path)
            config = checkout.get_config()
            config._replace(
                redirection_targets={},
            )
            checkout.save_config(config)

            path = normalize_path_arg(path)
            try:
                instance.unmount(
                    path,
                    use_force=not args.no_force,
                    unintentional_unmount=args.debug_mark_unintentional,
                )
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
            "--daemon-binary", "-d", help="Path to the binary for the edenfs daemon."
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
        parser.add_argument(
            "--preserved-vars",
            "-p",
            nargs=argparse.ZERO_OR_MORE,
            help=(
                "By default, the EdenFS daemon is started with a limited set "
                "of environment variables. This option specifies additional "
                "environment variables that should be preserved (or passed) "
                "when starting a new daemon. NOTE: this should only contain "
                "the names of the env vars, not their desired values."
            ),
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
                instance, daemon_binary, args.edenfs_args, args.preserved_vars
            )

        if config_mod.should_migrate_mount_protocol_to_nfs(instance):
            config_mod._do_nfs_migration(instance, get_migration_success_message)
        if config_mod.should_migrate_inode_catalog_to_in_memory(instance):
            config_mod._do_in_memory_inode_catalog_migration(instance)
        result = daemon.start_edenfs_service(
            instance, daemon_binary, args.edenfs_args, args.preserved_vars
        )

        # show Eden ready notification only if there are any active eden mounts
        if instance.get_mount_paths():
            should_notify_eden_ready_status = instance.get_config_bool(
                "notifications.notify-eden-ready", False
            )
            if sys.platform == "win32" and should_notify_eden_ready_status:
                self.send_edenfs_notification(instance, result)
        return result

    def send_edenfs_notification(self, instance: EdenInstance, result: int) -> None:
        """Send notification for EdenFS health status."""
        health_info = instance.check_health()
        try:
            with instance.get_thrift_client_legacy() as client:
                if result == 1 or not health_info.is_healthy():
                    request = SendNotificationRequest(
                        title="EdenFS not healthy",
                        description="EdenFS failed to start properly.",
                    )
                elif health_info.is_starting():
                    updated_health_info = wait_for_instance_healthy(
                        instance, timeout=600
                    )
                    if updated_health_info.is_healthy():
                        edenfs_pid = updated_health_info.pid
                        request = SendNotificationRequest(
                            title="EdenFS ready for use",
                            description=f"EdenFS started with pid: {edenfs_pid}",
                        )
                    else:
                        request = SendNotificationRequest(
                            title="EdenFS not healthy",
                            description="EdenFS failed to start properly.",
                        )
                else:
                    edenfs_pid = health_info.pid
                    request = SendNotificationRequest(
                        title="EdenFS ready for use",
                        description=f"EdenFS started with pid: {edenfs_pid}",
                    )
                client.sendNotification(request)
        except (
            thrift.transport.TTransport.TTransportException,
            EdenNotRunningError,
        ) as e:
            print_stderr(f"EdenFS not running: {e}")
        except Exception as e:
            print_stderr(f"unexpected error: {e}")

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
        eden_env = daemon.get_edenfs_environment(args.preserved_vars)
        cmd, eden_env = daemon.prepare_edenfs_privileges(
            daemon_binary, cmd, eden_env, privhelper
        )

        if sys.platform == "win32":
            return subprocess.call(cmd, env=eden_env)
        else:
            os.execvpe(cmd[0], cmd, env=eden_env)
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
class RestartCmd(Subcmd):
    # pyre-fixme[13]: Attribute `args` is never initialized.
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
            "--daemon-binary", "-d", help="Path to the binary for the edenfs daemon."
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
            help="Only perform a restart if there is already an EdenFS instance "
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
        parser.add_argument(
            "--preserved-vars",
            "-p",
            nargs=argparse.ZERO_OR_MORE,
            help=(
                "By default, the EdenFS daemon is started with a limited set "
                "of environment variables. This option specifies additional "
                "environment variables that should be preserved (or passed) "
                "when starting a new daemon. NOTE: this should only contain "
                "the names of the env vars, not their desired values."
            ),
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

        print("Recovery unsuccessful, forcing a full restart by sending SIGTERM")
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
                instance,
                daemon_binary=self.args.daemon_binary,
                preserved_env=self.args.preserved_vars,
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
            instance,
            daemon_binary=self.args.daemon_binary,
            preserved_env=self.args.preserved_vars,
        )

    def _full_restart(
        self,
        instance: EdenInstance,
        old_pid: int,
        migrate_to: Optional[str],
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
            instance,
            daemon_binary=self.args.daemon_binary,
            preserved_env=self.args.preserved_vars,
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

            # pyre-fixme[24]: Generic type `subprocess.Popen` expects 1 type parameter.
            proc: Optional[subprocess.Popen] = None
            sink: typing.IO[bytes]
            # potential "deadlock" here. This works because rage reporters are not expected
            # to produce any stdout until they've taken all of their stdin. But if they
            # violate that, then the proc.wait() could fail if its stdout pipe was full,
            # since we don't consume it until afterwards.
            if rage_processor and not args.stdout and not args.stderr:
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
    global_opts = parser.add_argument_group("global options")

    # TODO: We should probably rename this argument to --state-dir.
    # This directory contains materialized file state and the list of managed checkouts,
    # but doesn't really contain configuration.
    global_opts.add_argument(
        "--config-dir",
        help="Path to directory where EdenFS stores its internal state",
    )
    global_opts.add_argument(
        "--etc-eden-dir",
        help="Path to directory that holds the system configuration files",
    )
    global_opts.add_argument(
        "--home-dir", help="Path to directory where .edenrc config file is stored"
    )
    global_opts.add_argument("--checkout-dir", help=argparse.SUPPRESS)
    global_opts.add_argument(
        "--version", "-v", action="store_true", help="Print EdenFS version"
    )
    global_opts.add_argument(
        "--debug",
        action="store_true",
        help="Enable debug mode (more verbose logging, traceback, etc..)",
    )
    global_opts.add_argument(
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

    if args.config_dir == "":
        print_stderr("error: empty --config-dir path specified")
        return EX_USAGE

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
    # pyre-fixme[5]: Global expression must be annotated.
    asyncio_run = asyncio.run
except AttributeError:
    asyncio_run = asyncio.get_event_loop().run_until_complete


def main() -> int:
    # This is called hundreds of millions of times on unique hosts.
    # Increase how often it's sampled.
    usage.set_sample_rate(automation=10000)
    parser = create_parser()
    try:
        args = parser.parse_args()
    except SystemExit as ex:
        if ex.code == 0:
            return 0
        # For some reason argparse calls sys.exit(2) when it encounters a parse
        # error... This makes it hard for us to distinguish between edenfsctl
        # failing w/ exit code 2 and a parse error. Let's catch the parse
        # error and return a more appropriate exit code.
        return EX_USAGE

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
