#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import logging
import os
import pathlib
import shlex
import shutil
import subprocess
import tempfile
import typing
from pathlib import Path
from types import TracebackType
from typing import Any, Dict, List, Optional, Union, cast

import eden.thrift
from eden.cli import util
from facebook.eden.ttypes import MountState

from .find_executables import FindExe


class EdenFS(object):
    """Manages an instance of the EdenFS fuse server."""

    _eden_dir: Path

    def __init__(
        self,
        base_dir: Optional[Path] = None,
        eden_dir: Optional[Path] = None,
        etc_eden_dir: Optional[Path] = None,
        home_dir: Optional[Path] = None,
        logging_settings: Optional[Dict[str, str]] = None,
        extra_args: Optional[List[str]] = None,
        storage_engine: str = "memory",
    ) -> None:
        """
        Construct a new EdenFS object.

        By default, all of the state directories needed for the edenfs daemon will be
        created under the directory specified by base_dir.  If base_dir is not
        specified, a temporary directory will be created.  The temporary directory will
        be removed when cleanup() or __exit__() is called on the EdenFS object.

        Explicit locations for various state directories (eden_dir, etc_eden_dir,
        home_dir) can also be given, if desired.  For instance, this allows an EdenFS
        object to be created for an existing eden state directory.
        """
        if base_dir is None:
            self._base_dir = Path(tempfile.mkdtemp(prefix="eden_test."))
            self._cleanup_base_dir = True
        else:
            self._base_dir = base_dir
            self._cleanup_base_dir = False

        if eden_dir is None:
            self._eden_dir = self._base_dir / "eden"
            self._eden_dir.mkdir(exist_ok=True)
        else:
            self._eden_dir = eden_dir

        if etc_eden_dir is None:
            self._etc_eden_dir = self._base_dir / "etc_eden"
            self._etc_eden_dir.mkdir(exist_ok=True)
        else:
            self._etc_eden_dir = etc_eden_dir

        if home_dir is None:
            self._home_dir = self._base_dir / "home"
            self._home_dir.mkdir(exist_ok=True)
        else:
            self._home_dir = home_dir

        self._storage_engine = storage_engine
        self._logging_settings = logging_settings
        self._extra_args = extra_args

        self._process: Optional[subprocess.Popen] = None

    @property
    def eden_dir(self) -> Path:
        return self._eden_dir

    @property
    def etc_eden_dir(self) -> Path:
        return self._etc_eden_dir

    @property
    def home_dir(self) -> Path:
        return self._home_dir

    @property
    def user_rc_path(self) -> Path:
        return self._home_dir / ".edenrc"

    @property
    def system_rc_path(self) -> Path:
        return self._etc_eden_dir / "edenfs.rc"

    def __enter__(self) -> "EdenFS":
        return self

    def __exit__(
        self, exc_type: type, exc_value: BaseException, tb: TracebackType
    ) -> bool:
        self.cleanup()
        return False

    def cleanup(self) -> None:
        """Stop the instance and clean up its temporary directories"""
        self.kill()
        if self._cleanup_base_dir:
            shutil.rmtree(self._base_dir, ignore_errors=True)

    def kill(self) -> None:
        """Stops and unmounts this instance."""
        if self._process is None or self._process.returncode is not None:
            return
        self.shutdown()

    def get_thrift_client(self) -> eden.thrift.EdenClient:
        return eden.thrift.create_thrift_client(str(self._eden_dir))

    def run_cmd(
        self,
        command: str,
        *args: str,
        cwd: Optional[str] = None,
        capture_stderr: bool = False,
    ) -> str:
        """
        Run the specified eden command.

        Args: The eden command name and any arguments to pass to it.
        Usage example: run_cmd('mount', 'my_eden_client')
        Throws a subprocess.CalledProcessError if eden exits unsuccessfully.
        """
        cmd = self.get_eden_cli_args(command, *args)
        try:
            stderr = subprocess.STDOUT if capture_stderr else subprocess.PIPE
            env = dict(os.environ)
            # TODO(T37669726): Re-enable LSAN.
            env["LSAN_OPTIONS"] = "detect_leaks=0:verbosity=1:log_threads=1"
            completed_process = subprocess.run(
                cmd, stdout=subprocess.PIPE, stderr=stderr, check=True, cwd=cwd, env=env
            )
        except subprocess.CalledProcessError as ex:
            # Re-raise our own exception type so we can include the error
            # output.
            raise EdenCommandError(ex)
        return cast(str, completed_process.stdout.decode("utf-8"))

    def run_unchecked(
        self, command: str, *args: str, **kwargs: Any
    ) -> subprocess.CompletedProcess:
        """Run the specified eden command.

        Args: The eden command name and any arguments to pass to it.
        Usage example: run_cmd('mount', 'my_eden_client')
        Returns a subprocess.CompletedProcess object.
        """
        cmd = self.get_eden_cli_args(command, *args)
        return subprocess.run(cmd, **kwargs)

    def get_eden_cli_args(self, command: str, *args: str) -> List[str]:
        """Combines the specified eden command args with the appropriate
        defaults.

        Args:
            command: the eden command
            *args: extra string arguments to the command
        Returns:
            A list of arguments to run Eden that can be used with
            subprocess.Popen() or subprocess.check_call().
        """
        cmd = [
            typing.cast(str, FindExe.EDEN_CLI),  # T38947910
            "--config-dir",
            str(self._eden_dir),
            "--etc-eden-dir",
            str(self._etc_eden_dir),
            "--home-dir",
            str(self._home_dir),
        ]
        cmd.append(command)
        cmd.extend(args)
        return cmd

    def start(
        self,
        timeout: float = 60,
        takeover_from: Optional[int] = None,
        extra_args: Optional[List[str]] = None,
    ) -> None:
        """
        Run "eden daemon" to start the eden daemon.
        """
        use_gdb = False
        if os.environ.get("EDEN_GDB"):
            use_gdb = True
            # Starting up under GDB takes longer than normal.
            # Allow an extra 90 seconds (for some reason GDB can take a very
            # long time to load symbol information, particularly on dynamically
            # linked builds).
            timeout += 90

        takeover = takeover_from is not None
        self.spawn_nowait(gdb=use_gdb, takeover=takeover, extra_args=extra_args)

        assert self._process is not None
        util.wait_for_daemon_healthy(
            proc=self._process,
            config_dir=self._eden_dir,
            get_client=lambda: self.get_thrift_client(),
            timeout=timeout,
            exclude_pid=takeover_from,
        )

    def get_extra_daemon_args(self) -> List[str]:
        extra_daemon_args = [
            # Defaulting to 8 import processes is excessive when the test
            # framework runs tests on each CPU core.
            "--num_hg_import_threads",
            "2",
            "--local_storage_engine_unsafe",
            self._storage_engine,
            "--hgPath",
            typing.cast(str, FindExe.HG_REAL),  # T38947910
        ]

        if "SANDCASTLE" in os.environ:
            extra_daemon_args.append("--allowRoot")

        # Turn up the VLOG level for the fuse server so that errors are logged
        # with an explanation when they bubble up to RequestData::catchErrors
        if self._logging_settings:
            logging_arg = ",".join(
                "%s=%s" % (module, level)
                for module, level in sorted(self._logging_settings.items())
            )
            extra_daemon_args.extend(["--logging=" + logging_arg])
        if self._extra_args:
            extra_daemon_args.extend(self._extra_args)

        return extra_daemon_args

    def spawn_nowait(
        self,
        gdb: bool = False,
        takeover: bool = False,
        extra_args: Optional[List[str]] = None,
    ) -> None:
        """
        Start edenfs but do not wait for it to become healthy.
        """
        if self._process is not None:
            raise Exception("cannot start an already-running eden client")

        args = self.get_eden_cli_args(
            "daemon",
            "--daemon-binary",
            typing.cast(str, FindExe.EDEN_DAEMON),  # T38947910
            "--foreground",
        )

        extra_daemon_args = self.get_extra_daemon_args()
        if extra_args:
            extra_daemon_args.extend(extra_args)

        if takeover:
            args.append("--takeover")

        # If the EDEN_GDB environment variable is set, run eden inside gdb
        # so a developer can debug crashes
        if os.environ.get("EDEN_GDB"):
            gdb_exit_handler = (
                "python gdb.events.exited.connect("
                "lambda event: "
                'gdb.execute("quit") if getattr(event, "exit_code", None) == 0 '
                "else False"
                ")"
            )
            gdb_args = [
                # Register a handler to exit gdb if the program finishes
                # successfully.
                # Start the program immediately when gdb starts
                "-ex",
                gdb_exit_handler,
                # Start the program immediately when gdb starts
                "-ex",
                "run",
            ]
            args.append("--gdb")
            for arg in gdb_args:
                args.append("--gdb-arg=" + arg)

        if "EDEN_DAEMON_ARGS" in os.environ:
            args.extend(shlex.split(os.environ["EDEN_DAEMON_ARGS"]))

        full_args = args + ["--"] + extra_daemon_args
        logging.info(
            "Invoking eden daemon: %s", " ".join(shlex.quote(arg) for arg in full_args)
        )
        self._process = subprocess.Popen(full_args)

    def shutdown(self) -> None:
        """
        Run "eden shutdown" to stop the eden daemon.
        """
        assert self._process is not None

        self.run_cmd("shutdown", "-t", "30")
        return_code = self._process.wait()
        self._process = None
        if return_code != 0:
            raise Exception(
                "eden exited unsuccessfully with status {}".format(return_code)
            )

    def restart(self) -> None:
        self.shutdown()
        self.start()

    def get_pid_via_thrift(self):
        with self.get_thrift_client() as client:
            return client.getDaemonInfo().pid

    def graceful_restart(self, timeout: float = 30) -> None:
        assert self._process is not None
        # Get the process ID of the old edenfs process.
        # Note that this is not necessarily self._process.pid, since the eden
        # CLI may have spawned eden using sudo, and self._process may refer to
        # a sudo parent process.
        old_pid = self.get_pid_via_thrift()

        old_process = self._process
        self._process = None

        self.start(timeout=timeout, takeover_from=old_pid)

        # Check the return code from the old edenfs process
        return_code = old_process.wait()
        if return_code != 0:
            raise Exception(
                "eden exited unsuccessfully with status {}".format(return_code)
            )

    def stop_with_stale_mounts(self) -> None:
        """Stop edenfs without unmounting any of its mount points.
        This will leave the mount points mounted but no longer connected to a FUSE
        daemon.  Attempts to access files or directories inside the mount will fail with
        an ENOTCONN error after this.
        """
        assert self._process is not None

        cmd = [
            typing.cast(str, FindExe.TAKEOVER_TOOL),  # T38947910
            "--edenDir",
            str(self._eden_dir),
        ]
        subprocess.check_call(cmd)

        old_process = self._process
        self._process = None

        return_code = old_process.wait()
        if return_code != 0:
            raise Exception(
                f"eden exited unsuccessfully with status {return_code} "
                "after a fake takeover stop"
            )

    def add_repository(self, name: str, repo_path: str) -> None:
        """
        Run "eden repository" to define a repository configuration
        """
        self.run_cmd("repository", name, repo_path)

    def repository_cmd(self) -> str:
        """
        Executes "eden repository" to list the repositories,
        and returns the output as a string.
        """
        return self.run_cmd("repository")

    def list_cmd(self) -> Dict[str, Dict[str, Any]]:
        """
        Executes "eden list --json" to list the Eden checkouts and returns the result as
        a dictionary.
        """
        data = self.run_cmd("list", "--json")
        return json.loads(data)

    def list_cmd_simple(self) -> Dict[str, str]:
        """
        Executes "eden list --json" to list the Eden checkouts and returns the result in
        a simplified format that can be more easily used in test case assertions.

        The result is a dictionary of { mount_path: status }
        The status is a string containing one of the MountState names, or "NOT_RUNNING"
        if the mount is not running.  If the mount is known to the running edenfs
        instance but not listed in the configuration file, " (unconfigured)" will be
        appended to the status string.
        """
        results: Dict[str, str] = {}
        for path, mount_info in self.list_cmd().items():
            status_str = mount_info["state"]
            if not mount_info["configured"]:
                status_str += " (unconfigured)"
            results[path] = status_str

        return results

    def get_mount_state(
        self, mount: pathlib.Path, client: Optional[eden.thrift.EdenClient] = None
    ) -> Optional[MountState]:
        """
        Query edenfs over thrift for the state of the specified mount.

        Returns the MountState enum, or None if edenfs does not currently know about
        this mount path.
        """
        if client is None:
            with self.get_thrift_client() as client:
                return self.get_mount_state(mount, client)
        else:
            for entry in client.listMounts():
                entry_path = pathlib.Path(os.fsdecode(entry.mountPoint))
                if entry_path == mount:
                    return entry.state
            return None

    def clone(
        self, repo: str, path: Union[str, os.PathLike], allow_empty: bool = False
    ) -> None:
        """
        Run "eden clone"
        """
        params = ["clone", repo, str(path)]
        if allow_empty:
            params.append("--allow-empty-repo")
        self.run_cmd(*params)

    def remove(self, path: str) -> None:
        """
        Run "eden remove <path>"
        """
        self.run_cmd("remove", "--yes", path)

    def in_proc_mounts(self, mount_path: str) -> bool:
        """Gets all eden mounts found in /proc/mounts, and returns
        true if this eden instance exists in list.
        """
        with open("/proc/mounts", "r") as f:
            mounts = [
                line.split(" ")[1]
                for line in f.readlines()
                if line.split(" ")[0] == "edenfs"
            ]
        return mount_path in mounts

    def is_healthy(self) -> bool:
        """Executes `eden health` and returns True if it exited with code 0."""
        cmd_result = self.run_unchecked("health")
        return cmd_result.returncode == 0

    def set_log_level(self, category: str, level: str) -> None:
        with self.get_thrift_client() as client:
            client.debugSetLogLevel(category, level)

    def client_dir_for_mount(self, mount_path: pathlib.Path) -> pathlib.Path:
        client_link = mount_path / ".eden" / "client"
        return pathlib.Path(util.readlink_retry_estale(str(client_link)))

    def overlay_dir_for_mount(self, mount_path: pathlib.Path) -> pathlib.Path:
        return self.client_dir_for_mount(mount_path) / "local"

    def mount(self, mount_path: pathlib.Path) -> None:
        self.run_cmd("mount", "--", str(mount_path))

    def unmount(self, mount_path: pathlib.Path) -> None:
        self.run_cmd("unmount", "--", str(mount_path))


class EdenCommandError(subprocess.CalledProcessError):
    def __init__(self, ex: subprocess.CalledProcessError) -> None:
        super().__init__(ex.returncode, ex.cmd, output=ex.output, stderr=ex.stderr)

    def __str__(self) -> str:
        cmd_str = " ".join(shlex.quote(arg) for arg in self.cmd)
        return "eden command [%s] returned non-zero exit status %d\n" "stderr=%s" % (
            cmd_str,
            self.returncode,
            self.stderr,
        )


_can_run_eden: Optional[bool] = None
_can_run_sudo: Optional[bool] = None


def can_run_eden() -> bool:
    """
    Determine if we can run eden.

    This is used to determine if we should even attempt running the
    integration tests.
    """
    global _can_run_eden
    if _can_run_eden is None:
        _can_run_eden = _compute_can_run_eden()

    return _can_run_eden


def _compute_can_run_eden() -> bool:
    # FUSE must be available
    if not os.path.exists("/dev/fuse"):
        return False

    # We must be able to start eden as root.
    if os.geteuid() == 0:
        return True

    # The daemon must either be setuid root, or we must have sudo priviliges.
    # Typically for the tests the daemon process is not setuid root,
    # so check if we have are able to run things under sudo.
    return can_run_sudo()


def can_run_sudo() -> bool:
    global _can_run_sudo
    if _can_run_sudo is None:
        _can_run_sudo = _compute_can_run_sudo()

    return _can_run_sudo


def _compute_can_run_sudo() -> bool:
    cmd = ["/usr/bin/sudo", "-E", "/bin/true"]
    with open("/dev/null", "r") as dev_null:
        # Close stdout, stderr, and stdin, and call setsid() to make
        # sure we are detached from any controlling terminal.  This makes
        # sure that sudo can't prompt for a password if it needs one.
        # sudo will only succeed if it can run with no user input.
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            stdin=dev_null,
            preexec_fn=os.setsid,
        )
    process.communicate()
    return process.returncode == 0
