import test_util

import sys
import unittest

class TestPushRenames(test_util.TestBase):
    def setUp(self):
        test_util.TestBase.setUp(self)
        self._load_fixture_and_fetch('pushrenames.svndump',
                                     stupid=True)

    def _debug_print_copies(self, ctx):
        w = sys.stderr.write
        for f in ctx.files():
            if f not in ctx:
                w('R %s\n' % f)
            else:
                w('U %s %r\n' % (f, ctx[f].data()))
                if ctx[f].renamed():
                    w('%s copied from %s\n' % (f, ctx[f].renamed()[0]))

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

    def test_push_rename_with_space(self):
        changes = [
            ('random/dir with space/file with space',
             'random/dir with space/file with space',
             'file contents'),
            ]
        self.commitchanges(changes)

        changes = [
            ('random/dir with space/file with space',
             'random2/dir with space/file with space',
             None),
            ('random/dir with space/file with space',
             None, None),
            ]
        self.commitchanges(changes)
        self.pushrevisions()
        self.assertEqual(self.repo['tip'].manifest().keys(),
                         ['a', 'c', 'b', 'e', 'd',
                          'random2/dir with space/file with space'])

    def test_push_rename_tree(self):
        repo = self.repo

        changes = [
            ('geek/alpha', 'geek/alpha', 'content',),
            ('geek/beta', 'geek/beta', 'content',),
            ('geek/delta', 'geek/delta', 'content',),
            ('geek/gamma', 'geek/gamma', 'content',),
            ('geek/later/pi', 'geek/later/pi', 'content geek/later/pi',),
            ('geek/later/rho', 'geek/later/rho', 'content geek/later/rho',),
            ('geek/other/blah', 'geek/other/blah', 'content geek/other/blah',),
            ('geek/other/another/layer', 'geek/other/another/layer', 'content deep file',),
            ]

        self.commitchanges(changes)
        self.pushrevisions()
        self.assertchanges(changes, self.repo['tip'])

        changes = [
            # rename (copy + remove) all of geek to greek
            ('geek/alpha', 'greek/alpha', None,),
            ('geek/beta', 'greek/beta', None,),
            ('geek/delta', 'greek/delta', None,),
            ('geek/gamma', 'greek/gamma', None,),
            ('geek/later/pi', 'greek/later/pi', None,),
            ('geek/later/rho', 'greek/later/rho', None,),
            ('geek/other/blah', 'greek/other/blah', None,),
            ('geek/other/another/layer', 'greek/other/another/layer', None,),

            ('geek/alpha', None, None,),
            ('geek/beta', None, None,),
            ('geek/delta', None, None,),
            ('geek/gamma', None, None,),
            ('geek/later/pi', None, None,),
            ('geek/later/rho', None, None,),
            ('geek/other/blah', None, None,),
            ('geek/other/another/layer', None, None,),
            ]
        self.commitchanges(changes)
        self.pushrevisions()
        # print '\n'.join(sorted(self.svnls('trunk')))
        assert reduce(lambda x, y: x and y,
                      ('geek' not in f for f in self.svnls('trunk'))), (
            'This failure means rename of an entire tree is broken.'
            ' There is a print on the preceding line commented out '
            'that should help you.')


def suite():
    all_tests = [unittest.TestLoader().loadTestsFromTestCase(TestPushRenames),
          ]
    return unittest.TestSuite(all_tests)
