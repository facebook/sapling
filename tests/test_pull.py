import test_util

import os.path
import subprocess
from mercurial import node
from mercurial import ui
from mercurial import util as hgutil
from mercurial import commands
from hgsubversion import verify

class TestPull(test_util.TestBase):
    def setUp(self):
        super(TestPull, self).setUp()

    def _loadupdate(self, fixture_name, *args, **kwargs):
        kwargs = kwargs.copy()
        kwargs.update(noupdate=False)
        repo, repo_path = self.load_and_fetch(fixture_name, *args, **kwargs)
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
        self.assertTrue('tip' not in repo['.'].tags())

    def test_onerevision_doupdate(self):
        repo, repo_path = self._loadupdate('single_rev.svndump')
        state = repo.parents()
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        commands.pull(self.repo.ui, repo, update=True)
        self.failIfEqual(state, repo.parents())
        self.assertTrue('tip' in repo['.'].tags())

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
        self.assertTrue('tip' not in repo['.'].tags())
        self.assertEqual(len(repo.heads()), 2)

    def test_tag_repull_doesnt_happen(self):
        repo = self._loadupdate('branchtagcollision.svndump')[0]
        oldheads = map(node.hex, repo.heads())
        commands.pull(repo.ui, repo)
        self.assertEqual(oldheads, map(node.hex, repo.heads()))

    def test_pull_with_secret_default(self):
        repo = self._loadupdate('branchtagcollision.svndump',
                                config={'phases.new-commit': 'secret'})[0]
        oldheads = map(node.hex, repo.heads())
        commands.pull(repo.ui, repo)
        self.assertEqual(oldheads, map(node.hex, repo.heads()))

    def test_skip_basic(self):
        repo, repo_path = self._loadupdate('single_rev.svndump')
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        self.add_svn_rev(repo_path, {'trunk/beta': 'More changed'})
        self.add_svn_rev(repo_path, {'trunk/gamma': 'Even more changeder'})
        repo.ui.setconfig('hgsubversion', 'unsafeskip', '3 4')
        commands.pull(repo.ui, repo)
        tip = repo['tip'].rev()
        self.assertEqual(tip, 1)
        self.assertEquals(verify.verify(repo.ui, repo, rev=tip), 1)

    def test_skip_delete_restore(self):
        repo, repo_path = self._loadupdate('delete_restore_trunk.svndump',
                                           rev=2)
        repo.ui.setconfig('hgsubversion', 'unsafeskip', '3 4')
        commands.pull(repo.ui, repo)
        tip = repo['tip'].rev()
        self.assertEqual(tip, 1)
        self.assertEquals(verify.verify(repo.ui, repo, rev=tip), 0)
