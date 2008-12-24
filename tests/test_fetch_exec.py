import unittest

from mercurial import node

import test_util

class TestFetchExec(test_util.TestBase):
    def _load_fixture_and_fetch(self, fixture_name, stupid):
        return test_util.load_fixture_and_fetch(fixture_name, self.repo_path,
                                                self.wc_path, stupid=stupid)

    def assertexec(self, ctx, files, isexec=True):
        for f in files:
            self.assertEqual(isexec, 'x' in ctx[f].flags())

    def test_exec(self, stupid=False):
        repo = self._load_fixture_and_fetch('executebit.svndump', stupid)
        self.assertexec(repo[0], ['text1', 'binary1', 'empty1'], True)
        self.assertexec(repo[0], ['text2', 'binary2', 'empty2'], False)
        self.assertexec(repo[1], ['text1', 'binary1', 'empty1'], False)
        self.assertexec(repo[1], ['text2', 'binary2', 'empty2'], True)

    def test_exec_stupid(self):
        self.test_exec(True)

    def test_empty_prop_val_executable(self, stupid=False):
        repo = self._load_fixture_and_fetch('executable_file_empty_prop.svndump', stupid)
        self.assertEqual(node.hex(repo['tip'].node()),
                         '08e6b380bf291b361a418203a1cb9427213cd1fd')
        self.assertEqual(repo['tip']['foo'].flags(), 'x')

    def test_empty_prop_val_executable_stupid(self):
        self.test_empty_prop_val_executable(True)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(TestFetchExec),
          ]
    return unittest.TestSuite(all)
