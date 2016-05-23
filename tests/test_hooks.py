import sys
import test_util
import unittest

from mercurial import hg
from mercurial import commands

class TestHooks(test_util.TestBase):
    def setUp(self):
        super(TestHooks, self).setUp()

    def _loadupdate(self, fixture_name, *args, **kwargs):
        kwargs = kwargs.copy()
        kwargs.update(noupdate=False)
        repo, repo_path = self.load_and_fetch(fixture_name, *args, **kwargs)
        return repo, repo_path

    def test_updatemetahook(self):
        repo, repo_path = self._loadupdate('single_rev.svndump')
        state = repo[None].parents()
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed'})
        commands.pull(self.repo.ui, self.repo)

        # Clone to a new repository and add a hook
        new_wc_path = "%s-2" % self.wc_path
        commands.clone(self.repo.ui, self.wc_path, new_wc_path)
        newrepo = hg.repository(test_util.testui(), new_wc_path)
        newrepo.ui.setconfig('hooks', 'changegroup.meta',
                'python:hgsubversion.hooks.updatemeta.hook')

        # Commit a rev that should trigger svn meta update
        self.add_svn_rev(repo_path, {'trunk/alpha': 'Changed Again'})
        commands.pull(self.repo.ui, self.repo)

        self.called = False
        import hgsubversion.svncommands
        oldupdatemeta = hgsubversion.svncommands.updatemeta
        def _updatemeta(ui, repo, args=[]):
            self.called = True
        hgsubversion.svncommands.updatemeta = _updatemeta

        # Pull and make sure our updatemeta function gets called
        commands.pull(newrepo.ui, newrepo)
        hgsubversion.svncommands.updatemeta = oldupdatemeta
        self.assertTrue(self.called)
