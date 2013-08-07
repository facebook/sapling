import test_util

import unittest

class TestFetchSymlinks(test_util.TestBase):
    def test_symlinks(self, stupid=False):
        repo = self._load_fixture_and_fetch('symlinks.svndump', stupid=stupid)
        # Check symlinks throughout history
        links = {
            0: {
                'linka': 'a',
                'linka2': 'a',
                'd/linka': 'a',
                },
            1: {
                'linkaa': 'a',
                'linka2': 'a',
                'd2/linka': 'a',
                },
            2: {
                'linkaa': 'b',
                'linka2': 'a',
                'd2/linka': 'b',
                },
            3: {
                },
            4: {
                'linka3': 'a',
                },
            5: {
                'linka3': 'a',
                },
            6: {
                'linka3': 'a',
                'linka4': 'link to this',
                },
            }

        for rev in repo:
            ctx = repo[rev]
            for f in ctx.manifest():
                l = 'l' in ctx[f].flags()
                lref = f in links[rev]
                self.assertEqual(lref, l, '%r != %r for %s@%r' % (lref, l, f, rev))
                if f in links[rev]:
                    self.assertEqual(links[rev][f], ctx[f].data())
            for f in links[rev]:
                self.assertTrue(f in ctx)

    def test_symlinks_stupid(self):
        self.test_symlinks(True)

class TestMergeSpecial(test_util.TestBase):
    def test_special(self):
        repo = self._load_fixture_and_fetch('addspecial.svndump',
                                            subdir='trunk')
        ctx = repo['tip']
        self.assertEqual(ctx['fnord'].flags(), 'l')
        self.assertEqual(ctx['exe'].flags(), 'x')
