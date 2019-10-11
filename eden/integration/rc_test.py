#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import stat
import tempfile
import textwrap

from eden.cli import util

from .lib import testcase


@testcase.eden_repo_test
class RCTest(testcase.EdenRepoTest):
    def populate_repo(self) -> None:
        self.repo.write_file("readme.txt", "test\n")
        self.repo.commit("Initial commit.")

    def test_eden_list(self) -> None:
        mounts = self.eden.run_cmd("list")
        self.assertEqual(f"{self.mount}\n", mounts)

        self.eden.remove(self.mount)
        mounts = self.eden.run_cmd("list")
        self.assertEqual("", mounts, msg="There should be 0 mount paths after remove")

        self.eden.clone(self.repo_name, self.mount)
        mounts = self.eden.run_cmd("list")
        self.assertEqual(f"{self.mount}\n", mounts)

    def test_unmount_rmdir(self) -> None:
        clients = os.path.join(self.eden_dir, "clients")
        client_names = os.listdir(clients)
        self.assertEqual(1, len(client_names), msg="There should only be 1 client")
        test_client_dir = os.path.join(clients, client_names[0])

        # Eden list command uses keys of directory map to get mount paths
        mounts = self.eden.list_cmd_simple()
        self.assertEqual({self.mount: "RUNNING"}, mounts)

        self.eden.remove(self.mount)
        self.assertFalse(os.path.isdir(test_client_dir))

        # Check that _remove_path_from_directory_map in remove is successful
        mounts = self.eden.list_cmd_simple()
        self.assertEqual({}, mounts, msg="There should be 0 paths in the directory map")

        self.eden.clone(self.repo_name, self.mount)
        self.assertTrue(
            os.path.isdir(test_client_dir),
            msg="Client name should be restored verbatim because \
                             it should be a function of the mount point",
        )
        mounts = self.eden.list_cmd_simple()
        self.assertEqual(
            {self.mount: "RUNNING"},
            mounts,
            msg="The client directory should have been restored",
        )

    def test_override_system_config(self) -> None:
        system_repo = self.create_repo("system_repo")

        system_repo.write_file("hello.txt", "hola\n")
        system_repo.commit("Initial commit.")

        repo_info = util.get_repo(system_repo.path)
        assert repo_info is not None

        # Create temporary system config
        system_config_dir = self.eden.etc_eden_dir / "config.d"
        system_config_dir.mkdir(parents=True, exist_ok=True)
        f, path = tempfile.mkstemp(dir=str(system_config_dir), suffix=".toml")

        # Add system_repo to system config file
        config = textwrap.dedent(
            f"""\
            ["repository {self.repo_name}"]
            path = "{repo_info.source}"
            type = "{repo_info.type}"
            """
        )
        os.write(f, config.encode("utf-8"))
        os.close(f)

        # Clone repository
        mount_path = os.path.join(self.mounts_dir, self.repo_name + "-1")
        self.eden.clone(self.repo_name, mount_path)

        # Verify that clone used repository data from user config
        readme = os.path.join(mount_path, "hello.txt")
        self.assertFalse(os.path.exists(readme))

        hello = os.path.join(mount_path, "readme.txt")
        st = os.lstat(hello)
        self.assertTrue(stat.S_ISREG(st.st_mode))

        with open(hello, "r") as hello_file:
            self.assertEqual("test\n", hello_file.read())

        # Add system_repo to system config file with new name
        repo_name = "repo"
        f = os.open(path, os.O_WRONLY)
        config = textwrap.dedent(
            f"""\
            ["repository {repo_name}"]
            path = "{repo_info.source}"
            type = "{repo_info.type}"
            """
        )
        os.write(f, config.encode("utf-8"))
        os.close(f)

        # Clone repository
        mount_path = os.path.join(self.mounts_dir, repo_name + "-1")
        self.eden.clone(repo_name, mount_path)

        # Verify that clone used repository data from system config
        readme = os.path.join(mount_path, "readme.txt")
        self.assertFalse(os.path.exists(readme))

        hello = os.path.join(mount_path, "hello.txt")
        st = os.lstat(hello)
        self.assertTrue(stat.S_ISREG(st.st_mode))

        with open(hello, "r") as hello_file:
            self.assertEqual("hola\n", hello_file.read())
