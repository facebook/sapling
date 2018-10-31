#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

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
import typing

from .find_executables import FindExe
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
        result = subprocess.run(
            ["systemctl", "--user", "show-environment"],
            env=self.env,
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

    def systemd_run(
        self,
        command: typing.Sequence[str],
        properties: typing.Mapping[str, str],
        extra_env: typing.Mapping[str, str],
    ) -> SystemdUnitName:
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
        return match.group("unit")

    def get_active_unit_names(self) -> typing.List[SystemdUnitName]:
        def parse_line(line: str) -> SystemdUnitName:
            parts = re.split(r" +", line)
            return parts[0]

        stdout = subprocess.check_output(
            [
                "systemctl",
                "--user",
                "list-units",
                "--all",
                "--full",
                "--no-legend",
                "--no-pager",
                "--plain",
                "--state=active",
            ],
            env=self.env,
        )
        return [parse_line(line) for line in stdout.decode("utf-8").splitlines()]

    def get_unit_paths(self) -> typing.List[pathlib.Path]:
        stdout = subprocess.check_output(
            ["systemd-analyze", "--user", "unit-paths"], env=self.env
        )
        return [pathlib.Path(line) for line in stdout.decode("utf-8").splitlines()]

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

    def __str__(self) -> str:
        return f"systemd ({self.xdg_runtime_dir})"


@contextlib.contextmanager
def temporary_systemd_user_service_manager() -> typing.Iterator[
    SystemdUserServiceManager
]:
    """Create an isolated systemd instance for tests."""

    lifetime_duration = 30
    with create_tmp_dir() as xdg_runtime_dir:
        if _is_system_booted_with_systemd():
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
            with _transient_unmanaged_systemd_user_service_manager(
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
            subprocess.check_call(
                ["systemctl", "--user", "stop", "--", child_systemd_service],
                env=child_systemd.env,
            )
        except Exception:
            logger.warning(
                f"Failed to stop systemd user service manager ({child_systemd})",
                exc_info=True,
            )
            # Ignore the exception.


@contextlib.contextmanager
def _transient_unmanaged_systemd_user_service_manager(
    xdg_runtime_dir: pathlib.Path, lifetime_duration: int
) -> typing.Iterator[SystemdUserServiceManager]:
    """Create an isolated systemd instance as child process.

    This function does not work if a user systemd instance is already running.
    """

    parent_pty_fd: int
    systemd_process: subprocess.Popen

    def start_systemd_process(output_fd: int) -> subprocess.Popen:
        env = dict(os.environ)
        env["XDG_RUNTIME_DIR"] = str(xdg_runtime_dir)
        # HACK(strager): Work around 'systemd --user' refusing to start if the
        # system is not managed by systemd.
        env["LD_PRELOAD"] = str(
            pathlib.Path(FindExe.FORCE_SD_BOOTED).resolve(strict=True)
        )
        systemd_process = subprocess.Popen(
            [
                "timeout",
                f"{lifetime_duration}s",
                "/usr/lib/systemd/systemd",
                "--user",
                "--unit=basic.target",
            ],
            stdin=subprocess.DEVNULL,
            stdout=output_fd,
            stderr=output_fd,
            env=env,
        )
        os.close(output_fd)
        return systemd_process

    def stop_systemd_process() -> None:
        systemd_process.terminate()
        try:
            systemd_process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            logger.warning(
                "Failed to terminate systemd user service manager", exc_info=True
            )
            # Ignore the exception.

    def wait_until_systemd_is_alive() -> None:
        while True:
            systemd_did_exit = systemd_process.poll() is not None
            if systemd_did_exit:
                forward_process_output(timeout=1)
                raise Exception("systemd failed to start")
            if child_systemd.is_alive():
                return
            forward_process_output(timeout=0.1)

    def forward_process_output(timeout: typing.Optional[float]) -> None:
        _copy_stream(
            source_fd=parent_pty_fd, destination=sys.stderr.buffer, timeout=timeout
        )

    def forward_process_output_in_background_thread() -> None:
        threading.Thread(
            target=lambda: forward_process_output(timeout=None), daemon=True
        ).start()

    # HACK(strager): The TestPilot test runner hangs if we pass any of our
    # standard file descriptors to systemd. Additionally, systemd doesn't write
    # anything to stderr if stderr is a pipe. Create a pseudoterminal and
    # manually forward systemd's logs to our stderr.
    (parent_pty_fd, child_pty_fd) = pty.openpty()

    systemd_process = start_systemd_process(child_pty_fd)
    try:
        child_systemd = SystemdUserServiceManager(xdg_runtime_dir=xdg_runtime_dir)
        wait_until_systemd_is_alive()
        forward_process_output_in_background_thread()
        yield child_systemd
    finally:
        stop_systemd_process()

    # HACK(strager): Leak parent_pty_fd. It might be in use by
    # forward_process_output in a background thread.


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
