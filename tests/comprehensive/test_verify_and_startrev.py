import os
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

from hgsubversion import verify

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
    'movetotrunk.svndump',
])

_skipstandard = set([
    'subdir_is_file_prefix.svndump',
    'correct.svndump',
    'corrupt.svndump',
    'emptyrepo2.svndump',
])

def _do_case(self, name, layout):
    subdir = test_util.subdir.get(name, '')
    config = {}
    for branch, path in test_util.custom.get(name, {}).iteritems():
        config['hgsubversionbranch.%s' % branch] = path
    repo, svnpath = self.load_and_fetch(name,
                                        subdir=subdir,
                                        layout=layout,
                                        config=config)
    assert test_util.repolen(self.repo) > 0
    for i in repo:
        ctx = repo[i]
        self.assertEqual(verify.verify(repo.ui, repo, rev=ctx.node(),
                                       stupid=True), 0)
        self.assertEqual(verify.verify(repo.ui, repo, rev=ctx.node(),
                                       stupid=False), 0)

    # check a startrev clone
    if layout == 'single' and name not in _skipshallow:
        self.wc_path += '_shallow'
        shallowrepo = self.fetch(svnpath, subdir=subdir,
                                 layout='single', startrev='HEAD')

        self.assertEqual(test_util.repolen(shallowrepo), 1,
                         "shallow clone should have just one revision, not %d"
                         % test_util.repolen(shallowrepo))

        fulltip = repo['tip']
        shallowtip = shallowrepo['tip']

        repo.ui.pushbuffer()
        self.assertEqual(0, verify.verify(repo.ui, shallowrepo,
                                          rev=shallowtip.node(),
                                          stupid=True))
        self.assertEqual(0, verify.verify(repo.ui, shallowrepo,
                                          rev=shallowtip.node(),
                                          stupid=False))

        stupidui = test_util.testui(stupid=True)
        self.assertEqual(verify.verify(stupidui, repo, rev=ctx.node(),
                                       stupid=True), 0)
        self.assertEqual(verify.verify(stupidui, repo, rev=ctx.node(),
                                       stupid=False), 0)

        # viewing diff's of lists of files is easier on the eyes
        self.assertMultiLineEqual('\n'.join(fulltip), '\n'.join(shallowtip),
                                  repo.ui.popbuffer())

        for f in fulltip:
            self.assertMultiLineEqual(fulltip[f].data(), shallowtip[f].data())


def buildmethod(case, name, layout):
    m = lambda self: self._do_case(case, layout)
    m.__name__ = name
    m.__doc__ = 'Test verify on %s (%s)' % (case, layout)
    return m

attrs = {'_do_case': _do_case, 'stupid_mode_tests': True}
fixtures = [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]
for case in fixtures:
    if case in _skipall:
        continue
    bname = 'test_' + case[:-len('.svndump')]
    if case not in _skipstandard:
        attrs[bname] = buildmethod(case, bname, 'standard')
    attrs[bname + '_single'] = buildmethod(case, bname + '_single', 'single')
    if case in test_util.custom:
        attrs[bname + '_custom'] = buildmethod(case, bname + '_custom', 'custom')

VerifyTests = type('VerifyTests', (test_util.TestBase,), attrs)
