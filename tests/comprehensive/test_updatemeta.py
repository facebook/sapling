import os
import pickle
import unittest

# wrapped in a try/except because of weirdness in how
# run.py works as compared to nose.
try:
    import test_util
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))
    import test_util

import test_rebuildmeta

from mercurial import context
from mercurial import extensions
from mercurial import hg
from mercurial import ui

from hgsubversion import svncommands
from hgsubversion import svnmeta



def _do_case(self, name, stupid, single):
    subdir = test_util.subdir.get(name, '')
    layout = 'auto'
    if single:
        layout = 'single'
    repo, repo_path = self.load_and_fetch(name, subdir=subdir, stupid=stupid,
                                          layout=layout)
    assert test_util.repolen(self.repo) > 0
    wc2_path = self.wc_path + '_clone'
    u = ui.ui()
    src, dest = test_util.hgclone(u, self.wc_path, wc2_path, update=False)
    src = test_util.getlocalpeer(src)
    dest = test_util.getlocalpeer(dest)

    # insert a wrapper that prevents calling changectx.children()
    def failfn(orig, ctx):
        self.fail('calling %s is forbidden; it can cause massive slowdowns '
                  'when rebuilding large repositories' % orig)

    origchildren = getattr(context.changectx, 'children')
    extensions.wrapfunction(context.changectx, 'children', failfn)

    # test updatemeta on an empty repo
    try:
        svncommands.updatemeta(u, dest,
                                args=[test_util.fileurl(repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    self._run_assertions(name, stupid, single, src, dest, u)


def _run_assertions(self, name, stupid, single, src, dest, u):
    test_rebuildmeta._run_assertions(self, name, stupid, single, src, dest, u)


skip = set([
    'project_root_not_repo_root.svndump',
    'corrupt.svndump',
])

attrs = {'_do_case': _do_case,
         '_run_assertions': _run_assertions,
         }
for case in [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]:
    # this fixture results in an empty repository, don't use it
    if case in skip:
        continue
    bname = 'test_' + case[:-len('.svndump')]
    attrs[bname] = test_rebuildmeta.buildmethod(case, bname, False, False)
    name = bname + '_stupid'
    attrs[name] = test_rebuildmeta.buildmethod(case, name, True, False)
    name = bname + '_single'
    attrs[name] = test_rebuildmeta.buildmethod(case, name, False, True)

UpdateMetaTests = type('UpdateMetaTests', (test_util.TestBase,), attrs)
