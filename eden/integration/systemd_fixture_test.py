#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import errno
import os
import os.path
import pathlib
import subprocess
import time
import typing
import unittest

from eden.cli.daemon import did_process_exit
from eden.test_support.environment_variable import EnvironmentVariableMixin
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from .lib.linux import ProcessID, is_cgroup_v2_mounted
from .lib.systemd import (
    SystemdService,
    SystemdUnitName,
    SystemdUserServiceManager,
    SystemdUserServiceManagerMixin,
    temporary_systemd_user_service_manager,
)


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
            if unit_name.endswith(".device"):
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

    def test_manager_process_id_is_valid(self) -> None:
        with temporary_systemd_user_service_manager() as systemd:
            self.assertTrue(does_process_exist(systemd.process_id))

    def test_closing_manager_kills_process(self) -> None:
        with temporary_systemd_user_service_manager() as systemd:
            process_id = systemd.process_id
        self.assertFalse(does_process_exist(process_id))

    def test_exit_kills_manager(self) -> None:
        systemd = self.make_temporary_systemd_user_service_manager()
        process_id = systemd.process_id
        systemd.exit()
        self.assertFalse(systemd.is_alive())
        self.assertTrue(did_process_exit(process_id))


class TemporarySystemdUserServiceManagerIsolationTest(
    unittest.TestCase,
    EnvironmentVariableMixin,
    SystemdUserServiceManagerMixin,
    TemporaryDirectoryMixin,
):
    def test_services_with_same_name_by_different_managers_are_independent(
        self
    ) -> None:
        systemd_1 = self.make_temporary_systemd_user_service_manager()
        systemd_2 = self.make_temporary_systemd_user_service_manager()
        unit_name = "isolation_test.service"
        service_1 = systemd_1.systemd_run(
            command=["/bin/sleep", "10"],
            properties={"RemainAfterExit": "yes"},
            extra_env={},
            unit_name=unit_name,
        )
        service_2 = systemd_2.systemd_run(
            command=["/bin/sleep", "10"],
            properties={"RemainAfterExit": "yes"},
            extra_env={},
            unit_name=unit_name,
        )
        service_1.stop()
        self.assertEqual(
            (service_2.query_active_state(), service_2.query_sub_state()),
            ("active", "running"),
            "Stopping systemd_1's service should not stop systemd_2's service",
        )

    def test_manager_cannot_see_services_of_different_manager(self) -> None:
        systemd_1 = self.make_temporary_systemd_user_service_manager()
        systemd_2 = self.make_temporary_systemd_user_service_manager()
        service = systemd_1.systemd_run(
            command=["/bin/sleep", "10"],
            properties={"RemainAfterExit": "yes"},
            extra_env={},
        )
        self.assertIn(
            service.unit_name,
            systemd_1.get_active_unit_names(),
            "systemd_1 should see its own unit",
        )
        self.assertNotIn(
            service.unit_name,
            systemd_2.get_active_unit_names(),
            "systemd_2 should not see systemd_1's unit",
        )

    def test_environment_variables_do_not_leak_to_services(self) -> None:
        spy_variable_name = "EDEN_TEST_VARIABLE"
        self.set_environment_variable(
            spy_variable_name, "this should not propogate to the service"
        )

        systemd = self.make_temporary_systemd_user_service_manager()
        env_variables = self.get_service_environment(systemd)

        env_variable_names = [name for (name, value) in env_variables]
        self.assertIn(
            "PATH",
            env_variable_names,
            "Sanity check: $PATH should be set in service environment",
        )
        self.assertNotIn(spy_variable_name, env_variable_names)

    def test_path_environment_variable_is_forced_to_default(self) -> None:
        # See https://www.freedesktop.org/software/systemd/man/systemd.exec.html#%24PATH
        allowed_path_entries = {
            "/usr/local/sbin",
            "/usr/local/bin",
            "/usr/sbin",
            "/usr/bin",
            "/sbin",
            "/bin",
        }

        spy_path_entry = self.make_temporary_directory()
        self.set_environment_variable(
            "PATH", spy_path_entry + os.pathsep + os.environ["PATH"]
        )

        systemd = self.make_temporary_systemd_user_service_manager()
        env_variables = self.get_service_environment(systemd)

        path_value = [value for (name, value) in env_variables if name == "PATH"][0]
        for path_entry in path_value.split(os.pathsep):
            self.assertIn(
                path_entry,
                allowed_path_entries,
                "$PATH should only include default paths\n$PATH: {path_value!r}",
            )

    def get_service_environment(
        self, systemd: SystemdUserServiceManager
    ) -> typing.List[typing.Tuple[str, str]]:
        env_output_file = pathlib.Path(self.make_temporary_directory()) / "env_output"
        env_service = systemd.systemd_run(
            command=["/usr/bin/env", "-0"],
            properties={"StandardOutput": f"file:{env_output_file}"},
            extra_env={},
        )
        env_service.poll_until_inactive(timeout=10)

        def parse_entry(entry_str: str) -> typing.Tuple[str, str]:
            [name, value] = entry_str.split("=", 1)
            return (name, value)

        env_output = env_output_file.read_text()
        return [
            parse_entry(entry_str) for entry_str in env_output.split("\0") if entry_str
        ]


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
            r".*"
            r"unit_name='my-test-unit\.service'"
            r".*"
            r"systemd=SystemdUserServiceManager\("
            r".*"
            r"xdg_runtime_dir=PosixPath\('\S+'\)"
            r".*"
            r"\)"
            r".*"
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


def does_process_exist(process_id: int) -> bool:
    try:
        os.kill(process_id, 0)
    except OSError as ex:
        if ex.errno == errno.ESRCH:
            return False
        if ex.errno == errno.EPERM:
            return True
        raise ex
    else:
        return True
