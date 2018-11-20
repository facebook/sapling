#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import os.path
import pathlib
import subprocess
import time
import typing
import unittest

from .lib.linux import ProcessID, is_cgroup_v2_mounted
from .lib.systemd import (
    SystemdService,
    SystemdUnitName,
    SystemdUserServiceManager,
    SystemdUserServiceManagerMixin,
)
from .lib.temporary_directory import TemporaryDirectoryMixin


class TemporarySystemdUserServiceManagerTest(
    unittest.TestCase, SystemdUserServiceManagerMixin
):
    def test_unit_paths_includes_manager_specific_directories(self) -> None:
        systemd = self.make_temporary_systemd_user_service_manager()
        paths = systemd.get_unit_paths()
        self.assertIn(systemd.xdg_runtime_dir / "systemd" / "user.control", paths)

    def test_no_units_are_active(self) -> None:
        def is_interesting_unit(unit_name: SystemdUnitName) -> bool:
            if unit_name in ("-.slice"):
                return False
            if unit_name in ("dbus.service", "dbus.socket"):
                return False
            if unit_name.endswith(".mount") or unit_name.endswith(".swap"):
                return False
            if unit_name.endswith(".scope"):
                return False
            if unit_name.endswith(".target"):
                return False
            return True

        systemd = self.make_temporary_systemd_user_service_manager()
        unit_names = systemd.get_active_unit_names()
        self.assertEqual(
            [unit for unit in unit_names if is_interesting_unit(unit)],
            [],
            f"systemd should be managing no interesting units\n"
            f"All units: {unit_names}",
        )


class SystemdServiceTest(
    unittest.TestCase, TemporaryDirectoryMixin, SystemdUserServiceManagerMixin
):
    systemd: SystemdUserServiceManager

    def setUp(self) -> None:
        super().setUp()
        self.systemd = self.make_temporary_systemd_user_service_manager()

    def test_str_of_service_includes_unit_name_and_systemd_directory(self) -> None:
        service = SystemdService(unit_name="my-test-unit.service", systemd=self.systemd)
        self.assertRegex(
            str(service), r"^my-test-unit\.service \(XDG_RUNTIME_DIR=/\S+\)$"
        )

    def test_repr_of_service_includes_unit_name_and_systemd_directory(self) -> None:
        service = SystemdService(unit_name="my-test-unit.service", systemd=self.systemd)
        self.assertRegex(
            repr(service),
            r"^SystemdService\("
            r"unit_name='my-test-unit\.service', "
            r"systemd=SystemdUserServiceManager\("
            r"xdg_runtime_dir=PosixPath\('\S+'\)"
            r"\)"
            r"\)",
        )

    def test_start_executes_oneshot_service(self) -> None:
        message_file = pathlib.Path(self.make_temporary_directory()) / "message.txt"
        service = self.enable_service(
            "test-SystemdServiceTest.service",
            f"""
[Service]
Type=oneshot
ExecStart=/bin/echo "Hello from service"
StandardOutput=file:{message_file}
""",
        )
        service.start()
        self.assertEqual(message_file.read_text(), "Hello from service\n")

    def test_start_executes_oneshot_instanced_service(self) -> None:
        temp_dir = pathlib.Path(self.make_temporary_directory())
        message_file = temp_dir / "message.txt"

        unit_file = temp_dir / "test-SystemdServiceTest@.service"
        unit_file.write_text(
            f"""
[Service]
Type=oneshot
ExecStart=/bin/echo "instance: %i"
StandardOutput=file:{message_file}
"""
        )
        self.systemd.enable_runtime_unit_from_file(unit_file=unit_file)

        service = self.systemd.get_service("test-SystemdServiceTest@hello.service")
        service.start()
        self.assertEqual(message_file.read_text(), "instance: hello\n")

    def test_unstarted_service_is_inactive(self) -> None:
        service = self.enable_service(
            "test-SystemdServiceTest.service",
            """
[Service]
ExecStart=/bin/false
""",
        )
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("inactive", "dead"),
        )

    def test_running_simple_service_is_active(self) -> None:
        service = self.enable_service(
            "test-SystemdServiceTest.service",
            """
[Service]
Type=simple
ExecStart=/bin/sleep 30
""",
        )
        service.start()
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("active", "running"),
        )

    def test_service_exiting_with_code_1_is_failed(self) -> None:
        service = self.enable_service(
            "test-SystemdServiceTest.service",
            """
[Service]
Type=notify
ExecStart=/bin/false
""",
        )
        try:
            service.start()
        except subprocess.CalledProcessError:
            pass
        self.assertEqual(
            (service.query_active_state(), service.query_sub_state()),
            ("failed", "failed"),
        )

    @unittest.skipIf(
        not is_cgroup_v2_mounted(),
        "T36934106: Fix EdenFS systemd integration tests for cgroups v1",
    )
    def test_processes_of_forking_service_includes_all_child_processes(self) -> None:
        service = self.enable_service(
            "test-SystemdServiceTest.service",
            """
[Service]
Type=forking
ExecStart=/bin/sh -c "/bin/sleep 30 | /bin/cat & exit"
""",
        )
        service.start()

        # HACK(strager): Sometimes, /bin/sh appears inside the cgroup's process
        # list. Wait a bit to reduce test flakiness.
        # TODO(strager): Figure out why sometimes /bin/sh is still inside the
        # cgroup's process list.
        time.sleep(1)

        process_ids = service.query_process_ids()
        process_exes = [get_resolved_process_exe_or_error(pid) for pid in process_ids]
        expected_process_exes = [
            pathlib.Path(p).resolve() for p in ["/bin/sleep", "/bin/cat"]
        ]
        self.assertCountEqual(
            process_exes, expected_process_exes, f"Process IDs: {process_ids}"
        )

    def enable_service(
        self, service_name: SystemdUnitName, unit_file_content: str
    ) -> SystemdService:
        unit_file = pathlib.Path(self.make_temporary_directory()) / service_name
        unit_file.write_text(unit_file_content)
        self.systemd.enable_runtime_unit_from_file(unit_file=unit_file)
        return self.systemd.get_service(service_name)


def get_process_exe(process_id: ProcessID) -> pathlib.Path:
    return pathlib.Path(os.readlink(pathlib.Path("/proc") / str(process_id) / "exe"))


def get_process_exe_or_error(
    process_id: ProcessID
) -> typing.Union[pathlib.Path, OSError]:
    try:
        return get_process_exe(process_id)
    except OSError as e:
        return e


def get_resolved_process_exe_or_error(
    process_id: ProcessID
) -> typing.Union[pathlib.Path, OSError]:
    try:
        return get_process_exe(process_id).resolve()
    except OSError as e:
        return e
