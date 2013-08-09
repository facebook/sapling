import test_util

import unittest

from mercurial import node

class TestFetchExec(test_util.TestBase):
    stupid_mode_tests = True

    def assertexec(self, ctx, files, isexec=True):
        for f in files:
            self.assertEqual(isexec, 'x' in ctx[f].flags())

    def test_exec(self):
        repo = self._load_fixture_and_fetch('executebit.svndump')
        self.assertexec(repo[0], ['text1', 'binary1', 'empty1'], True)
        self.assertexec(repo[0], ['text2', 'binary2', 'empty2'], False)
        self.assertexec(repo[1], ['text1', 'binary1', 'empty1'], False)
        self.assertexec(repo[1], ['text2', 'binary2', 'empty2'], True)

    def test_empty_prop_val_executable(self):
        repo = self._load_fixture_and_fetch('executable_file_empty_prop.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '08e6b380bf291b361a418203a1cb9427213cd1fd')
        self.assertEqual(repo['tip']['foo'].flags(), 'x')
