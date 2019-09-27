#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import errno
import json
import os
import signal
import subprocess
import sys
import typing
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple, Type

import eden.thrift
import thrift.transport
from eden.cli.util import EdenStartError, check_health_using_lockfile
from eden.thrift import EdenNotRunningError
from facebook.eden import EdenService
from facebook.eden.ttypes import GlobParams, MountInfo as ThriftMountInfo, MountState
from fb303_core.ttypes import fb303_status

from . import (
    buck,
    config as config_mod,
    debug as debug_mod,
    doctor as doctor_mod,
    filesystem,
    mtab,
    process_finder,
    redirect as redirect_mod,
    stats as stats_mod,
    subcmd as subcmd_mod,
    top as top_mod,
    trace as trace_mod,
    ui,
    util,
    version as version_mod,
)
from .cmd_util import get_eden_instance, require_checkout
from .config import EdenCheckout, EdenInstance
from .stats_print import format_size
from .subcmd import Subcmd
from .util import ShutdownError, print_stderr


if os.name == "nt":
    from . import winproc
else:
    from . import daemon, fsck as fsck_mod, rage as rage_mod


subcmd = subcmd_mod.Decorator()

# For a non-unix system (like Windows), we will define our own error codes.
try:
    EX_OK = os.EX_OK
    EX_USAGE = os.EX_USAGE
    EX_SOFTWARE = os.EX_SOFTWARE
    EX_OSFILE = os.EX_OSFILE
except AttributeError:  # On a non-unix system
    EX_OK = 0
    EX_USAGE = 64
    EX_SOFTWARE = 70
    EX_OSFILE = 72


def do_version(args: argparse.Namespace) -> int:
    instance = get_eden_instance(args)
    print("Installed: %s" % version_mod.get_installed_eden_rpm_version())

    try:
        rv = version_mod.get_running_eden_version(instance)
        print("Running:   %s" % rv)
        if rv.startswith("-") or rv.endswith("-"):
            print("(Dev version of eden seems to be running)")
    except EdenNotRunningError:
        print("Running:   Unknown (edenfs does not appear to be running)")
    return 0


@subcmd("version", "Print Eden's version information.")
class VersionCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        return do_version(args)


@subcmd("info", "Get details about a checkout")
class InfoCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "client", default=None, nargs="?", help="Name of the checkout"
        )

    def run(self, args: argparse.Namespace) -> int:
        instance, checkout, _rel_path = require_checkout(args, args.client)
        info = instance.get_client_info_from_checkout(checkout)
        json.dump(info, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0


@subcmd("du", "Show disk space usage for a checkout")
class DiskUsageCmd(Subcmd):
    isatty = sys.stdout.isatty()
    # Escape sequence to move the cursor left to the start of the line
    # and then clear to the end of that line.
    MOVE_TO_SOL_CLEAR_TO_EOL = "\r\x1b[K" if isatty else ""

    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "mounts", default=[], nargs="*", help="Names of the mount points"
        )

    def run(self, args: argparse.Namespace) -> int:
        mounts = args.mounts
        instance = None

        if len(mounts) == 0:
            instance, checkout, _rel_path = require_checkout(args, None)
            mounts = [str(checkout.path)]

        backing_repos = set()

        for mount in mounts:
            instance, checkout, _rel_path = require_checkout(args, mount)
            config = checkout.get_config()
            backing_repos.add(config.backing_repo)

            self.usage_for_mount(mount, args)

        for backing in backing_repos:
            self.backing_usage(backing)

        if instance:
            self.shared_usage(instance)

        return 0

    def du(self, path) -> int:
        output = subprocess.check_output(["du", "-skxc", path]).decode("utf-8")
        # the -k option reports 1024 byte blocks, so scale to bytes
        return int(output.split("\t")[0]) * 1024

    def underlined(self, message) -> None:
        underline = "-" * len(message)
        print(f"\n{message}\n{underline}\n")

    def usage_for_dir(self, label, path) -> None:
        if self.isatty:
            print(f"Computing usage of {label}...", end="", flush=True)
        usage = self.du(path)
        print(f"{self.MOVE_TO_SOL_CLEAR_TO_EOL}{label}: {format_size(usage)}")

    def backing_usage(self, backing_repo: Path) -> None:
        self.underlined(f"Backing repo: {backing_repo}")
        print(
            """CAUTION! You can lose work and break things by deleting data from the
backing repo directory!
"""
        )
        self.usage_for_dir("Overall usage", backing_repo)

        top_dirs = os.listdir(backing_repo)
        if ".hg" in top_dirs:
            hg_dir = backing_repo / ".hg"
            lfs_dir = hg_dir / "store" / "lfs"
            self.usage_for_dir("          .hg", hg_dir)
            if os.path.exists(lfs_dir):
                self.usage_for_dir("    LFS cache", hg_dir)
                print(
                    """
Mercurial currently has no safe way to manage the LFS cache.  See T35583239
"""
                )

            if len(top_dirs) > 1:
                print(
                    f"""
Working copy detected in backing repo.  This is not generally useful
and just takes up space.  You can make this a bare repo to reclaim
space by running:

    hg -R {backing_repo} checkout null
"""
                )

    def shared_usage(self, instance: EdenInstance) -> None:
        logs_dir = instance.state_dir / "logs"
        storage_dir = instance.state_dir / "storage"

        self.underlined("Data shared by all mounts in this Eden instance")
        self.usage_for_dir("Log files", logs_dir)
        self.usage_for_dir("Storage engine", storage_dir)
        print("\nRun `eden gc` to reduce the space used by the storage engine.")

    def usage_for_redirections(self, checkout: EdenCheckout) -> None:
        redirections = redirect_mod.get_effective_redirections(checkout, mtab.new())
        seen_paths = set()
        if len(redirections) > 0:
            print("Redirections:")

            for redir in redirections.values():
                target = redir.expand_target_abspath(checkout)
                seen_paths.add(target)
                self.usage_for_dir(f"    {redir.repo_path}", target)

            print(
                """
To reclaim space from buck-out directories, run `buck clean` from the
parent of the buck-out directory.
"""
            )

        # Deal with any legacy bind mounts that may have been made
        # obsolete by being migrated to redirections
        legacy_bind_mounts_dir = os.path.join(checkout.state_dir, "bind-mounts")
        legacy_dirs: List[str] = []
        for legacy in os.listdir(legacy_bind_mounts_dir):
            legacy_dir = os.path.join(legacy_bind_mounts_dir, legacy)
            if legacy_dir not in seen_paths:
                if not legacy_dirs:
                    legacy_dirs.append(legacy_dir)
                    print("Legacy bind mounts:")
                self.usage_for_dir(f"    {legacy_dir}", legacy_dir)

        if legacy_dirs:
            print(
                """
Legacy bind mount dirs listed above are unused and can be removed!
"""
            )

    def usage_for_mount(self, mount: str, args: argparse.Namespace) -> None:
        self.underlined(f"Mount: {mount}")

        instance, checkout, _rel_path = require_checkout(args, mount)
        info = instance.get_client_info_from_checkout(checkout)

        client_dir = info["client-dir"]
        overlay_dir = os.path.join(client_dir, "local")

        self.usage_for_dir("Materialized files", overlay_dir)
        with instance.get_thrift_client() as client:
            if self.isatty:
                print(f"Computing usage of ignored files...", end="", flush=True)
            scm_status = client.getScmStatus(
                bytes(checkout.path), True, info["snapshot"]
            )
            ignored_usage = 0
            buckd_usage = 0
            pyc_usage = 0
            for rel_path, _file_status in scm_status.entries.items():
                try:
                    st = os.lstat(os.path.join(bytes(checkout.path), rel_path))
                    if b".buckd" in rel_path:
                        buckd_usage += st.st_size
                    elif rel_path.endswith(b".pyc"):
                        pyc_usage += st.st_size
                    else:
                        ignored_usage += st.st_size
                except FileNotFoundError:
                    # Status can show files that were present in the overlay
                    # before a redirection was mounted over the top of it,
                    # which makes them inaccessible here.  Alternatively,
                    # someone may have raced with us and removed the file
                    # between the status call and our attempt to stat it.
                    # Just absorb the error here and ignore it.
                    pass
            print(f"{self.MOVE_TO_SOL_CLEAR_TO_EOL}", end="")
            print(
                f"  Ignored .buckd state: {format_size(buckd_usage)}    (see T37376291)"
            )
            print(f"    Ignored .pyc files: {format_size(pyc_usage)}")
            print(f"   Other Ignored files: {format_size(ignored_usage)}")

        print(
            """
Materialized files will be de-materialized once committed.
Use `hg status -i` to see Ignored files, `hg clean --all` to remove them
but be careful: it will remove untracked files as well!
It is best to use `eden redirect` or the `mkscratch` utility to relocate
files outside the repo rather than to ignore and clean them up.
"""
        )

        fsck_dir = os.path.join(client_dir, "fsck")
        if os.path.exists(fsck_dir):
            self.usage_for_dir("fsck recovered files", fsck_dir)
            print(
                f"""
A filesytem check recovered data and stored it at:
   {fsck_dir}
If you have recovered all that you need from it, you can remove that
directory to reclaim the disk space.
"""
            )

        self.usage_for_redirections(checkout)


@subcmd("status", "Check the health of the Eden service", aliases=["health"])
class StatusCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--timeout",
            type=float,
            default=15.0,
            help="Wait up to TIMEOUT seconds for the daemon to respond "
            "(default=%(default)s).",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        health_info = instance.check_health(timeout=args.timeout)
        if health_info.is_healthy():
            print("eden running normally (pid {})".format(health_info.pid))
            return 0

        print("edenfs not healthy: {}".format(health_info.detail))
        return 1


@subcmd("repository", "List all repositories")
class RepositoryCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "name", nargs="?", default=None, help="Name of the checkout to mount"
        )
        parser.add_argument(
            "path", nargs="?", default=None, help="Path to the repository to import"
        )
        parser.add_argument(
            "--with-buck",
            "-b",
            action="store_true",
            help="Checkout should create a bind mount for buck-out/.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        if args.name and args.path:
            repo = util.get_repo(args.path)
            if repo is None:
                print_stderr("%s does not look like a git or hg repository" % args.path)
                return 1
            try:
                instance.add_repository(
                    args.name,
                    repo_type=repo.type,
                    source=repo.source,
                    with_buck=args.with_buck,
                )
            except config_mod.UsageError as ex:
                print_stderr("error: {}", ex)
                return 1
        elif args.name or args.path:
            print_stderr("repository command called with incorrect arguments")
            return 1
        else:
            repo_list = instance.get_repository_list()
            for repo_name in sorted(repo_list):
                print(repo_name)
        return 0


class ListMountInfo(typing.NamedTuple):
    path: Path
    data_dir: Path
    state: Optional[MountState]
    configured: bool

    def to_json_dict(self) -> Dict[str, Any]:
        return {
            "data_dir": str(self.data_dir),
            # pyre-fixme[6]: Expected `MountState` for 1st param but got
            #  `Optional[MountState]`.
            "state": MountState._VALUES_TO_NAMES.get(self.state)
            if self.state is not None
            else "NOT_RUNNING",
            "configured": self.configured,
        }


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

        mounts = self.get_mounts(instance)
        out = ui.get_output()
        if args.json:
            self.print_mounts_json(out, mounts)
        else:
            self.print_mounts(out, mounts)
        return 0

    @classmethod
    def get_mounts(cls, instance: EdenInstance) -> Dict[Path, ListMountInfo]:
        try:
            with instance.get_thrift_client() as client:
                thrift_mounts = client.listMounts()
        except EdenNotRunningError:
            thrift_mounts = []

        config_mounts = instance.get_checkouts()
        return cls.combine_mount_info(thrift_mounts, config_mounts)

    @staticmethod
    def combine_mount_info(
        thrift_mounts: List[ThriftMountInfo], config_checkouts: List[EdenCheckout]
    ) -> Dict[Path, ListMountInfo]:
        mount_points: Dict[Path, ListMountInfo] = {}

        for thrift_mount in thrift_mounts:
            path = Path(os.fsdecode(thrift_mount.mountPoint))
            # Older versions of Eden did not report the state field.
            # If it is missing, set it to RUNNING.
            state = (
                thrift_mount.state
                if thrift_mount.state is not None
                else MountState.RUNNING
            )
            data_dir = Path(os.fsdecode(thrift_mount.edenClientPath))
            mount_points[path] = ListMountInfo(
                path=path, data_dir=data_dir, state=state, configured=False
            )

        # Add all mount points listed in the config that were not reported
        # in the thrift call.
        for checkout in config_checkouts:
            mount_info = mount_points.get(checkout.path, None)
            if mount_info is not None:
                mount_points[checkout.path] = mount_info._replace(configured=True)
            else:
                mount_points[checkout.path] = ListMountInfo(
                    path=checkout.path,
                    data_dir=checkout.state_dir,
                    state=None,
                    configured=True,
                )

        return mount_points

    @staticmethod
    def print_mounts_json(
        out: ui.Output, mount_points: Dict[Path, ListMountInfo]
    ) -> None:
        data = {
            str(mount.path): mount.to_json_dict() for mount in mount_points.values()
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
                # pyre-fixme[6]: Expected `MountState` for 1st param but got
                #  `Optional[MountState]`.
                state_name = MountState._VALUES_TO_NAMES[mount_info.state]
                state_str = f" ({state_name})"

            out.writeln(f"{path}{state_str}{suffix}")


class RepoError(Exception):
    pass


@subcmd("clone", "Create a clone of a specific repo and check it out")
class CloneCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "repo",
            help="The path to an existing repo to clone, or the name of a "
            "known repository configuration",
        )
        parser.add_argument("path", help="Path where the checkout should be mounted")
        parser.add_argument(
            "--rev", "-r", type=str, help="The initial revision to check out"
        )
        parser.add_argument(
            "--allow-empty-repo",
            "-e",
            action="store_true",
            help="Allow repo with null revision (no revisions)",
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

        # pyre-fixme[16]: `Namespace` has no attribute `path`.
        args.path = os.path.realpath(args.path)

        # Find the repository information
        try:
            repo, repo_type, repo_config = self._get_repo_info(
                instance, args.repo, args.rev
            )
        except RepoError as ex:
            print_stderr("error: {}", ex)
            return 1

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

        # Attempt to start the daemon if it is not already running.
        health_info = instance.check_health()
        if not health_info.is_healthy():
            print("edenfs daemon is not currently running.  Starting edenfs...")
            # Sometimes this returns a non-zero exit code if it does not finish
            # startup within the default timeout.
            if instance.should_use_experimental_systemd_mode():
                exit_code = daemon.start_systemd_service(
                    instance=instance,
                    daemon_binary=args.daemon_binary,
                    edenfs_args=args.edenfs_args,
                )
            else:
                exit_code = daemon.start_daemon(
                    instance, args.daemon_binary, args.edenfs_args
                )
            if exit_code != 0:
                return exit_code

        if repo_type is not None:
            print(f"Cloning new {repo_type} repository at {args.path}...")
        else:
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

    def _get_repo_info(
        self, instance: EdenInstance, repo_arg: str, rev: Optional[str]
    ) -> Tuple[util.Repo, Optional[str], config_mod.CheckoutConfig]:
        # Check to see if repo_arg points to an existing Eden mount
        checkout_config = instance.get_checkout_config_for_path(repo_arg)
        if checkout_config is not None:
            repo = util.get_repo(str(checkout_config.backing_repo))
            if repo is None:
                raise RepoError(
                    "eden mount is configured to use repository "
                    f"{checkout_config.backing_repo} but unable to find a "
                    "repository at that location"
                )
            return repo, None, checkout_config

        # Check to see if repo_arg looks like an existing repository path.
        repo = util.get_repo(repo_arg)
        if repo is None:
            # This is not a valid repository path.
            # Check to see if this is a repository config name instead.
            repo_config = instance.find_config_for_alias(repo_arg)
            if repo_config is None:
                raise RepoError(
                    f"{repo_arg!r} does not look like a valid "
                    "hg or git repository or a well-known "
                    "repository name"
                )

            repo = util.get_repo(str(repo_config.backing_repo))
            if repo is None:
                raise RepoError(
                    f"cloning {repo_arg} requires an existing "
                    f"repository to be present at "
                    f"{repo_config.backing_repo}"
                )

            return repo, repo_arg, repo_config

        # This is a valid repository path.
        # Try to identify what type of repository this is, so we can find
        # the proper configuration to use.
        project_id = util.get_project_id(repo, rev)

        project_config = None
        if project_id is not None:
            project_config = instance.find_config_for_alias(project_id)
        repo_type = project_id
        if project_config is None:
            repo_config = config_mod.CheckoutConfig(
                backing_repo=Path(repo.source),
                scm_type=repo.type,
                bind_mounts={},
                default_revision=config_mod.DEFAULT_REVISION[repo.type],
                redirections={},
            )
        else:
            # Build our own CheckoutConfig object, using our source repository
            # path and type, but the bind-mount and revision configuration from the
            # project configuration.
            repo_config = config_mod.CheckoutConfig(
                backing_repo=Path(repo.source),
                scm_type=repo.type,
                bind_mounts=project_config.bind_mounts,
                default_revision=project_config.default_revision,
                redirections={},
            )

        return repo, repo_type, repo_config


@subcmd("config", "Query Eden configuration")
class ConfigCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        instance.print_full_config(file=sys.stdout)
        return 0


@subcmd("doctor", "Debug and fix issues with Eden")
class DoctorCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--dry-run",
            "-n",
            action="store_true",
            help="Do not try to fix any issues: only report them.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        return doctor_mod.cure_what_ails_you(
            instance,
            args.dry_run,
            mount_table=mtab.new(),
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=process_finder.new(),
        )


@subcmd("top", "Monitor Eden accesses by process.")
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
        top = top_mod.Top()
        return top.start(args)


@subcmd("fsck", "Perform a filesystem check for Eden")
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
            help="The path to an Eden checkout to verify.",
        )

    def run(self, args: argparse.Namespace) -> int:
        if not args.path:
            return_codes = self.check_all(args)
            if not return_codes:
                print_stderr("No Eden checkouts are configured.  Nothing to check.")
                return 0
        else:
            return_codes = self.check_explicit_paths(args)

        return max(return_codes)

    def check_explicit_paths(self, args: argparse.Namespace) -> List[int]:
        return_codes: List[int] = []
        for path in args.path:
            # Check to see if this looks like an Eden checkout state directory.
            # If this looks like an Eden checkout state directory,
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

        with instance.get_thrift_client() as client:
            # TODO: unload
            print("Clearing and compacting local caches...", end="", flush=True)
            client.clearAndCompactLocalStore()
            print()
            # TODO: clear kernel caches

        return 0


@subcmd("chown", "Chown an entire eden repository")
class ChownCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("path", metavar="path", help="The Eden checkout to chown")
        parser.add_argument(
            "uid", metavar="uid", help="The uid or unix username to chown to"
        )
        parser.add_argument(
            "gid", metavar="gid", help="The gid or unix group name to chown to"
        )

    def resolve_uid(self, uid_str) -> int:
        try:
            return int(uid_str)
        except ValueError:
            import pwd

            return pwd.getpwnam(uid_str).pw_uid

    def resolve_gid(self, gid_str) -> int:
        try:
            return int(gid_str)
        except ValueError:
            import grp

            return grp.getgrnam(gid_str).gr_gid

    def run(self, args: argparse.Namespace) -> int:
        uid = self.resolve_uid(args.uid)
        gid = self.resolve_gid(args.gid)

        instance, checkout, _rel_path = require_checkout(args, args.path)
        with instance.get_thrift_client() as client:
            print("Chowning Eden repository...", end="", flush=True)
            client.chown(args.path, uid, gid)
            print("done")

        for redir in redirect_mod.get_effective_redirections(
            checkout, mtab.new()
        ).values():
            target = redir.expand_target_abspath(checkout)
            print(f"Chowning redirection: {redir.repo_path}...", end="", flush=True)
            subprocess.run(["sudo", "chown", "-R", f"{uid}:{gid}", str(target)])
            subprocess.run(
                ["sudo", "chown", f"{uid}:{gid}", str(checkout.path / redir.repo_path)]
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

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        for path in args.paths:
            try:
                exitcode = instance.mount(path)
                if exitcode:
                    return exitcode
            except EdenNotRunningError as ex:
                print_stderr("error: {}", ex)
                return 1
        return 0


@subcmd("remove", "Remove an eden checkout", aliases=["rm"])
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
            "paths", nargs="+", metavar="path", help="The Eden checkout(s) to remove"
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        configured_mounts = list(instance.get_mount_paths())

        # First translate the list of paths into canonical checkout paths
        # We also track a bool indicating if this checkout is currently mounted
        mounts: List[Tuple[str, bool]] = []
        for path in args.paths:
            try:
                mount_path = util.get_eden_mount_name(path)
                active = True
            except util.NotAnEdenMountError as ex:
                # This is not an active mount point.
                # Check for it by name in the config file anyway, in case it is
                # listed in the config file but not currently mounted.
                mount_path = os.path.realpath(path)
                if mount_path in configured_mounts:
                    active = False
                else:
                    print(f"error: {ex}")
                    return 1
                active = False
            except Exception as ex:
                print(f"error: cannot determine mount point for {path}: {ex}")
                return 1
            mounts.append((mount_path, active))

        # Warn the user since this operation permanently destroys data
        if args.prompt and sys.stdin.isatty():
            mounts_list = "\n  ".join(path for path, active in mounts)
            print(
                f"""\
Warning: this operation will permanently delete the following checkouts:
  {mounts_list}

Any uncommitted changes and shelves in this checkout will be lost forever."""
            )
            if not prompt_confirmation("Proceed?"):
                print("Not confirmed")
                return 2

        # Unmount + destroy everything
        exit_code = 0
        for mount, active in mounts:
            print(f"Removing {mount}...")
            if active:
                try:
                    stop_aux_processes_for_path(mount)
                    instance.unmount(mount)
                except Exception as ex:
                    print_stderr(f"error unmounting {mount}: {ex}")
                    exit_code = 1
                    # We intentionally fall through here and remove the mount point
                    # from the config file.  The most likely cause of failure is if
                    # edenfs times out performing the unmount.  We still want to go
                    # ahead delete the mount from the config in this case.

            try:
                instance.destroy_mount(mount)
            except Exception as ex:
                print_stderr(f"error deleting configuration for {mount}: {ex}")
                exit_code = 1
                # Continue around the loop removing any other mount points

        if exit_code == 0:
            print(f"Success")
        return exit_code


@subcmd("prefetch", "Prefetch content for matching file patterns")
class PrefetchCmd(Subcmd):
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
                # pyre-fixme[16]: `Namespace` has no attribute `PATTERN`.
                args.PATTERN += [pat.strip() for pat in f.readlines()]

        if os.name == "nt":
            if args.no_prefetch:
                # Not implementing --no-prefetch option right now because no
                # one is using it. Will come back it.
                raise NotImplementedError(
                    "Eden prefetch --no-prefetch is not implemented on Windows."
                    "You could use hg files to get the similar info from Mercurial"
                )

            cmd = [
                "hg",
                "prefetch",
                "-r",
                ".",
                "--cwd",
                str(checkout.path),
                "--",
            ] + args.PATTERN
            if args.silent:
                cmd += ["--quiet"]

            subprocess.check_call(cmd)
        else:
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


@subcmd("start", "Start the edenfs daemon", aliases=["daemon"])
class StartCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--daemon-binary", help="Path to the binary for the Eden daemon."
        )
        parser.add_argument(
            "--if-necessary",
            action="store_true",
            help="Only start edenfs if there are Eden checkouts configured.",
        )
        parser.add_argument(
            "--if-not-running",
            action="store_true",
            help="Exit successfully if EdenFS is already running.",
        )
        parser.add_argument(
            "--foreground",
            "-F",
            action="store_true",
            help="Run eden in the foreground, rather than daemonizing",
        )
        parser.add_argument(
            "--takeover",
            "-t",
            action="store_true",
            help="If an existing edenfs daemon is running, gracefully take "
            "over its mount points.",
        )
        parser.add_argument("--gdb", "-g", action="store_true", help="Run under gdb")
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
            help="Run eden under strace, and write strace output to FILE",
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

        if args.takeover and args.if_not_running:
            raise config_mod.UsageError(
                "the --takeover and --if-not-running flags cannot be combined"
            )

        instance = get_eden_instance(args)
        if args.if_necessary and not instance.get_mount_paths():
            print("No Eden mount points configured.")
            return 0

        # Check to see if edenfs is already running
        if not args.takeover:
            health_info = instance.check_health()
            if health_info.is_healthy():
                msg = f"EdenFS is already running (pid {health_info.pid})"
                if args.if_not_running:
                    print(msg)
                    return 0
                raise subcmd_mod.CmdError(msg)

        if instance.should_use_experimental_systemd_mode():
            if args.foreground or args.gdb:
                return self.start(args, instance)
            else:
                return self.start_using_systemd(args, instance)
        else:
            return self.start(args, instance)

    def start(self, args: argparse.Namespace, instance: EdenInstance) -> int:
        if os.name == "nt":
            winproc.start_process(instance, args.daemon_binary, args.edenfs_args)
        else:
            daemon.exec_daemon(
                instance,
                args.daemon_binary,
                args.edenfs_args,
                takeover=args.takeover,
                gdb=args.gdb,
                gdb_args=args.gdb_arg,
                strace_file=args.strace,
                foreground=args.foreground,
            )

        # Return success if no is exception is raised
        return 0

    def start_using_systemd(
        self, args: argparse.Namespace, instance: EdenInstance
    ) -> int:
        if args.strace:
            raise NotImplementedError(
                "TODO(T33122320): Implement 'eden start --strace'"
            )
        if args.takeover:
            raise NotImplementedError(
                "TODO(T33122320): Implement 'eden start --takeover'"
            )

        return daemon.start_systemd_service(
            instance=instance,
            daemon_binary=args.daemon_binary,
            edenfs_args=args.edenfs_args,
        )


def stop_aux_processes_for_path(repo_path: str) -> None:
    """Tear down processes that will hold onto file handles and prevent shutdown
    for a given mount point/repo"""
    buck.stop_buckd_for_repo(repo_path)


def stop_aux_processes(client: eden.thrift.EdenClient) -> None:
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


@subcmd("restart", "Restart the edenfs daemon")
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
            help="Completely shut down edenfs before restarting it.  This "
            "will unmount and remount the edenfs mounts, requiring processes "
            "using them to re-open any files and directories they are using.",
        )
        mode_group.add_argument(
            "--graceful",
            action="store_const",
            const=RESTART_MODE_GRACEFUL,
            dest="restart_type",
            help="Perform a graceful restart.  The new edenfs daemon will "
            "take over the existing edenfs mount points with minimal "
            "disruption to clients.  Open file handles will continue to work "
            "across the restart.",
        )
        mode_group.add_argument(
            "--force",
            action="store_const",
            const=RESTART_MODE_FORCE,
            dest="restart_type",
            help="Force a full restart, even if the existing edenfs daemon is "
            "still in the middle of starting or stopping.",
        )

        parser.add_argument(
            "--daemon-binary", help="Path to the binary for the Eden daemon."
        )
        parser.add_argument(
            "--shutdown-timeout",
            type=float,
            default=None,
            help="How long to wait for the old edenfs process to exit when "
            "performing a full restart.",
        )

    def run(self, args: argparse.Namespace) -> int:
        self.args = args
        if args.restart_type is None:
            # Default to a full restart for now
            # pyre-fixme[16]: `Namespace` has no attribute `restart_type`.
            args.restart_type = RESTART_MODE_FULL

        instance = get_eden_instance(self.args)

        health = instance.check_health()
        if health.is_healthy():
            assert health.pid is not None
            if self.args.restart_type == RESTART_MODE_GRACEFUL:
                return self._graceful_restart(instance)
            else:
                # pyre-fixme[6]: Expected `int` for 2nd param but got `Optional[int]`.
                return self._full_restart(instance, health.pid)
        elif health.pid is None:
            # The daemon is not running
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

            if self.args.restart_type != RESTART_MODE_FORCE:
                print(f"Use --force if you want to forcibly restart the current daemon")
                return 1
            # pyre-fixme[6]: Expected `int` for 2nd param but got `Optional[int]`.
            return self._force_restart(instance, health.pid, stop_timeout)

    def _graceful_restart(self, instance: EdenInstance) -> int:
        print("Performing a graceful restart...")
        if instance.should_use_experimental_systemd_mode():
            raise NotImplementedError(
                "TODO(T33122320): Implement 'eden restart --graceful'"
            )
        else:
            daemon.exec_daemon(
                instance, daemon_binary=self.args.daemon_binary, takeover=True
            )
            return 1  # never reached

    def _start(self, instance: EdenInstance) -> int:
        print("Eden is not currently running.  Starting it...")
        if instance.should_use_experimental_systemd_mode():
            return daemon.start_systemd_service(
                instance=instance, daemon_binary=self.args.daemon_binary
            )
        else:
            daemon.exec_daemon(instance, daemon_binary=self.args.daemon_binary)
        return 1  # never reached

    def _full_restart(self, instance: EdenInstance, old_pid: int) -> int:
        print(
            """\
About to perform a full restart of Eden.
Note: this will temporarily disrupt access to your Eden-managed repositories.
Any programs using files or directories inside the Eden mounts will need to
re-open these files after Eden is restarted.
"""
        )
        if self.args.restart_type != RESTART_MODE_FORCE and sys.stdin.isatty():
            if not prompt_confirmation("Proceed?"):
                print("Not confirmed.")
                return 1

        self._do_stop(instance, old_pid, timeout=15)
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
        with instance.get_thrift_client() as client:
            try:
                stop_aux_processes(client)
            except Exception:
                pass
            try:
                client.initiateShutdown(
                    f"`eden restart --force` requested by pid={os.getpid()} "
                    f"uid={os.getuid()}"
                )
            except Exception:
                print("Sending SIGTERM...")
                os.kill(pid, signal.SIGTERM)
        self._wait_for_stop(instance, pid, timeout)

    def _finish_restart(self, instance: EdenInstance) -> int:
        if instance.should_use_experimental_systemd_mode():
            exit_code = daemon.start_systemd_service(
                instance=instance, daemon_binary=self.args.daemon_binary
            )
        else:
            exit_code = daemon.start_daemon(
                instance, daemon_binary=self.args.daemon_binary
            )
        if exit_code != 0:
            print("Failed to start edenfs!", file=sys.stderr)
            return exit_code

        print(
            """\

Successfully restarted edenfs.
Note: any programs running inside of an Eden-managed directory will need to cd
out of and back into the repository to pick up the new working directory state.
If you see "Transport endpoint not connected" errors from any program this
means it is still attempting to use the old mount point from the previous Eden
process."""
        )
        return 0


@subcmd("rage", "Gather diagnostic information about eden")
class RageCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--stdout",
            action="store_true",
            help="Print the rage report to stdout: ignore reporter.",
        )

    def run(self, args: argparse.Namespace) -> int:
        instance = get_eden_instance(args)
        rage_processor = instance.get_config_value("rage.reporter", default="")

        proc: Optional[subprocess.Popen] = None
        if rage_processor and not args.stdout:
            proc = subprocess.Popen(["sh", "-c", rage_processor], stdin=subprocess.PIPE)
            sink = proc.stdin
        else:
            proc = None
            sink = sys.stdout.buffer

        rage_mod.print_diagnostic_info(instance, sink)
        if proc:
            sink.close()
            proc.wait()
        return 0


SHUTDOWN_EXIT_CODE_NORMAL = 0
SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN = 0
SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR = 2
SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL = 3
SHUTDOWN_EXIT_CODE_ERROR = 4


@subcmd("stop", "Shutdown the daemon", aliases=["shutdown"])
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
            help="Forcibly kill edenfs with SIGKILL, rather than attempting to "
            "shut down cleanly.  Not that Eden will normally need to re-scan its "
            "data the next time it starts after an unclean shutdown.",
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
                with instance.get_thrift_client() as client:
                    client.set_timeout(self.__thrift_timeout(args))
                    pid = client.getPid()
                    stop_aux_processes(client)
                    # Ask the client to shutdown
                    print(f"Stopping edenfs daemon (pid {pid})...")
                    client.initiateShutdown(
                        f"`eden stop` requested by pid={os.getpid()} uid={os.getuid()}"
                    )
            except thrift.transport.TTransport.TTransportException as e:
                print_stderr(f"warning: edenfs is not responding: {e}")
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
                print_stderr("Terminated edenfs with SIGKILL.")
                return SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
        except ShutdownError as ex:
            print_stderr("Error: " + str(ex))
            return SHUTDOWN_EXIT_CODE_ERROR

    def _kill(self, instance: EdenInstance, args: argparse.Namespace) -> int:
        # Get the pid from the lock file rather than trying to query it over thrift.
        # If the user is forcibly trying to kill Eden we assume something is probably
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

    def __thrift_timeout(self, args: argparse.Namespace) -> Optional[float]:
        if args.timeout == 0:
            return None
        else:
            return args.timeout


def create_parser() -> argparse.ArgumentParser:
    """Returns a parser"""
    parser = argparse.ArgumentParser(description="Manage Eden checkouts.")
    # TODO: We should probably rename this argument to --state-dir.
    # This directory contains materialized file state and the list of managed checkouts,
    # but doesn't really contain configuration.
    parser.add_argument(
        "--config-dir",
        help="The path to the directory where edenfs stores its internal state.",
    )
    parser.add_argument(
        "--etc-eden-dir",
        help="Path to directory that holds the system configuration files.",
    )
    parser.add_argument(
        "--home-dir", help="Path to directory where .edenrc config file is stored."
    )
    parser.add_argument(
        "--version", "-v", action="store_true", help="Print eden version."
    )

    subcmd_add_list: List[Type[Subcmd]] = [
        subcmd_mod.HelpCmd,
        stats_mod.StatsCmd,
        trace_mod.TraceCmd,
        redirect_mod.RedirectCmd,
    ]

    subcmd_add_list.append(debug_mod.DebugCmd)

    subcmd_mod.add_subcommands(parser, subcmd.commands + subcmd_add_list)

    return parser


def prompt_confirmation(prompt: str) -> bool:
    # Import readline lazily here because it conflicts with ncurses's resize support.
    # https://bugs.python.org/issue2675
    try:
        import readline  # noqa: F401 Importing readline improves the behavior of input()
    except ImportError:
        # We don't strictly need readline
        pass

    prompt_str = f"{prompt} [y/N] "
    while True:
        response = input(prompt_str)
        value = response.lower()
        if value in ("y", "yes"):
            return True
        if value in ("", "n", "no"):
            return False
        print('Please enter "yes" or "no"')


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
Your current working directory appears to be a stale Eden
mount point from a previous Eden daemon instance.
Please run "cd / && cd -" to update your shell's working directory."""
    if not can_continue:
        print(f"Error: {msg}", file=sys.stderr)
        return EX_OSFILE

    print(f"Warning: {msg}", file=sys.stderr)
    doctor_mod.working_directory_was_stale = True
    return None


def main() -> int:
    # Before doing anything else check that the current working directory is valid.
    # This helps catch the case where a user is trying to run the Eden CLI inside
    # a stale eden mount point.
    stale_return_code = check_for_stale_working_directory()
    if stale_return_code is not None:
        return stale_return_code

    parser = create_parser()
    args = parser.parse_args()
    if args.version:
        return do_version(args)
    if getattr(args, "func", None) is None:
        parser.print_help()
        return EX_OK
    try:
        return_code: int = args.func(args)
    except subcmd_mod.CmdError as ex:
        print(f"error: {ex}", file=sys.stderr)
        return EX_SOFTWARE
    except config_mod.UsageError as ex:
        print(f"error: {ex}", file=sys.stderr)
        return EX_USAGE
    return return_code


if __name__ == "__main__":
    retcode = main()
    sys.exit(retcode)
