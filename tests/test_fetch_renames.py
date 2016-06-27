import test_util

import sys
import unittest

class TestFetchRenames(test_util.TestBase):
    stupid_mode_tests = True

    def _debug_print_copies(self, repo):
        w = sys.stderr.write
        for rev in repo:
            ctx = repo[rev]
            w('%d - %s\n' % (ctx.rev(), ctx.branch()))
            for f in ctx:
                fctx = ctx[f]
                w('%s: %r %r\n' % (f, fctx.data(), fctx.renamed()))

    def test_rename(self):
        config = {
            'hgsubversion.filestoresize': '0',
            # we set this because we expect all of the copies to be
            # handled via replay, and we want to notice if that
            # changes.
            'hgsubversion.failonmissing': 'yes',
            }
        repo = self._load_fixture_and_fetch('renames.svndump', config=config)
        self._run_assertions(repo)

    def test_rename_with_prefix(self):
        config = {
            'hgsubversion.filestoresize': '0',
            'hgsubversion.failonmissing': 'yes',
            }
        repo = self._load_fixture_and_fetch('renames_with_prefix.svndump',
                                            subdir='prefix',
                                            config=config)
        self._run_assertions(repo)

    def _run_assertions(self, repo):
        # Map revnum to mappings of dest name to (source name, dest content)
        copies = {
            4: {
                'a1': ('a', 'a\n'),
                'linka1': ('linka', 'a'),
                'a2': ('a', 'a\n'),
                'linka2': ('linka', 'a'),
                'b1': ('b', 'b\nc\n'),
                'linkb1': ('linkb', 'bc'),
                'da1/daf': ('da/daf', 'c\n'),
                'da1/dalink': ('da/dalink', 'daf'),
                'da1/db/dbf': ('da/db/dbf', 'd\n'),
                'da1/db/dblink': ('da/db/dblink', '../daf'),
                'da2/daf': ('da/daf', 'c\n'),
                'da2/dalink': ('da/dalink', 'daf'),
                'da2/db/dbf': ('da/db/dbf', 'd\n'),
                'da2/db/dblink': ('da/db/dblink', '../daf'),
                },
            5: {
                'c1': ('c', 'c\nc\n'),
                'linkc1': ('linkc', 'cc'),
                },
            9: {
                'unchanged2': ('unchanged', 'unchanged\n'),
                'unchangedlink2': ('unchangedlink', 'unchanged'),
                'unchangeddir2/f': ('unchangeddir/f', 'unchanged2\n'),
                'unchangeddir2/link': ('unchangeddir/link', 'f'),
                },
            10: {
                'groupdir2/b': ('groupdir/b', 'b\n'),
                'groupdir2/linkb': ('groupdir/linkb', 'b'),
                 },
            }
        for rev in repo:
            ctx = repo[rev]
            copymap = copies.get(rev, {})
            for f in ctx.manifest():
                cp = ctx[f].renamed()
                want = copymap.get(f)
                self.assertEqual(
                    bool(cp), bool(want),
                    'copy records differ for %s in %d (want %r, got %r)' % (
                        f, rev, want, cp))
                if not cp:
                    continue
                self.assertEqual(cp[0], want[0])
                self.assertEqual(ctx[f].data(), want[1])

        self.assertEqual(repo['tip']['changed3'].data(), 'changed\nchanged3\n')

    def test_case(self):
        repo = self._load_fixture_and_fetch('filecase.svndump')
        files = {
            0: ['A', 'a', 'e/a', 'b', 'd/a', 'D/a', 'f/a', 'F'],
            1: ['A', 'a', 'E/a', 'B', 'd/A', 'D/a', 'f/a', 'F'],
            }
        for rev in repo:
            self.assertEqual(sorted(files[rev]), sorted(repo[rev].manifest()))
