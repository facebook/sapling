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


class RCTest(testcase.EdenTestCase):
    def test_list_repository(self):
        eden = self.init_git_eden()

        out = eden.repository_cmd().split('\n')[:-1]
        expected = ['CLIENT']
        self.assertEqual(expected, out)
        config = '''\
[repository fbsource]
path = /data/users/carenthomas/fbsource
type = git

[bindmounts fbsource]
fbcode-buck-out = fbcode/buck-out
fbandroid-buck-out = fbandroid/buck-out
fbobjc-buck-out = fbobjc/buck-out
buck-out = buck-out

[repository git]
path = /home/carenthomas/src/git
type = git

[repository hg-crew]
url = /data/users/carenthomas/facebook-hg-rpms/hg-crew
type = hg
'''
        home_config_file = os.path.join(eden._home_dir, '.edenrc')
        with open(home_config_file, 'w') as f:
            f.write(config)
        out = eden.repository_cmd().split('\n')[:-1]
        expected = ['fbsource', 'git', 'hg-crew']
        self.assertEqual(expected, out)

    def test_eden_list(self):
        eden = self.init_git_eden()

        mount_paths = eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='There should only be 1 mount path')
        self.assertEqual(eden.mount_path, mount_paths[0])

        eden.unmount_cmd()
        mount_paths = eden.list_cmd().split('\n')[:-1]
        self.assertEqual(0, len(mount_paths),
                         msg='There should be 0 mount paths after unmount')

        eden.clone_cmd()
        mount_paths = eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='There should be 1 mount path after clone')
        self.assertEqual(eden.mount_path, mount_paths[0])

    def test_unmount_rmdir(self):
        eden = self.init_git_eden()

        clients = os.path.join(eden._config_dir, 'clients')
        client_names = os.listdir(clients)
        self.assertEqual(1, len(client_names),
                         msg='There should only be 1 client')
        test_client_dir = os.path.join(clients, client_names[0])

        # Eden list command uses keys of directory map to get mount paths
        mount_paths = eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='There should only be 1 path in the directory map')
        self.assertEqual(eden.mount_path, mount_paths[0])

        eden.unmount_cmd()
        self.assertFalse(os.path.isdir(test_client_dir))

        # Check that _remove_path_from_directory_map in unmount is successful
        mount_paths = eden.list_cmd().split('\n')[:-1]
        self.assertEqual(0, len(mount_paths),
                         msg='There should be 0 paths in the directory map')

        eden.clone_cmd()
        self.assertTrue(os.path.isdir(test_client_dir),
                        msg='Client name should be restored verbatim because \
                             it should be a function of the mount point')
        mount_paths = eden.list_cmd().split('\n')[:-1]
        self.assertEqual(1, len(mount_paths),
                         msg='The client directory should have been restored')
        self.assertEqual(eden.mount_path, mount_paths[0],
                         msg='Client directory name should match client name')
