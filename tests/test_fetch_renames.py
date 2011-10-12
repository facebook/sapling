import test_util

import sys
import unittest

class TestFetchRenames(test_util.TestBase):
    def _debug_print_copies(self, repo):
        w = sys.stderr.write
        for rev in repo:
            ctx = repo[rev]
            w('%d - %s\n' % (ctx.rev(), ctx.branch()))
            for f in ctx:
                fctx = ctx[f]
                w('%s: %r %r\n' % (f, fctx.data(), fctx.renamed()))

    def _test_rename(self, stupid):
        repo = self._load_fixture_and_fetch('renames.svndump', stupid=stupid)
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
                },
            9: {
                'unchanged2': ('unchanged', 'unchanged\n'),
                'unchangeddir2/f': ('unchangeddir/f', 'unchanged2\n'),
                },
            10: {
                 'groupdir2/b': ('groupdir/b', 'b\n')
                 },
            }
        for rev in repo:
            ctx = repo[rev]
            copymap = copies.get(rev, {})
            for f in ctx.manifest():
                cp = ctx[f].renamed()
                self.assertEqual(bool(cp), bool(copymap.get(f)),
                                 'copy records differ for %s in %d' % (f, rev))
                if not cp:
                    continue
                self.assertEqual(cp[0], copymap[f][0])
                self.assertEqual(ctx[f].data(), copymap[f][1])

        self.assertEqual(repo['tip']['changed3'].data(), 'changed\nchanged3\n')

    def test_rename(self):
        self._test_rename(False)

    def test_rename_stupid(self):
        self._test_rename(True)

    def _test_case(self, stupid):
        repo = self._load_fixture_and_fetch('filecase.svndump', stupid=stupid)
        files = {
            0: ['A', 'a', 'e/a', 'b', 'd/a', 'D/a', 'f/a', 'F'],
            1: ['A', 'a', 'E/a', 'B', 'd/A', 'D/a', 'f/a', 'F'],
            }
        for rev in repo:
            self.assertEqual(sorted(files[rev]), sorted(repo[rev].manifest()))

    def test_case(self):
        self._test_case(False)

    def test_case_stupid(self):
        self._test_case(True)

def suite():
    all_tests = [unittest.TestLoader().loadTestsFromTestCase(TestFetchRenames),
          ]
    return unittest.TestSuite(all_tests)
