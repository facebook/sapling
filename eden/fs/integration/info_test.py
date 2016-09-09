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
             repo_path=self.repo.path,
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
