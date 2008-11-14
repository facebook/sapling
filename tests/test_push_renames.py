import sys
import unittest

import test_util

class TestPushRenames(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        test_util.load_fixture_and_fetch('pushrenames.svndump',
                                         self.repo_path,
                                         self.wc_path,
                                         True)

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

    def test_push_renames(self):
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
        self.commitchanges(changes)
        self.pushrevisions()
        tip = self.repo['tip']
        # self._debug_print_copies(tip)
        self.assertchanges(changes, tip)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestPushRenames),
          ]
    return unittest.TestSuite(all)
