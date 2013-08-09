import os
import pickle
import unittest
import sys

# wrapped in a try/except because of weirdness in how
# run.py works as compared to nose.
try:
    import test_util
except ImportError:
    sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))
    import test_util

from mercurial import context
from mercurial import extensions
from mercurial import hg
from mercurial import ui

from hgsubversion import svncommands
from hgsubversion import svnmeta

# These test repositories have harmless skew in rebuildmeta for the
# last-pulled-rev because the last rev in svn causes absolutely no
# changes in hg.
expect_youngest_skew = [('file_mixed_with_branches.svndump', False, False),
                        ('file_mixed_with_branches.svndump', True, False),
                        ('unrelatedbranch.svndump', False, False),
                        ('unrelatedbranch.svndump', True, False),
                        ]



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

    try:
        svncommands.rebuildmeta(u, dest,
                                args=[test_util.fileurl(repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    self._run_assertions(name, stupid, single, src, dest, u)

    wc3_path = self.wc_path + '_partial'
    src, dest = test_util.hgclone(u,
                                  self.wc_path,
                                  wc3_path,
                                  update=False,
                                  rev=[0])
    srcrepo = test_util.getlocalpeer(src)
    dest = test_util.getlocalpeer(dest)

    # insert a wrapper that prevents calling changectx.children()
    extensions.wrapfunction(context.changectx, 'children', failfn)

    try:
        svncommands.rebuildmeta(u, dest,
                                args=[test_util.fileurl(repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    dest.pull(src)

    # insert a wrapper that prevents calling changectx.children()
    extensions.wrapfunction(context.changectx, 'children', failfn)
    try:
        svncommands.updatemeta(u, dest,
                               args=[test_util.fileurl(repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    self._run_assertions(name, stupid, single, srcrepo, dest, u)


def _run_assertions(self, name, stupid, single, src, dest, u):

    self.assertTrue(os.path.isdir(os.path.join(src.path, 'svn')),
                    'no .hg/svn directory in the source!')
    self.assertTrue(os.path.isdir(os.path.join(dest.path, 'svn')),
                    'no .hg/svn directory in the destination!')
    dest = hg.repository(u, os.path.dirname(dest.path))
    for tf in ('lastpulled', 'rev_map', 'uuid', 'tagmap', 'layout', 'subdir',):

        stf = os.path.join(src.path, 'svn', tf)
        self.assertTrue(os.path.isfile(stf), '%r is missing!' % stf)
        dtf = os.path.join(dest.path, 'svn', tf)
        self.assertTrue(os.path.isfile(dtf), '%r is missing!' % tf)
        old, new = open(stf).read(), open(dtf).read()
        if tf == 'lastpulled' and (name,
                                   stupid, single) in expect_youngest_skew:
            self.assertNotEqual(old, new,
                                'rebuildmeta unexpected match on youngest rev!')
            continue
        self.assertMultiLineEqual(old, new, tf + ' differs')
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
    attrs[bname] = buildmethod(case, bname, False, False)
    name = bname + '_stupid'
    attrs[name] = buildmethod(case, name, True, False)
    name = bname + '_single'
    attrs[name] = buildmethod(case, name, False, True)

RebuildMetaTests = type('RebuildMetaTests', (test_util.TestBase,), attrs)
