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

import test_rebuildmeta

from mercurial import context
from mercurial import extensions
from mercurial import hg
from mercurial import ui

from hgsubversion import svncommands
from hgsubversion import svnmeta



def _do_case(self, name, layout):
    subdir = test_util.subdir.get(name, '')
    single = layout == 'single'
    u = ui.ui()
    config = {}
    if layout == 'custom':
        config['hgsubversion.layout'] = 'custom'
        u.setconfig('hgsubversion', 'layout', 'custom')
        for branch, path in test_util.custom.get(name, {}).iteritems():
            config['hgsubversionbranch.%s' % branch] = path
            u.setconfig('hgsubversionbranch', branch, path)

    repo, repo_path = self.load_and_fetch(name,
                                          subdir=subdir,
                                          layout=layout,
                                          config=config)
    assert test_util.repolen(self.repo) > 0
    wc2_path = self.wc_path + '_clone'
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

    self._run_assertions(name, single, src, dest, u)


def _run_assertions(self, name, single, src, dest, u):
    test_rebuildmeta._run_assertions(self, name, single, src, dest, u)


skip = set([
    'project_root_not_repo_root.svndump',
    'corrupt.svndump',
])

attrs = {'_do_case': _do_case,
         '_run_assertions': _run_assertions,
         'stupid_mode_tests': True,
         }
for case in [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]:
    # this fixture results in an empty repository, don't use it
    if case in skip:
        continue
    bname = 'test_' + case[:-len('.svndump')]
    attrs[bname] = test_rebuildmeta.buildmethod(case, bname, 'auto')
    attrs[bname + '_single'] = test_rebuildmeta.buildmethod(case,
                                                            bname + '_single',
                                                            'single')
    if case in test_util.custom:
        attrs[bname + '_custom'] = test_rebuildmeta.buildmethod(case,
                                                                bname + '_custom',
                                                                'custom')


UpdateMetaTests = type('UpdateMetaTests', (test_util.TestBase,), attrs)
