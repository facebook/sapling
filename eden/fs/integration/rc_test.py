#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
from .lib import testcase


@testcase.eden_repo_test
class RCTest:
    def populate_repo(self):
        self.repo.write_file('readme.txt', 'test\n')
        self.repo.commit('Initial commit.')

    def test_eden_list(self):
        mount_paths = self.eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='There should only be 1 mount path')
        self.assertEqual(self.mount, mount_paths[0])

        self.eden.unmount(self.mount)
        mount_paths = self.eden.list_cmd().split('\n')[:-1]
        self.assertEqual(0, len(mount_paths),
                         msg='There should be 0 mount paths after unmount')

        self.eden.clone(self.repo_name, self.mount)
        mount_paths = self.eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='There should be 1 mount path after clone')
        self.assertEqual(self.mount, mount_paths[0])

    def test_unmount_rmdir(self):
        clients = os.path.join(self.eden_dir, 'clients')
        client_names = os.listdir(clients)
        self.assertEqual(1, len(client_names),
                         msg='There should only be 1 client')
        test_client_dir = os.path.join(clients, client_names[0])

        # Eden list command uses keys of directory map to get mount paths
        mount_paths = self.eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='There should only be 1 path in the directory map')
        self.assertEqual(self.mount, mount_paths[0])

        self.eden.unmount(self.mount)
        self.assertFalse(os.path.isdir(test_client_dir))

        # Check that _remove_path_from_directory_map in unmount is successful
        mount_paths = self.eden.list_cmd().split('\n')[:-1]
        self.assertEqual(0, len(mount_paths),
                         msg='There should be 0 paths in the directory map')

        self.eden.clone(self.repo_name, self.mount)
        self.assertTrue(os.path.isdir(test_client_dir),
                        msg='Client name should be restored verbatim because \
                             it should be a function of the mount point')
        mount_paths = self.eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='The client directory should have been restored')
        self.assertEqual(self.mount, mount_paths[0],
                         msg='Client directory name should match client name')
