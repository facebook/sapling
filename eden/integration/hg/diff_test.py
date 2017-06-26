#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

from .lib.hg_extension_test_base import HgExtensionTestBase
import re


class DiffTest(HgExtensionTestBase):
    def populate_backing_repo(self, repo):
        repo.write_file('rootfile.txt', '')
        repo.write_file('dir1/a.txt', 'original contents\n')
        repo.commit('Initial commit.')

    def check_output(self, output, regex_lines):
        output_lines = output.splitlines()
        for line_num, expected in enumerate(regex_lines):
            self.assertLess(line_num, len(output_lines))
            actual = output_lines[line_num]
            if hasattr(expected, 'match'):
                self.assertRegex(actual, expected,
                                 'mismatch on line %s' % line_num)
            else:
                self.assertEqual(actual, expected,
                                 'mismatch on line %s' % line_num)
        if line_num + 1 != len(output_lines):
            self.fail('extra output lines: %r' % (output_lines[line_num:],))

    def test_modify_file(self):
        self.write_file('dir1/a.txt', 'new line\noriginal contents\n')
        diff_output = self.hg('diff')
        expected_lines = [
            'diff -r 2feca41797bd dir1/a.txt',
            '--- a/dir1/a.txt\tSat Jan 01 08:00:00 2000 +0000',
            re.compile(re.escape('+++ b/dir1/a.txt\t') + '.*'),
            '@@ -1,1 +1,2 @@',
            '+new line',
            ' original contents',
        ]
        self.check_output(diff_output, expected_lines)

    def test_add_file(self):
        self.write_file('dir1/b.txt', 'new file\n1234\n5678\n')
        self.hg('add', 'dir1/b.txt')
        diff_output = self.hg('diff')
        expected_lines = [
            'diff -r 2feca41797bd dir1/b.txt',
            '--- /dev/null\tThu Jan 01 00:00:00 1970 +0000',
            re.compile(re.escape('+++ b/dir1/b.txt\t') + '.*'),
            '@@ -0,0 +1,3 @@',
            '+new file',
            '+1234',
            '+5678',
        ]
        self.check_output(diff_output, expected_lines)
