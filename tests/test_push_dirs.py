import os
import sys
import unittest

from mercurial import context
from mercurial import hg
from mercurial import node

import test_util

class TestPushDirectories(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        test_util.load_fixture_and_fetch('emptyrepo.svndump',
                                         self.repo_path,
                                         self.wc_path)

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
        nodeid = repo.commitctx(ctx)
        repo = self.repo
        hg.update(repo, nodeid)
        return nodeid

    def test_push_dirs(self, commit=True):
        changes = [
            # Single file in single directory
            ('d1/a', 'd1/a', 'a\n'),
            # Two files in one directory
            ('d2/a', 'd2/a', 'a\n'),
            ('d2/b', 'd2/b', 'a\n'),
            # Single file in empty directory hierarchy
            ('d31/d32/d33/d34/a', 'd31/d32/d33/d34/a', 'a\n'),
            ('d31/d32/a', 'd31/d32/a', 'a\n'),
            ]
        self._commitchanges(self.repo, changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('trunk'), 
                          ['d1', 'd1/a', 'd2', 'd2/a', 'd2/b', 'd31', 
                           'd31/d32', 'd31/d32/a', 'd31/d32/d33', 
                           'd31/d32/d33/d34', 'd31/d32/d33/d34/a'])

        changes = [
            # Remove single file in single directory
            ('d1/a', None, None),
            # Remove one file out of two
            ('d2/a', None, None),
            # Removing this file should remove one empty parent dir too
            ('d31/d32/d33/d34/a', None, None),
            ]
        self._commitchanges(self.repo, changes)
        self.pushrevisions()
        self.assertEqual(self.svnls('trunk'), 
                         ['d2', 'd2/b', 'd31', 'd31/d32', 'd31/d32/a', 'd31/d32/d33'])

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestPushDirectories),
          ]
    return unittest.TestSuite(all)
