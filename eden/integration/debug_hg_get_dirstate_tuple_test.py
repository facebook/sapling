#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib import testcase
import os
from textwrap import dedent


@testcase.eden_repo_test
class DebugHgGetDirstateTupleTest:
    def populate_repo(self):
        self.repo.write_file('hello', 'hola\n')
        self.repo.write_file(os.path.join('dir', 'file'), 'blah\n')
        self.repo.commit('Initial commit.')

    def test_get_dirstate_tuple_normal_file(self):
        output = self.eden.run_cmd(
            'debug', 'hg_get_dirstate_tuple', os.path.join(self.mount, 'hello')
        )
        expected = dedent('''\
        hello
            status = Normal
            mode = 0o100644
            mergeState = NotApplicable
        ''')
        self.assertEqual(expected, output)
