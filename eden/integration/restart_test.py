#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import os
import sys

from .lib import testcase


@testcase.eden_repo_test
class RestartTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file('adir/file', 'foo!\n')
        self.repo.symlink('slink', 'hello')
        self.repo.commit('Initial commit.')

    def edenfs_logging_settings(self):
        return {'eden.strace': 'DBG7', 'eden.fs.fuse': 'DBG7'}

    def test_restart(self):
        hello = os.path.join(self.mount, 'hello')
        with open(hello, 'r') as f:
            self.assertEqual('hola\n', f.read())

        # TODO: Once we fully support mount takeover, confirm that open file
        # handles work across restart.
        print('=== beginning restart ===', file=sys.stderr)
        self.eden.graceful_restart()
        print('=== restart complete ===', file=sys.stderr)

        with open(hello, 'r') as f:
            self.assertEqual('hola\n', f.read())
