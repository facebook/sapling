import os
import pickle
import sys
import unittest

# wrapped in a try/except because of weirdness in how
# run.py works as compared to nose.
try:
    import test_util
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))
    import test_util

from mercurial import hg
from mercurial import ui

from hgsubversion import svncommands

# these fixtures contain no files at HEAD and would result in empty clones
_skipshallow = set([
    'binaryfiles.svndump',
    'binaryfiles-broken.svndump',
    'emptyrepo.svndump',
    'correct.svndump',
    'corrupt.svndump',
])

_skipall = set([
    'project_root_not_repo_root.svndump',
])

_skipstandard = set([
    'subdir_is_file_prefix.svndump',
    'correct.svndump',
    'corrupt.svndump',
])

def _do_case(self, name, stupid, layout):
    subdir = test_util.subdir.get(name, '')
    repo, svnpath = self.load_and_fetch(name, subdir=subdir, stupid=stupid,
                                        layout=layout)
    assert len(self.repo) > 0
    for i in repo:
        ctx = repo[i]
        self.assertEqual(svncommands.verify(repo.ui, repo, rev=ctx.node()), 0)

    # check a startrev clone
    if layout == 'single' and name not in _skipshallow:
        self.wc_path += '_shallow'
        shallowrepo = self.fetch(svnpath, subdir=subdir, stupid=stupid,
                                 layout='single', startrev='HEAD')

        self.assertEqual(len(shallowrepo), 1,
                         "shallow clone should have just one revision, not %d"
                         % len(shallowrepo))

        fulltip = repo['tip']
        shallowtip = shallowrepo['tip']

        repo.ui.pushbuffer()
        self.assertEqual(0, svncommands.verify(repo.ui, shallowrepo,
                                               rev=shallowtip.node()))

        # viewing diff's of lists of files is easier on the eyes
        self.assertMultiLineEqual('\n'.join(fulltip), '\n'.join(shallowtip),
                                  repo.ui.popbuffer())

        for f in fulltip:
            self.assertMultiLineEqual(fulltip[f].data(), shallowtip[f].data())


def buildmethod(case, name, stupid, layout):
    m = lambda self: self._do_case(case, stupid, layout)
    m.__name__ = name
    bits = case, stupid and 'stupid' or 'real', layout
    m.__doc__ = 'Test verify on %s with %s replay. (%s)' % bits
    return m

attrs = {'_do_case': _do_case}
fixtures = [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]
for case in fixtures:
    if case in _skipall:
        continue
    bname = 'test_' + case[:-len('.svndump')]
    if case not in _skipstandard:
        attrs[bname] = buildmethod(case, bname, False, 'standard')
        name = bname + '_stupid'
        attrs[name] = buildmethod(case, name, True, 'standard')
    name = bname + '_single'
    attrs[name] = buildmethod(case, name, False, 'single')
    # Disabled because the "stupid and real are the same" tests
    # verify this plus even more.
    # name = bname + '_single_stupid'
    # attrs[name] = buildmethod(case, name, True, 'single')

VerifyTests = type('VerifyTests', (test_util.TestBase,), attrs)

def suite():
    all_tests = [unittest.TestLoader().loadTestsFromTestCase(VerifyTests)]
    return unittest.TestSuite(all_tests)
