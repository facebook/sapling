import os
import sys
import tempfile
import unittest

from mercurial import context
from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import revlog

import fetch_command
import push_cmd
import test_util

class TestPushRenames(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir
        test_util.load_fixture_and_fetch('pushrenames.svndump',
                                         self.repo_path,
                                         self.wc_path,
                                         True)

    # define this as a property so that it reloads anytime we need it
    @property
    def repo(self):
        return hg.repository(ui.ui(), self.wc_path)

    def tearDown(self):
        test_util.rmtree(self.tmpdir)
        os.chdir(self.oldwd)

    def _commitchanges(self, repo, changes):
        parentctx = repo['tip']

        changed, removed = [], []
        for source, dest, newdata in changes:
            if dest is None:
                removed.append(source)
            else:
                changed.append(dest)

        def filectxfn(repo, memctx, path):
            if path in removed:
                raise IOError()
            entry = [e for e in changes if path == e[1]][0]
            source, dest, newdata = entry
            if newdata is None:
                newdata = parentctx[source].data()
            copied = None
            if source != dest:
                copied = source
            return context.memfilectx(path=dest,
                                      data=newdata,
                                      islink=False,
                                      isexec=False,
                                      copied=copied)
                
        ctx = context.memctx(repo,
                             (parentctx.node(), node.nullid),
                             'automated test',
                             changed + removed,
                             filectxfn,
                             'an_author',
                             '2008-10-07 20:59:48 -0500')
        return repo.commitctx(ctx)

    def _debug_print_copies(self, ctx):
        w = sys.stderr.write
        for f in ctx.files():
            if f not in ctx:
                w('R %s\n' % f)
            else:
                w('U %s %r\n' % (f, ctx[f].data()))
                if ctx[f].renamed():
                    w('%s copied from %s\n' % (f, ctx[f].renamed()[0]))

    def assertchanges(self, changes, ctx):
        for source, dest, data in changes:
            if dest is None:
                self.assertTrue(source not in ctx)
            else:
                self.assertTrue(dest in ctx)
                if data is None:
                    data = ctx.parents()[0][source].data()
                self.assertEqual(data, ctx[dest].data())
                if dest != source:
                    copy = ctx[dest].renamed()
                    self.assertEqual(copy[0], source)

    def test_push_renames(self, commit=True):
        repo = self.repo

        changes = [
            # Regular copy of a single file
            ('a', 'a2', None),
            # Copy and update of target
            ('a', 'a3', 'aa\n'),
            # Regular move of a single file
            ('b', 'b2', None),
            ('b', None, None),
            # Regular move and update of target
            ('c', 'c2', 'c\nc\n'),
            ('c', None, None),
            # Copy and update of source and targets
            ('d', 'd2', 'd\nd2\n'),
            ('d', 'd', 'd\nd\n'),
            # Double copy and removal (aka copy and move)
            ('e', 'e2', 'e\ne2\n'),
            ('e', 'e3', 'e\ne3\n'),
            ('e', None, None),
            ]
        self._commitchanges(repo, changes)
        
        hg.update(repo, repo['tip'].node())
        push_cmd.push_revisions_to_subversion(
            ui.ui(), repo=self.repo, hg_repo_path=self.wc_path,
            svn_url=test_util.fileurl(self.repo_path))
        tip = self.repo['tip']
        # self._debug_print_copies(tip)
        self.assertchanges(changes, tip)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestPushRenames),
          ]
    return unittest.TestSuite(all)
