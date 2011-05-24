import test_util

import os.path
import subprocess
from mercurial import ui
from mercurial import util as hgutil
from mercurial import commands

class TestPull(test_util.TestBase):
    def setUp(self):
        super(TestPull, self).setUp()

    def _load_fixture_and_fetch(self, fixture_name):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=False,
                                                noupdate=False)

    def test_nochanges(self):
        self._load_fixture_and_fetch('single_rev.svndump')
        state = self.repo.parents()
        commands.pull(self.repo.ui, self.repo)
        self.assertEqual(state, self.repo.parents())

    def test_onerevision_noupdate(self):
        repo = self._load_fixture_and_fetch('single_rev.svndump')
        state = repo.parents()
        self._add_svn_rev({'trunk/alpha': 'Changed'})
        commands.pull(self.repo.ui, repo)
        self.assertEqual(state, repo.parents())
        self.assertTrue('tip' not in repo[None].tags())

    def test_onerevision_doupdate(self):
        repo = self._load_fixture_and_fetch('single_rev.svndump')
        state = repo.parents()
        self._add_svn_rev({'trunk/alpha': 'Changed'})
        commands.pull(self.repo.ui, repo, update=True)
        self.failIfEqual(state, repo.parents())
        self.assertTrue('tip' in repo[None].tags())

    def test_onerevision_divergent(self):
        repo = self._load_fixture_and_fetch('single_rev.svndump')
        self.commitchanges((('alpha', 'alpha', 'Changed another way'),))
        state = repo.parents()
        self._add_svn_rev({'trunk/alpha': 'Changed one way'})
        self.assertRaises(hgutil.Abort, commands.pull,
                          self.repo.ui, repo, update=True)
        self.assertEqual(state, repo.parents())
        self.assertTrue('tip' not in repo[None].tags())
        self.assertEqual(len(repo.heads()), 2)

def suite():
    import unittest, sys
    return unittest.findTestCases(sys.modules[__name__])
