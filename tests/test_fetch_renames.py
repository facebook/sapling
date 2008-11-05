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


class TestFetchRenames(unittest.TestCase):
    def setUp(self):
        self.oldwd = os.getcwd()
        self.tmpdir = tempfile.mkdtemp('svnwrap_test')
        self.repo_path = '%s/testrepo' % self.tmpdir
        self.wc_path = '%s/testrepo_wc' % self.tmpdir

    def tearDown(self):
        shutil.rmtree(self.tmpdir)
        os.chdir(self.oldwd)

    def _load_fixture_and_fetch(self, fixture_name):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path)

    def _debug_print_copies(self, repo):
        w = sys.stderr.write
        for rev in repo:
            ctx = repo[rev]
            w('%d - %s\n' % (ctx.rev(), ctx.branch()))
            for f in ctx:
                fctx = ctx[f]
                w('%s: %r %r\n' % (f, fctx.data(), fctx.renamed()))

    def test_rename(self):
        repo = self._load_fixture_and_fetch('renames.svndump')
        # self._debug_print_copies(repo)

        # Map revnum to mappings of dest name to (source name, dest content)
        copies = {
            4: {
                'a1': ('a', 'a\n'), 
                'a2': ('a', 'a\n'),
                'b1': ('b', 'b\nc\n'),
                'da1/daf': ('da/daf', 'c\n'),
                'da1/db/dbf': ('da/db/dbf', 'd\n'),
                'da2/daf': ('da/daf', 'c\n'),
                'da2/db/dbf': ('da/db/dbf', 'd\n'),
                },
            5: {
                'c1': ('c', 'c\nc\n'),
                }
            }
        for rev in repo:
            ctx = repo[rev]
            copymap = copies.get(rev, {})
            for f in ctx.manifest():
                cp = ctx[f].renamed()
                self.assertEqual(bool(cp), bool(copymap.get(f)))
                if not cp:
                    continue
                self.assertEqual(cp[0], copymap[f][0])
                self.assertEqual(ctx[f].data(), copymap[f][1])

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchRenames),
          ]
    return unittest.TestSuite(all)
