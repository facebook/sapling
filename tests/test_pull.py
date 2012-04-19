import test_util

import os.path
import subprocess
from mercurial import node
from mercurial import ui
from mercurial import util as hgutil
from mercurial import commands

class TestPull(test_util.TestBase):
    def setUp(self):
        super(TestPull, self).setUp()

    def _loadupdate(self, fixture_name):
        repo, repo_path = self.load_and_fetch(fixture_name, stupid=False,
                                              noupdate=False)
        return repo, repo_path

    def test_nochanges(self):
        self._loadupdate('single_rev.svndump')
        state = self.repo.parents()
        commands.pull(self.repo.ui, self.repo)
        self.assertEqual(state, self.repo.parents())

    def test_onerevision_noupdate(self):
        repo, repo_path = self._loadupdate('single_rev.svndump')
        state = repo.parents()
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        commands.pull(self.repo.ui, repo)
        self.assertEqual(state, repo.parents())
        self.assertTrue('tip' not in repo[None].tags())

    def test_onerevision_doupdate(self):
        repo, repo_path = self._loadupdate('single_rev.svndump')
        state = repo.parents()
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        commands.pull(self.repo.ui, repo, update=True)
        self.failIfEqual(state, repo.parents())
        self.assertTrue('tip' in repo[None].tags())

    def test_onerevision_divergent(self):
        repo, repo_path = self._loadupdate('single_rev.svndump')
        self.commitchanges((('alpha', 'alpha', 'Changed another way'),))
        state = repo.parents()
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed one way'})
        try:
            commands.pull(self.repo.ui, repo, update=True)
        except hgutil.Abort:
            # hg < 1.9 raised when crossing branches
            pass
        self.assertEqual(state, repo.parents())
        self.assertTrue('tip' not in repo[None].tags())
        self.assertEqual(len(repo.heads()), 2)

    def test_tag_repull_doesnt_happen(self):
        repo = self._loadupdate('branchtagcollision.svndump')[0]
        oldheads = map(node.hex, repo.heads())
        commands.pull(repo.ui, repo)
        self.assertEqual(oldheads, map(node.hex, repo.heads()))

def suite():
    import unittest, sys
    return unittest.findTestCases(sys.modules[__name__])
