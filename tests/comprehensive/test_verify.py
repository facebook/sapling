import os
import pickle
import unittest

import test_util

from mercurial import hg
from mercurial import ui

from hgsubversion import svncommands

def _do_case(self, name, stupid):
    subdir = test_util.subdir.get(name, '')
    repo = self._load_fixture_and_fetch(name, subdir=subdir, stupid=stupid)
    assert len(self.repo) > 0
    for i in repo:
        ctx = repo[i]
        hg.clean(repo, ctx.node(), False)
        self.assertEqual(svncommands.verify(repo.ui, repo), 0)

def buildmethod(case, name, stupid):
    m = lambda self: self._do_case(case, stupid)
    m.__name__ = name
    bits = case, stupid and 'stupid' or 'real'
    m.__doc__ = 'Test verify on %s with %s replay.' % bits
    return m

attrs = {'_do_case': _do_case}
fixtures = [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]
for case in fixtures:
    # this fixture results in an empty repository, don't use it
    if case == 'project_root_not_repo_root.svndump':
        continue
    name = 'test_' + case[:-len('.svndump')]
    attrs[name] = buildmethod(case, name, False)
    name += '_stupid'
    attrs[name] = buildmethod(case, name, True)

VerifyTests = type('VerifyTests', (test_util.TestBase,), attrs)

def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(VerifyTests)]
    return unittest.TestSuite(all)
