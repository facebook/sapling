#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import pathlib

from parameterized import parameterized

from .lib import testcase


@testcase.eden_test(run_io_uring=True)
class ListTest(testcase.EdenTestCase):
    @parameterized.expand(
        [
            ("rust", {"EDENFSCTL_ONLY_RUST": "1"}),
            ("python", {"EDENFSCTL_SKIP_RUST": "1"}),
        ]
    )
    def test_list_skips_missing_client_state(
        self, impl: str, env: dict[str, str]
    ) -> None:
        repo = self.create_hg_repo("main")
        repo.write_file("hello", "hola\n")
        repo.commit("Initial commit.")

        self.eden.clone(repo.path, self.mount)

        client_dir = self.eden.client_dir_for_mount(self.mount_path)
        config_path = client_dir / "config.toml"
        self.assertTrue(
            config_path.is_file(),
            f"config.toml should exist before removal: {config_path}",
        )
        config_path.unlink()

        mounts = self.eden.list_cmd(env=env)
        expected_mount = str(self.mount_path)
        normalized_mounts = {
            str(pathlib.Path(mount)): mount_info for mount, mount_info in mounts.items()
        }
        self.assertIn(
            expected_mount,
            normalized_mounts,
            f"{impl} eden list should include the mount",
        )
        self.assertEqual("RUNNING", normalized_mounts[expected_mount]["state"])
