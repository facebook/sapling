import test_util

import sys
import unittest

class TestFetchDirectoryRemoval(test_util.TestBase):
    stupid_mode_tests = True

    def test_removal(self):
        repo = self._load_fixture_and_fetch('dir_removal.svndump',
                                            layout='single',
                                            subdir='dir1')
        self.assertEqual(sorted(repo['tip'].manifest().keys()),
                         ['1.txt', 'dir2/2.txt'])
        extra = repo['tip'].extra().copy()
        extra.pop('convert_revision', None)
        self.assertEqual(extra, {'branch': 'default'})
