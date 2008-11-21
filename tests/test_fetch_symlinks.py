import os
import shutil
import sys
import tempfile
import unittest

from mercurial import hg
from mercurial import ui
from mercurial import node

import fetch_command
import test_util


class TestFetchSymlinks(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

    def test_symlinks(self, stupid=False):
        repo = self._load_fixture_and_fetch('symlinks.svndump', stupid)
        # Check no symlink contains the 'link ' prefix
        for rev in repo:
            r = repo[rev]
            for f in r.manifest():
                if 'l' not in r[f].flags():
                    continue
                self.assertFalse(r[f].data().startswith('link '))
        # Check symlinks in tip
        tip = repo['tip']
        links = {
            'linkaa': 'b',
            'd2/linka': 'b',
            }
        for f in tip.manifest():
            self.assertEqual(f in links, 'l' in tip[f].flags())
            if f in links:
                self.assertEqual(links[f], tip[f].data())

    def test_symlinks_stupid(self):
        self.test_symlinks(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchSymlinks),
          ]
    return unittest.TestSuite(all)
