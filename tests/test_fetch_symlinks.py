import test_util

import unittest

class TestFetchSymlinks(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

    def test_symlinks(self, stupid=False):
        repo = self._load_fixture_and_fetch('symlinks.svndump', stupid)
        # Check no symlink contains the 'link ' prefix
        for rev in repo:
            r = repo[rev]
            for f in r.manifest():
                if 'l' not in r[f].flags():
                    continue
                self.assertFalse(r[f].data().startswith('link '))
        # Check symlinks in tip
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
            }
            
        for rev in repo:
            ctx = repo[rev]
            for f in ctx.manifest():
                self.assertEqual(f in links[rev], 'l' in ctx[f].flags())
                if f in links[rev]:
                    self.assertEqual(links[rev][f], ctx[f].data())
            for f in links[rev]:
                self.assertTrue(f in ctx)

    def test_symlinks_stupid(self):
        self.test_symlinks(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchSymlinks),
          ]
    return unittest.TestSuite(all)
