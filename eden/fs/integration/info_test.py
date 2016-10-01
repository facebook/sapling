#!/usr/bin/env python3
#
# Copyright (c) 2016, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import testcase
import hashlib
import json
import os
import unittest

# This is the name of the default repository created by EdenRepoTestBase.
repo_name = 'main'


@testcase.eden_repo_test
class InfoTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.commit('Initial commit.')

    @unittest.skip('bind-mounts are not torn down correctly')
    def test_info_with_bind_mounts(self):
        edenrc = os.path.join(os.environ['HOME'], '.edenrc')
        with open(edenrc, 'w') as f:
            f.write('''\
[repository {repo_name}]
path = {repo_path}
type = {repo_type}

[bindmounts {repo_name}]
buck-out = buck-out
'''.format(repo_name=repo_name,
             repo_path=self.repo.get_canonical_root(),
             repo_type=self.repo.get_type()))

        tmp = os.path.join(self.tmp_dir, 'eden_mount')

        self.eden.run_cmd('clone', repo_name, tmp)
        info = self.eden.run_cmd('info', tmp)

        client_info = json.loads(info)
        client_dir = os.path.join(self.eden_dir,
                                  'clients',
                                  hashlib.sha1(tmp.encode('utf-8')).hexdigest())
        self.assertEqual({
            'bind-mounts': {
                'buck-out': 'buck-out',
            },
            'client-dir': client_dir,
            'mount': tmp,
            'snapshot': self.repo.get_head_hash(),
        }, client_info)

    def test_relative_path(self):
        '''
        Test calling "eden info <relative_path>" and make sure it gives
        the expected results.
        '''
        info = self.eden.run_cmd('info', os.path.relpath(self.mount))

        client_info = json.loads(info)
        client_dir = os.path.join(
            self.eden_dir,
            'clients',
            hashlib.sha1(self.mount.encode('utf-8')).hexdigest())
        self.assertEqual({
            'bind-mounts': {},
            'client-dir': client_dir,
            'mount': self.mount,
            'snapshot': self.repo.get_head_hash(),
        }, client_info)

    def test_through_symlink(self):
        '''
        Test calling "eden info" through a symlink and make sure it gives
        the expected results.  This makes sure "eden info" resolves the path
        correctly before looking it up in the configuration.
        '''
        link1 = os.path.join(self.tmp_dir, 'link1')
        os.symlink(self.mount, link1)

        info1 = json.loads(self.eden.run_cmd('info', link1))
        self.assertEqual(self.mount, info1['mount'])

        # Create a non-normalized symlink pointing to the parent directory
        # of the mount
        link2 = os.path.join(self.tmp_dir, 'mounts_link')
        os.symlink(self.mount + '//..', link2)
        mount_through_link2 = os.path.join(link2, self.repo_name)

        info2 = json.loads(self.eden.run_cmd('info', mount_through_link2))
        self.assertEqual(self.mount, info2['mount'])
