#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import asyncio
import getpass
import os
import pathlib
import sys
from typing import List, Optional

from . import daemon_util
from .config import EdenInstance
from .logfile import forward_log_file
from .systemd import (
    EdenFSSystemdServiceConfig,
    SystemdConnectionRefusedError,
    SystemdFileNotFoundError,
    SystemdServiceFailedToStartError,
    SystemdUserBus,
    edenfs_systemd_service_name,
    print_service_status_using_systemctl_for_diagnostics_async,
)
from .util import print_stderr


async def start_systemd_service(
    instance: EdenInstance,
    daemon_binary: Optional[str] = None,
    edenfs_args: Optional[List[str]] = None,
) -> int:
    try:
        daemon_binary = daemon_util.find_daemon_binary(daemon_binary)
    except daemon_util.DaemonBinaryNotFound as e:
        print_stderr(f"error: {e}")
        return 1

    service_config = EdenFSSystemdServiceConfig(
        eden_dir=instance.state_dir,
        edenfs_executable_path=pathlib.Path(daemon_binary),
        extra_edenfs_arguments=edenfs_args or [],
    )
    service_config.write_config_file()
    service_name: str = edenfs_systemd_service_name(instance.state_dir)
    xdg_runtime_dir: str = _get_systemd_xdg_runtime_dir(config=instance)

    startup_log_path = service_config.startup_log_file_path
    startup_log_path.write_bytes(b"")
    with forward_log_file(startup_log_path, sys.stderr.buffer) as log_forwarder:
        loop: asyncio.AbstractEventLoop = asyncio.get_event_loop()

        async def start_service_async() -> int:
            async with SystemdUserBus(xdg_runtime_dir=xdg_runtime_dir) as systemd:
                service_name_bytes = service_name.encode()
                active_state = await systemd.get_unit_active_state_async(
                    service_name_bytes
                )
                if active_state == b"active":
                    print_stderr("error: EdenFS systemd service is already running")
                    await print_service_status_using_systemctl_for_diagnostics_async(
                        service_name=service_name, xdg_runtime_dir=xdg_runtime_dir
                    )
                    return 1

                await systemd.start_service_and_wait_async(service_name_bytes)
                return 0

        try:
            loop.create_task(log_forwarder.poll_forever_async())
            return await start_service_async()
        except (SystemdConnectionRefusedError, SystemdFileNotFoundError):
            print_stderr(
                f"error: The systemd user manager is not running. Run the "
                f"following command to\n"
                f"start it, then try again:\n"
                f"\n"
                f"  sudo systemctl start user@{getpass.getuser()}.service"
            )
            return 1
        except SystemdServiceFailedToStartError as e:
            print_stderr(f"error: {e}")
            return 1
        finally:
            log_forwarder.poll()


def _get_systemd_xdg_runtime_dir(config: EdenInstance) -> str:
    xdg_runtime_dir = os.getenv("XDG_RUNTIME_DIR")
    if xdg_runtime_dir is None:
        xdg_runtime_dir = config.get_fallback_systemd_xdg_runtime_dir()
        print_stderr(
            f"warning: The XDG_RUNTIME_DIR environment variable is not set; "
            f"using fallback: {xdg_runtime_dir!r}"
        )
    return xdg_runtime_dir
