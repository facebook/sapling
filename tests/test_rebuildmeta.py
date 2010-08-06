import test_util

import os
import pickle
import unittest

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
    self._load_fixture_and_fetch(name, subdir=subdir, stupid=stupid, layout=layout)
    assert len(self.repo) > 0
    wc2_path = self.wc_path + '_clone'
    u = ui.ui()
    src, dest = hg.clone(u, self.wc_path, wc2_path, update=False)

    # insert a wrapper that prevents calling changectx.children()
    def failfn(orig, ctx):
        self.fail('calling %s is forbidden; it can cause massive slowdowns '
                  'when rebuilding large repositories' % orig)

    origchildren = getattr(context.changectx, 'children')
    extensions.wrapfunction(context.changectx, 'children', failfn)

    try:
        svncommands.rebuildmeta(u, dest,
                                args=[test_util.fileurl(self.repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    self.assertTrue(os.path.isdir(os.path.join(src.path, 'svn')),
                    'no .hg/svn directory in the source!')
    self.assertTrue(os.path.isdir(os.path.join(src.path, 'svn')),
                    'no .hg/svn directory in the destination!')
    dest = hg.repository(u, os.path.dirname(dest.path))
    for tf in ('rev_map', 'uuid', 'tagmap', 'layout', ):
        stf = os.path.join(src.path, 'svn', tf)
        self.assertTrue(os.path.isfile(stf), '%r is missing!' % stf)
        dtf = os.path.join(dest.path, 'svn', tf)
        self.assertTrue(os.path.isfile(dtf), '%r is missing!' % tf)
        old, new = open(stf).read(), open(dtf).read()
        self.assertMultiLineEqual(old, new)
        self.assertEqual(src.branchtags(), dest.branchtags())
    srcbi = pickle.load(open(os.path.join(src.path, 'svn', 'branch_info')))
    destbi = pickle.load(open(os.path.join(dest.path, 'svn', 'branch_info')))
    self.assertEqual(sorted(srcbi.keys()), sorted(destbi.keys()))
    revkeys = svnmeta.SVNMeta(dest).revmap.keys()
    for branch in destbi:
        srcinfo = srcbi[branch]
        destinfo = destbi[branch]
        if srcinfo[:2] == (None, 0) or destinfo[:2] == (None, 0):
            self.assertTrue(srcinfo[2] <= destinfo[2],
                            'Latest revision for %s decreased from %d to %d!'
                            % (branch or 'default', srcinfo[2], destinfo[2]))
            self.assertEqual(srcinfo[0], destinfo[0])
        else:
            pr = sorted(filter(lambda x: x[1] == srcinfo[0] and x[0] <= srcinfo[1],
                        revkeys), reverse=True)[0][0]
            self.assertEqual(pr, destinfo[1])
            self.assertEqual(srcinfo[2], destinfo[2])


def buildmethod(case, name, stupid, single):
    m = lambda self: self._do_case(case, stupid, single)
    m.__name__ = name
    m.__doc__ = ('Test rebuildmeta on %s with %s replay. (%s)' %
                 (case,
                  (stupid and 'stupid') or 'real',
                  (single and 'single') or 'standard',
                  )
                 )
    return m


attrs = {'_do_case': _do_case,
         }
for case in [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]:
    # this fixture results in an empty repository, don't use it
    if case == 'project_root_not_repo_root.svndump':
        continue
    bname = 'test_' + case[:-len('.svndump')]
    attrs[bname] = buildmethod(case, bname, False, False)
    name = bname + '_stupid'
    attrs[name] = buildmethod(case, name, True, False)
    name = bname + '_single'
    attrs[name] = buildmethod(case, name, False, True)

RebuildMetaTests = type('RebuildMetaTests', (test_util.TestBase, ), attrs)


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(RebuildMetaTests),
          ]
    return unittest.TestSuite(all)
