import test_util

import os
import unittest

from mercurial import commands
from mercurial import dispatch
from mercurial import error
from mercurial import hg
from mercurial import node
from mercurial import ui

def _dispatch(ui, cmd):
    assert '--quiet' in cmd
    try:
        req = dispatch.request(cmd, ui=ui)
        req.earlyoptions = {'config': [], 'cwd': '', 'debugger': None,
                            'profile': False, 'repository': ''}
        dispatch._dispatch(req)
    except AttributeError:
        dispatch._dispatch(ui, cmd)

class TestMercurialCore(test_util.TestBase):
    '''
    Test that the core Mercurial operations aren't broken by hgsubversion.
    '''

    @test_util.requiresoption('updaterev')
    def test_update(self):
        ''' Test 'clone --updaterev' '''
        ui = self.ui()
        _dispatch(ui, ['init', '--quiet', self.wc_path])
        repo = self.repo
        repo.ui.setconfig('ui', 'username', 'anonymous')

        fpath = os.path.join(self.wc_path, 'it')
        f = file(fpath, 'w')
        f.write('C1')
        f.flush()
        commands.add(ui, repo)
        commands.commit(ui, repo, message="C1")
        f.write('C2')
        f.flush()
        commands.commit(ui, repo, message="C2")
        f.write('C3')
        f.flush()
        commands.commit(ui, repo, message="C3")

        self.assertEqual(test_util.repolen(repo), 3)

        updaterev = 1
        _dispatch(ui, ['clone', '--quiet',
                       self.wc_path, self.wc_path + '2',
                       '--updaterev=%s' % updaterev])

        repo2 = hg.repository(ui, self.wc_path + '2')

        self.assertEqual(str(repo[updaterev]), str(repo2['.']))

    @test_util.requiresoption('branch')
    def test_branch(self):
        ''' Test 'clone --branch' '''
        ui = self.ui()
        _dispatch(ui, ['init', '--quiet', self.wc_path])
        repo = self.repo
        repo.ui.setconfig('ui', 'username', 'anonymous')

        fpath = os.path.join(self.wc_path, 'it')
        f = file(fpath, 'w')
        f.write('C1')
        f.flush()
        commands.add(ui, repo)
        commands.branch(ui, repo, label="B1")
        commands.commit(ui, repo, message="C1")
        f.write('C2')
        f.flush()
        commands.branch(ui, repo, label="default")
        commands.commit(ui, repo, message="C2")
        f.write('C3')
        f.flush()
        commands.branch(ui, repo, label="B2")
        commands.commit(ui, repo, message="C3")

        self.assertEqual(test_util.repolen(repo), 3)

        branch = 'B1'
        _dispatch(ui, [
            'clone', '--quiet',
            self.wc_path, self.wc_path + '2',
            '--branch', branch])

        repo2 = hg.repository(ui, self.wc_path + '2')

        self.assertEqual(repo[branch].hex(), repo2['.'].hex())
