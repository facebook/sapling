#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import unittest

from .lib.systemd import SystemdUnitName, temporary_systemd_user_service_manager


class TemporarySystemdUserServiceManagerTest(unittest.TestCase):
    def test_unit_paths_includes_manager_specific_directories(self) -> None:
        with temporary_systemd_user_service_manager() as systemd:
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

        with temporary_systemd_user_service_manager() as systemd:
            unit_names = systemd.get_active_unit_names()
            self.assertEqual(
                [unit for unit in unit_names if is_interesting_unit(unit)],
                [],
                f"systemd should be managing no interesting units\n"
                f"All units: {unit_names}",
            )
