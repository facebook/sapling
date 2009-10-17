import os
import pickle
import unittest

from mercurial import hg
from mercurial import ui

from tests import test_util
from hgsubversion import wrappers


def _do_case(self, name, layout):
    subdir = test_util.subdir.get(name, '')
    self._load_fixture_and_fetch(name, subdir=subdir, stupid=False, layout=layout)
    assert len(self.repo) > 0, 'Repo had no changes, maybe you need to add a subdir entry in test_util?'
    wc2_path = self.wc_path + '_stupid'
    u = ui.ui()
    checkout_path = self.repo_path
    if subdir:
        checkout_path += '/' + subdir
    u.setconfig('hgsubversion', 'stupid', '1')
    u.setconfig('hgsubversion', 'layout', layout)
    hg.clone(u, test_util.fileurl(checkout_path), wc2_path, update=False)
    if layout == 'single':
        self.assertEqual(len(self.repo.heads()), 1)
    self.repo2 = hg.repository(ui.ui(), wc2_path)
    self.assertEqual(self.repo.heads(), self.repo2.heads())


def buildmethod(case, name, layout):
    m = lambda self: self._do_case(case, layout)
    m.__name__ = name
    m.__doc__ = 'Test stupid produces same as real on %s. (%s)' % (case, layout)
    return m

attrs = {'_do_case': _do_case,
         }
for case in (f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')):
    name = 'test_' + case[:-len('.svndump')]
    attrs[name] = buildmethod(case, name, 'auto')
    name += '_single'
    attrs[name] = buildmethod(case, name, 'single')

StupidPullTests = type('StupidPullTests', (test_util.TestBase, ), attrs)


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(StupidPullTests),
          ]
    return unittest.TestSuite(all)
