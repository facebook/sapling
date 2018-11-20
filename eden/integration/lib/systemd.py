#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import contextlib
import errno
import logging
import os
import os.path
import pathlib
import pty
import re
import select
import subprocess
import sys
import tempfile
import threading
import types
import typing

from .find_executables import FindExe
from .linux import LinuxCgroup, ProcessID
from .temporary_directory import create_tmp_dir


logger = logging.getLogger(__name__)

SystemdUnitName = str


class SystemdUserServiceManager:
    """A running 'systemd --user' process manageable using 'systemctl --user'."""

    def __init__(self, xdg_runtime_dir: pathlib.Path) -> None:
        super().__init__()
        self.__xdg_runtime_dir = xdg_runtime_dir

    @property
    def xdg_runtime_dir(self) -> pathlib.Path:
        return self.__xdg_runtime_dir

    def is_alive(self) -> bool:
        result = self._systemctl.run(
            ["--user", "show-environment"],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        if result.returncode == 0:
            return True
        if result.returncode == 1:
            logger.warning(f'{self} is not alive: {result.stdout.decode("utf-8")}')
            return False
        result.check_returncode()
        return False

    def enable_runtime_unit_from_file(self, unit_file: pathlib.Path) -> None:
        self._systemctl.check_call(["enable", "--runtime", "--", unit_file])
        self._systemctl.check_call(["daemon-reload"])
        self.sanity_check_enabled_unit(unit_file=unit_file)

    def sanity_check_enabled_unit(self, unit_file: pathlib.Path) -> None:
        unit_name = unit_file.name
        if "@" in unit_name:
            unit_name = unit_name.replace("@", "@testinstance")
        self.sanity_check_enabled_unit_fragment(
            unit_name=unit_name, expected_unit_file=unit_file
        )
        self.sanity_check_enabled_unit_sources(
            unit_name=unit_name, expected_unit_file=unit_file
        )

    def sanity_check_enabled_unit_fragment(
        self, unit_name: SystemdUnitName, expected_unit_file: pathlib.Path
    ) -> None:
        service = SystemdService(unit_name=unit_name, systemd=self)
        actual_unit_file = service.query_fragment_path()
        if actual_unit_file != expected_unit_file:
            raise Exception(
                "Enabled unit's FragmentPath does not match unit file\n"
                "Expected: {repr(expected_unit_file)}\n"
                "Actual:   {repr(actual_unit_file)}"
            )

    def sanity_check_enabled_unit_sources(
        self, unit_name: SystemdUnitName, expected_unit_file: pathlib.Path
    ) -> None:
        actual_unit_sources = self._systemctl.check_output(["cat", "--", unit_name])

        expected_unit_sources = b""
        for file in [expected_unit_file]:
            expected_unit_sources += b"# " + bytes(file) + b"\n"
            expected_unit_sources += file.read_bytes()

        if actual_unit_sources != expected_unit_sources:
            raise Exception(
                "Enabled unit does not match unit file\n"
                "Expected: {repr(expected_unit_sources)}\n"
                "Actual:   {repr(actual_unit_sources)}"
            )

    def systemd_run(
        self,
        command: typing.Sequence[str],
        properties: typing.Mapping[str, str],
        extra_env: typing.Mapping[str, str],
    ) -> "SystemdService":
        systemd_run_command = ["systemd-run", "--user"]
        for name, value in properties.items():
            systemd_run_command.extend(("--property", f"{name}={value}"))
        for name, value in extra_env.items():
            systemd_run_command.extend(("--setenv", f"{name}={value}"))
        systemd_run_command.append("--")
        systemd_run_command.extend(command)

        output = subprocess.check_output(
            systemd_run_command, env=self.env, stderr=subprocess.STDOUT
        )
        match = re.match(
            r"^Running as unit: (?P<unit>.*)$",
            output.decode("utf-8"),
            flags=re.MULTILINE,
        )
        if match is None:
            raise Exception("Failed to parse unit from command output")
        return self.get_service(match.group("unit"))

    def get_active_unit_names(self) -> typing.List[SystemdUnitName]:
        def parse_line(line: str) -> SystemdUnitName:
            parts = re.split(r" +", line)
            return parts[0]

        stdout = self._systemctl.check_output(
            [
                "list-units",
                "--all",
                "--full",
                "--no-legend",
                "--no-pager",
                "--plain",
                "--state=active",
            ]
        )
        return [parse_line(line) for line in stdout.decode("utf-8").splitlines()]

    def get_unit_paths(self) -> typing.List[pathlib.Path]:
        stdout = subprocess.check_output(
            ["systemd-analyze", "--user", "unit-paths"], env=self.env
        )
        return [pathlib.Path(line) for line in stdout.decode("utf-8").splitlines()]

    def get_service(self, unit_name: SystemdUnitName) -> "SystemdService":
        return SystemdService(unit_name=unit_name, systemd=self)

    @property
    def env(self) -> typing.Dict[str, str]:
        env = dict(os.environ)
        env.update(self.extra_env)
        return env

    @property
    def extra_env(self) -> typing.Dict[str, str]:
        return {
            "DBUS_SESSION_BUS_ADDRESS": "",
            "XDG_RUNTIME_DIR": str(self.xdg_runtime_dir),
        }

    @property
    def _systemctl(self) -> "_SystemctlCLI":
        return _SystemctlCLI(env=self.env)

    def __str__(self) -> str:
        return f"systemd ({self.xdg_runtime_dir})"

    def __repr__(self) -> str:
        return (
            f"SystemdUserServiceManager(xdg_runtime_dir={repr(self.xdg_runtime_dir)})"
        )


class SystemdService:
    def __init__(
        self, unit_name: SystemdUnitName, systemd: SystemdUserServiceManager
    ) -> None:
        super().__init__()
        self.__systemd = systemd
        self.__unit_name = unit_name

    @property
    def unit_name(self) -> SystemdUnitName:
        return self.__unit_name

    def start(self) -> None:
        self.__systemctl.check_call(["start", "--", self.unit_name])

    def stop(self) -> None:
        self.__systemctl.check_call(["stop", "--", self.unit_name])

    def restart(self) -> None:
        self.__systemctl.check_call(["restart", "--", self.unit_name])

    def query_active_state(self) -> str:
        return self.__query_property("ActiveState").decode("utf-8")

    def query_sub_state(self) -> str:
        return self.__query_property("SubState").decode("utf-8")

    def query_cgroup(self) -> LinuxCgroup:
        return LinuxCgroup(self.__query_property("ControlGroup"))

    def query_process_ids(self) -> typing.Sequence[ProcessID]:
        return self.query_cgroup().query_process_ids()

    def query_fragment_path(self) -> pathlib.Path:
        return pathlib.Path(os.fsdecode(self.__query_property("FragmentPath")))

    def __query_property(self, property: str) -> bytes:
        stdout = self.__systemctl.check_output(
            ["show", f"--property={property}", "--", self.unit_name]
        )
        prefix = property.encode("utf-8") + b"="
        if not stdout.startswith(prefix):
            raise Exception(f"Failed to parse output of systemctl show: {stdout}")
        return stdout[len(prefix) :].rstrip(b"\n")

    @property
    def __systemctl(self) -> "_SystemctlCLI":
        return self.__systemd._systemctl

    def __str__(self) -> str:
        return f"{self.unit_name} (XDG_RUNTIME_DIR={self.__systemd.xdg_runtime_dir})"

    def __repr__(self) -> str:
        return (
            f"SystemdService(unit_name={repr(self.unit_name)}, "
            f"systemd={repr(self.__systemd)})"
        )


class _SystemctlCLI:
    def __init__(self, env: typing.Dict[str, str]) -> None:
        super().__init__()
        self.__env = env

    def check_call(
        self, command_arguments: typing.Sequence[typing.Union[str, pathlib.Path]]
    ) -> None:
        """Run 'systemctl --user' with the given arguments.

        See also subprocess.check_call.
        """
        subprocess.check_call(self.__command(command_arguments), env=self.__env)

    def check_output(
        self, command_arguments: typing.Sequence[typing.Union[str, pathlib.Path]]
    ) -> bytes:
        """Run 'systemctl --user' and return the command's output.

        See also subprocess.check_output.
        """
        return subprocess.check_output(
            self.__command(command_arguments), env=self.__env
        )

    def run(
        self,
        command_arguments: typing.Sequence[typing.Union[str, pathlib.Path]],
        stdout: "subprocess._FILE" = None,
        stderr: "subprocess._FILE" = None,
    ) -> subprocess.CompletedProcess:
        """Run 'systemctl --user' and return the command's output and exit status.

        See also subprocess.run.
        """
        return subprocess.run(
            self.__command(command_arguments),
            env=self.__env,
            stdout=stdout,
            stderr=stderr,
        )

    def __command(
        self, command_arguments: typing.Sequence[typing.Union[str, pathlib.Path]]
    ) -> typing.Sequence[str]:
        command = ["systemctl", "--user"]
        command.extend(str(arg) for arg in command_arguments)
        return command


class SystemdUserServiceManagerMixin(metaclass=abc.ABCMeta):
    def make_temporary_systemd_user_service_manager(self) -> SystemdUserServiceManager:
        context_manager = temporary_systemd_user_service_manager()
        exit = context_manager.__exit__
        systemd = context_manager.__enter__()
        self.addCleanup(lambda: exit(None, None, None))  # pyre-ignore (T36820067)
        return systemd

    def addCleanup(
        self,
        function: typing.Callable[..., typing.Any],
        *args: typing.Any,
        **kwargs: typing.Any,
    ) -> None:
        raise NotImplementedError()


@contextlib.contextmanager
def temporary_systemd_user_service_manager() -> typing.Iterator[
    SystemdUserServiceManager
]:
    """Create an isolated systemd instance for tests."""

    def should_create_managed() -> bool:
        forced_type_variable = "EDEN_TEST_FORCE_SYSTEMD_USER_SERVICE_MANAGER_TYPE"
        forced_type = os.getenv(forced_type_variable)
        if forced_type is not None and forced_type:
            if forced_type == "managed":
                return True
            if forced_type == "unmanaged":
                return False
            raise ValueError(
                f"Unsupported value for {forced_type_variable}: {forced_type!r}"
            )

        if not _is_system_booted_with_systemd():
            return False
        return True

    lifetime_duration = 30
    with create_tmp_dir() as xdg_runtime_dir:
        if should_create_managed():
            parent_systemd = SystemdUserServiceManager(
                xdg_runtime_dir=_get_current_xdg_runtime_dir()
            )
            with _transient_managed_systemd_user_service_manager(
                xdg_runtime_dir=xdg_runtime_dir,
                parent_systemd=parent_systemd,
                lifetime_duration=lifetime_duration,
            ) as child_systemd:
                yield child_systemd
        else:
            with _TransientUnmanagedSystemdUserServiceManager(
                xdg_runtime_dir=xdg_runtime_dir, lifetime_duration=lifetime_duration
            ) as systemd:
                yield systemd


def _is_system_booted_with_systemd() -> bool:
    """See the sd_booted(3) manual page."""
    return pathlib.Path("/run/systemd/system/").exists()


@contextlib.contextmanager
def _transient_managed_systemd_user_service_manager(
    xdg_runtime_dir: pathlib.Path,
    parent_systemd: SystemdUserServiceManager,
    lifetime_duration: int,
) -> typing.Iterator[SystemdUserServiceManager]:
    """Create an isolated systemd instance using 'systemd-run systemd'."""

    child_systemd_service = parent_systemd.systemd_run(
        command=["/usr/lib/systemd/systemd", "--user", "--unit=basic.target"],
        properties={
            "Description": f"Eden test systemd user service manager "
            f"({xdg_runtime_dir})",
            "CollectMode": "inactive-or-failed",
            "Restart": "no",
            "RuntimeMaxSec": str(lifetime_duration),
            "TimeoutStartSec": str(lifetime_duration),
            "Type": "notify",
        },
        extra_env={"XDG_RUNTIME_DIR": str(xdg_runtime_dir)},
    )
    child_systemd = SystemdUserServiceManager(xdg_runtime_dir=xdg_runtime_dir)
    try:
        yield child_systemd
    finally:
        try:
            child_systemd_service.stop()
        except Exception:
            logger.warning(
                f"Failed to stop systemd user service manager ({child_systemd})",
                exc_info=True,
            )
            # Ignore the exception.


class _TransientUnmanagedSystemdUserServiceManager:
    """Create an isolated systemd instance as child process.

    This class does not work if a user systemd instance is already running.
    """

    __child_systemd: SystemdUserServiceManager
    __cleanups: contextlib.ExitStack
    __lifetime_duration: int
    __parent_pty_fd: int
    __systemd_process: subprocess.Popen
    __xdg_runtime_dir: pathlib.Path

    def __init__(self, xdg_runtime_dir: pathlib.Path, lifetime_duration: int) -> None:
        super().__init__()
        self.__xdg_runtime_dir = xdg_runtime_dir
        self.__lifetime_duration = lifetime_duration
        self.__cleanups = contextlib.ExitStack()

    def start_systemd_process(self, output_fd: int) -> None:
        env = dict(os.environ)
        env["XDG_RUNTIME_DIR"] = str(self.__xdg_runtime_dir)
        # HACK(strager): Work around 'systemd --user' refusing to start if the
        # system is not managed by systemd.
        env["LD_PRELOAD"] = str(
            pathlib.Path(FindExe.FORCE_SD_BOOTED).resolve(strict=True)
        )
        self.__systemd_process = subprocess.Popen(
            [
                "timeout",
                f"{self.__lifetime_duration}s",
                "/usr/lib/systemd/systemd",
                "--user",
                "--unit=basic.target",
            ],
            stdin=subprocess.DEVNULL,
            stdout=output_fd,
            stderr=output_fd,
            env=env,
        )
        self.__cleanups.callback(lambda: self.stop_systemd_process())
        os.close(output_fd)

    def stop_systemd_process(self) -> None:
        self.__systemd_process.terminate()
        try:
            self.__systemd_process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            logger.warning(
                "Failed to terminate systemd user service manager", exc_info=True
            )
            # Ignore the exception.

    def wait_until_systemd_is_alive(self) -> None:
        while True:
            systemd_did_exit = self.__systemd_process.poll() is not None
            if systemd_did_exit:
                self.forward_process_output(timeout=1)
                raise Exception("systemd failed to start")
            if self.__child_systemd.is_alive():
                return
            self.forward_process_output(timeout=0.1)

    def forward_process_output(self, timeout: typing.Optional[float]) -> None:
        _copy_stream(
            source_fd=self.__parent_pty_fd,
            destination=sys.stderr.buffer,
            timeout=timeout,
        )

    def forward_process_output_in_background_thread(self) -> None:
        threading.Thread(
            target=lambda: self.forward_process_output(timeout=None), daemon=True
        ).start()

    def __enter__(self) -> SystemdUserServiceManager:
        # HACK(strager): The TestPilot test runner hangs if we pass any of our
        # standard file descriptors to systemd. Additionally, systemd doesn't write
        # anything to stderr if stderr is a pipe. Create a pseudoterminal and
        # manually forward systemd's logs to our stderr.
        (self.__parent_pty_fd, child_pty_fd) = pty.openpty()

        self.start_systemd_process(output_fd=child_pty_fd)
        self.__child_systemd = SystemdUserServiceManager(
            xdg_runtime_dir=self.__xdg_runtime_dir
        )
        self.wait_until_systemd_is_alive()
        self.forward_process_output_in_background_thread()
        return self.__child_systemd

    def __exit__(
        self,
        exc_type: typing.Optional[typing.Type[BaseException]],
        exc_value: typing.Optional[BaseException],
        traceback: typing.Optional[types.TracebackType],
    ) -> typing.Optional[bool]:
        self.__cleanups.close()
        # HACK(strager): Leak self.__parent_pty_fd. It might be in use by
        # self.forward_process_output in a background thread.
        return None


def _get_current_xdg_runtime_dir() -> pathlib.Path:
    problems = []
    path = None

    if path is None:
        path_from_env = os.environ.get("XDG_RUNTIME_DIR")
        if path_from_env is None or path_from_env == "":
            problems.append("$XDG_RUNTIME_DIR is not set")
        else:
            path = pathlib.Path(path_from_env)

    if path is None:
        if os.getuid() == 0:
            path = pathlib.Path("/run")
        else:
            path = pathlib.Path("/run/user") / str(os.getuid())

    assert path is not None
    if not path.exists():
        problems.append(f"'{path}' does not exist")
        raise Exception(
            "Could not determine XDG_RUNTIME_DIR: " + ", and ".join(problems)
        )
    return path


def _copy_stream(
    source_fd: int, destination: typing.IO[bytes], timeout: typing.Optional[float]
) -> None:
    while True:
        (read_ready, _write_ready, _x_ready) = select.select(
            [source_fd], [], [], timeout
        )
        if source_fd not in read_ready:
            break
        try:
            data = os.read(source_fd, 1024)
        except OSError as e:
            if e.errno == errno.EIO:
                break
            raise e
        if not data:
            break
        destination.write(data)
