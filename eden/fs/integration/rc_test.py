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

        out = eden.repository_cmd()
        self.assertEqual('', out)
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
