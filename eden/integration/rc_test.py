#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

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

        self.eden.clone(self.repo.path, self.mount)
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

        self.eden.clone(self.repo.path, self.mount)
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
