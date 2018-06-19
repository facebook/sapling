#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import argparse
import errno
import glob
import json
import os
import readline  # noqa: F401 Importing readline improves the behavior of input()
import signal
import subprocess
import sys
import typing
from typing import Any, List, Optional, Set, Tuple

import eden.thrift
from eden.thrift import EdenNotRunningError
from facebook.eden import EdenService
from facebook.eden.ttypes import GlobParams

from . import (
    buck,
    config as config_mod,
    daemon,
    debug as debug_mod,
    doctor as doctor_mod,
    mtab,
    rage as rage_mod,
    stats as stats_mod,
    subcmd as subcmd_mod,
    util,
    version as version_mod,
)
from .cmd_util import create_config
from .subcmd import Subcmd
from .util import ShutdownError, print_stderr


subcmd = subcmd_mod.Decorator()


def infer_client_from_cwd(config: config_mod.Config, clientname: str) -> str:
    if clientname:
        return clientname

    all_clients = config.get_all_client_config_info()
    path = normalize_path_arg(os.getcwd())

    # Keep going while we're not in the root, as dirname(/) is /
    # and we can keep iterating forever.
    while len(path) > 1:
        for _, info in all_clients.items():
            if info["mount"] == path:
                return typing.cast(str, info["mount"])
        path = os.path.dirname(path)

    print_stderr("cwd is not an eden mount point, and no checkout name was specified.")
    sys.exit(2)


def do_version(args: argparse.Namespace) -> int:
    config = create_config(args)
    print("Installed: %s" % version_mod.get_installed_eden_rpm_version())
    import eden

    try:
        rv = version_mod.get_running_eden_version(config)
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
        config = create_config(args)
        info = config.get_client_info(infer_client_from_cwd(config, args.client))
        json.dump(info, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0


@subcmd("status", "Check the health of the Eden service", aliases=["health"])
class StatusCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)
        health_info = config.check_health()
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
        config = create_config(args)
        if args.name and args.path:
            repo = util.get_repo(args.path)
            if repo is None:
                print_stderr("%s does not look like a git or hg repository" % args.path)
                return 1
            try:
                config.add_repository(
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
            repo_list = config.get_repository_list()
            for repo_name in sorted(repo_list):
                print(repo_name)
        return 0


@subcmd("list", "List available checkouts")
class ListCmd(Subcmd):
    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)

        try:
            with config.get_thrift_client() as client:
                active_mount_points: Set[Optional[str]] = {
                    mount.mountPoint for mount in client.listMounts()
                }
        except EdenNotRunningError:
            active_mount_points = set()

        config_mount_points = set(config.get_mount_paths())

        for path in sorted(active_mount_points | config_mount_points):
            assert path is not None
            if path not in config_mount_points:
                print(path + " (unconfigured)")
            elif path in active_mount_points:
                print(path + " (active)")
            else:
                print(path)
        return 0


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
        config = create_config(args)

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

        args.path = os.path.realpath(args.path)

        # Find the repository information
        try:
            repo, repo_type, repo_config = self._get_repo_info(
                config, args.repo, args.rev
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
        health_info = config.check_health()
        if not health_info.is_healthy():
            print("edenfs daemon is not currently running.  Starting edenfs...")
            # Sometimes this returns a non-zero exit code if it does not finish
            # startup within the default timeout.
            exit_code = daemon.start_daemon(
                config, args.daemon_binary, args.edenfs_args
            )
            if exit_code != 0:
                return exit_code

        if repo_type is not None:
            print(f"Cloning new {repo_type} repository at {args.path}...")
        else:
            print(f"Cloning new repository at {args.path}...")

        try:
            config.clone(repo_config, args.path, commit)
            print(f"Success.  Checked out commit {commit:.8}")
            # In the future it would probably be nice to fork a background
            # process here to prefetch files that we think the user is likely
            # to want to access soon.
            return 0
        except Exception as ex:
            print_stderr("error: {}", ex)
            return 1

    def _get_repo_info(
        self, config: config_mod.Config, repo_arg: str, rev: Optional[str]
    ) -> Tuple[util.Repo, Optional[str], config_mod.ClientConfig]:
        # Check to see if repo_arg points to an existing Eden mount
        eden_config = config.get_client_config_for_path(repo_arg)
        if eden_config is not None:
            repo = util.get_repo(eden_config.path)
            if repo is None:
                raise RepoError(
                    "eden mount is configured to use repository "
                    f"{eden_config.path} but unable to find a "
                    "repository at that location"
                )
            return repo, None, eden_config

        # Check to see if repo_arg looks like an existing repository path.
        repo = util.get_repo(repo_arg)
        if repo is None:
            # This is not a valid repository path.
            # Check to see if this is a repository config name instead.
            repo_config = config.find_config_for_alias(repo_arg)
            if repo_config is None:
                raise RepoError(
                    f"{repo_arg!r} does not look like a valid "
                    "hg or git repository or a well-known "
                    "repository name"
                )

            repo = util.get_repo(repo_config.path)
            if repo is None:
                raise RepoError(
                    f"cloning {repo_arg} requires an existing "
                    f"repository to be present at "
                    f"{repo_config.path}"
                )

            return repo, repo_arg, repo_config

        # This is a valid repository path.
        # Try to identify what type of repository this is, so we can find
        # the proper configuration to use.
        project_id = util.get_project_id(repo, rev)

        project_config = None
        if project_id is not None:
            project_config = config.find_config_for_alias(project_id)
        repo_type = project_id
        if project_config is None:
            repo_config = config_mod.ClientConfig(
                path=repo.source,
                scm_type=repo.type,
                hooks_path=config.get_default_hooks_path(),
                bind_mounts={},
                default_revision=config_mod.DEFAULT_REVISION[repo.type],
            )
        else:
            # Build our own ClientConfig object, using our source repository
            # path and type, but the hooks, bind-mount, and revision
            # configuration from the project configuration.
            repo_config = config_mod.ClientConfig(
                path=repo.source,
                scm_type=repo.type,
                hooks_path=project_config.hooks_path,
                bind_mounts=project_config.bind_mounts,
                default_revision=project_config.default_revision,
            )

        return repo, repo_type, repo_config


@subcmd("config", "Query Eden configuration")
class ConfigCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--get", help="Name of value to get")

    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)
        if args.get:
            try:
                print(config.get_config_value(args.get))
            except (KeyError, ValueError):
                # mirrors `git config --get invalid`; just exit with code 1
                return 1
        else:
            config.print_full_config()
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
        config = create_config(args)
        return doctor_mod.cure_what_ails_you(
            config, args.dry_run, out=sys.stdout, mount_table=mtab.LinuxMountTable()
        )


@subcmd(
    "mount",
    (
        "Remount an existing checkout (for instance, after it was "
        'unmounted with "unmount")'
    ),
)
class MountCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "paths", nargs="+", metavar="path", help="The checkout mount path"
        )

    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)
        for path in args.paths:
            try:
                exitcode = config.mount(path)
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
        config = create_config(args)

        # First translate the list of paths into mount point names
        mounts = []
        for path in args.paths:
            try:
                mount_path = util.get_eden_mount_name(path)
            except util.NotAnEdenMountError as ex:
                print(f"error: {ex}")
                return 1
            except Exception as ex:
                print(f"error: cannot determine moint point for {path}: {ex}")
                return 1
            mounts.append(mount_path)

        # Warn the user since this operation permanently destroys data
        if args.prompt and sys.stdin.isatty():
            mounts_list = "\n  ".join(mounts)
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
        for mount in mounts:
            print(f"Removing {mount}...")
            try:
                stop_aux_processes_for_path(mount)
                config.unmount(mount, delete_config=True)
            except EdenService.EdenError as ex:
                print_stderr("error: {}", ex)
                return 1

        print(f"Success")
        return 0


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
            "PATTERN", nargs="+", help="Filename patterns to match via fnmatch"
        )

    def _repo_root(self, path: str) -> Optional[str]:
        try:
            return util.get_eden_mount_name(path)
        except Exception:
            # Likely no .eden dir there, so probably not an eden repo
            return None

    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)

        if args.repo:
            repo_root = self._repo_root(args.repo)
            if not repo_root:
                print(f"{args.repo} does not appear to be an eden repo")
                return 1
            if repo_root != os.path.realpath(args.repo):
                print(f"{args.repo} is not the root of an eden repo")
                return 1
        else:
            repo_root = self._repo_root(os.getcwd())
            if not repo_root:
                print("current directory does not appear to be an eden repo")
                return 1

        if args.pattern_file is not None:
            with open(args.pattern_file) as f:
                args.PATTERN += [pat.strip() for pat in f.readlines()]

        with config.get_thrift_client() as client:
            result = client.globFiles(
                GlobParams(
                    mountPoint=repo_root,
                    globs=args.PATTERN,
                    includeDotfiles=False,
                    prefetchFiles=not args.no_prefetch,
                    suppressFileList=args.silent,
                )
            )
            if not args.silent:
                for name in result.matchingFiles:
                    print(name)

        return 0


@subcmd("unmount", "Unmount a specific checkout")
class UnmountCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--destroy",
            action="store_true",
            help="Permanently delete all state associated with the checkout.",
        )
        parser.add_argument(
            "paths",
            nargs="+",
            metavar="path",
            help="Path where checkout should be unmounted from",
        )

    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)
        for path in args.paths:
            path = normalize_path_arg(path)
            try:
                config.unmount(path, delete_config=args.destroy)
            except EdenService.EdenError as ex:
                print_stderr("error: {}", ex)
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
        config = create_config(args)

        if args.if_necessary and not config.get_mount_paths():
            print("No Eden mount points configured.")
            return 0

        return daemon.start_daemon(
            config,
            args.daemon_binary,
            args.edenfs_args,
            takeover=args.takeover,
            gdb=args.gdb,
            gdb_args=args.gdb_arg,
            strace_file=args.strace,
            foreground=args.foreground,
        )


def stop_aux_processes_for_path(repo_path: str) -> None:
    """Tear down processes that will hold onto file handles and prevent shutdown
    for a given mount point/repo"""
    buck.stop_buckd_for_repo(repo_path)


def stop_aux_processes(client: eden.thrift.EdenClient) -> None:
    """Tear down processes that will hold onto file handles and prevent shutdown
    for all mounts"""

    active_mount_points: Set[Optional[str]] = {
        mount.mountPoint for mount in client.listMounts()
    }

    for repo in active_mount_points:
        if repo is not None:
            stop_aux_processes_for_path(repo)

    # TODO: intelligently stop nuclide-server associated with eden
    # print('Stopping nuclide-server...')
    # subprocess.run(['pkill', '-f', 'nuclide-main'])


RESTART_MODE_FULL = "full"
RESTART_MODE_GRACEFUL = "graceful"


@subcmd("restart", "Restart the edenfs daemon")
class RestartCmd(Subcmd):
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

        parser.add_argument(
            "--shutdown-timeout",
            type=float,
            default=30,
            help="How long to wait for the old edenfs process to exit when "
            "performing a full restart.",
        )

    def run(self, args: argparse.Namespace) -> int:
        self.args = args

        if self.args.restart_type is None:
            # Default to a full restart for now
            self.args.restart_type = RESTART_MODE_FULL

        self.config = create_config(args)
        stopping = False
        pid = None
        try:
            with self.config.get_thrift_client() as client:
                pid = client.getPid()
                if self.args.restart_type == RESTART_MODE_FULL:
                    stop_aux_processes(client)
                    # Ask the old edenfs daemon to shutdown
                    self.msg("Stopping the existing edenfs daemon (pid {})...", pid)
                    client.initiateShutdown(
                        f"`eden restart` requested by pid={os.getpid()} "
                        f"uid={os.getuid()}"
                    )
                    stopping = True
        except EdenNotRunningError:
            pass

        if stopping:
            assert isinstance(pid, int)
            daemon.wait_for_shutdown(
                self.config, pid, timeout=self.args.shutdown_timeout
            )
            self._start()
        elif pid is None:
            self.msg("edenfs is not currently running.")
            self._start()
        else:
            self._graceful_start()
        return 0

    def msg(self, msg: str, *args: Any, **kwargs: Any) -> None:
        if args or kwargs:
            msg = msg.format(*args, **kwargs)
        print(msg)

    def _start(self) -> None:
        self.msg("Starting edenfs...")
        daemon.start_daemon(self.config)

    def _graceful_start(self) -> None:
        self.msg("Performing a graceful restart...")
        daemon.start_daemon(self.config, takeover=True)


@subcmd("rage", "Prints diagnostic information about eden")
class RageCmd(Subcmd):
    def setup_parser(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--stdout",
            action="store_true",
            help="Print the rage report to stdout: ignore reporter.",
        )

    def run(self, args: argparse.Namespace) -> int:
        rage_processor = None
        config = create_config(args)
        try:
            rage_processor = config.get_config_value("rage.reporter")
        except KeyError:
            pass

        proc: Optional[subprocess.Popen] = None
        if rage_processor and not args.stdout:
            proc = subprocess.Popen(["sh", "-c", rage_processor], stdin=subprocess.PIPE)
            sink = proc.stdin
        else:
            proc = None
            sink = sys.stdout.buffer

        rage_mod.print_diagnostic_info(config, sink)
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

    def run(self, args: argparse.Namespace) -> int:
        config = create_config(args)
        try:
            with config.get_thrift_client() as client:
                pid = client.getPid()
                stop_aux_processes(client)
                # Ask the client to shutdown
                print(f"Stopping edenfs daemon (pid {pid})...")
                client.initiateShutdown(
                    f"`eden stop` requested by pid={os.getpid()} uid={os.getuid()}"
                )
        except EdenNotRunningError:
            print_stderr("error: edenfs is not running")
            return SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR

        if args.timeout == 0:
            print_stderr("Sent async shutdown request to edenfs.")
            return SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN

        try:
            if daemon.wait_for_shutdown(config, pid, timeout=args.timeout):
                print_stderr("edenfs exited cleanly.")
                return SHUTDOWN_EXIT_CODE_NORMAL
            else:
                print_stderr("Terminated edenfs with SIGKILL.")
                return SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
        except ShutdownError as ex:
            print_stderr("Error: " + str(ex))
            return SHUTDOWN_EXIT_CODE_ERROR


def create_parser() -> argparse.ArgumentParser:
    """Returns a parser"""
    parser = argparse.ArgumentParser(description="Manage Eden checkouts.")
    parser.add_argument(
        "--config-dir", help="Path to directory where internal data is stored."
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

    subcmd_mod.add_subcommands(
        parser,
        subcmd.commands + [debug_mod.DebugCmd, subcmd_mod.HelpCmd, stats_mod.StatsCmd],
    )

    return parser


def prompt_confirmation(prompt: str) -> bool:
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


def main() -> int:
    parser = create_parser()
    args = parser.parse_args()
    if args.version:
        return do_version(args)
    if getattr(args, "func", None) is None:
        parser.print_help()
        return 0
    return_code: int = args.func(args)
    return return_code


if __name__ == "__main__":
    retcode = main()
    sys.exit(retcode)
