import os
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
from mercurial import localrepo
from mercurial import ui
from mercurial import util as hgutil

from hgsubversion import compathacks
from hgsubversion import svncommands
from hgsubversion import svnmeta
from hgsubversion import util

# These test repositories have harmless skew in rebuildmeta for the
# last-pulled-rev because the last rev in svn causes absolutely no
# changes in hg.
expect_youngest_skew = [('file_mixed_with_branches.svndump', False, False),
                        ('file_mixed_with_branches.svndump', True, False),
                        ('unrelatedbranch.svndump', False, False),
                        ('unrelatedbranch.svndump', True, False),
                        ]



def _do_case(self, name, layout):
    subdir = test_util.subdir.get(name, '')
    single = layout == 'single'
    u = ui.ui()
    config = {}
    if layout == 'custom':
        for branch, path in test_util.custom.get(name, {}).iteritems():
            config['hgsubversionbranch.%s' % branch] = path
            u.setconfig('hgsubversionbranch', branch, path)
    repo, repo_path = self.load_and_fetch(name, subdir=subdir, layout=layout)
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

    try:
        svncommands.rebuildmeta(u, dest,
                                args=[test_util.fileurl(repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    self._run_assertions(name, single, src, dest, u)

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

    if hgutil.safehasattr(localrepo.localrepository, 'pull'):
        dest.pull(src)
    else:
        # Mercurial >= 3.2
        from mercurial import exchange
        exchange.pull(dest, src)

    # insert a wrapper that prevents calling changectx.children()
    extensions.wrapfunction(context.changectx, 'children', failfn)
    try:
        svncommands.updatemeta(u, dest,
                               args=[test_util.fileurl(repo_path +
                                                        subdir), ])
    finally:
        # remove the wrapper
        context.changectx.children = origchildren

    self._run_assertions(name, single, srcrepo, dest, u)


def _run_assertions(self, name, single, src, dest, u):

    self.assertTrue(os.path.isdir(os.path.join(src.path, 'svn')),
                    'no .hg/svn directory in the source!')
    self.assertTrue(os.path.isdir(os.path.join(dest.path, 'svn')),
                    'no .hg/svn directory in the destination!')
    dest = hg.repository(u, os.path.dirname(dest.path))
    for tf in ('lastpulled', 'rev_map', 'uuid', 'tagmap', 'layout', 'subdir',):

        stf = os.path.join(src.path, 'svn', tf)
        # the generation of tagmap is lazy so it doesn't strictly need to exist
        # if it's not being used
        if not stf.endswith('tagmap'):
            self.assertTrue(os.path.isfile(stf), '%r is missing!' % stf)
        dtf = os.path.join(dest.path, 'svn', tf)
        old, new = None, None
        if not dtf.endswith('tagmap'):
            self.assertTrue(os.path.isfile(dtf), '%r is missing!' % tf)
        if os.path.isfile(stf) and os.path.isfile(dtf):
            old, new = util.load(stf, resave=False), util.load(dtf, resave=False)
        if tf == 'lastpulled' and (name,
                                   self.stupid, single) in expect_youngest_skew:
            self.assertNotEqual(old, new,
                                'rebuildmeta unexpected match on youngest rev!')
            continue
        self.assertEqual(old, new, tf + ' differs')
        try:
          self.assertEqual(src.branchmap(), dest.branchmap())
        except AttributeError:
          # hg 2.8 and earlier
          self.assertEqual(src.branchtags(), dest.branchtags())
    srcbi = util.load(os.path.join(src.path, 'svn', 'branch_info'))
    destbi = util.load(os.path.join(dest.path, 'svn', 'branch_info'))
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


def buildmethod(case, name, layout):
    m = lambda self: self._do_case(case, layout)
    m.__name__ = name
    m.__doc__ = ('Test rebuildmeta on %s (%s)' %
                 (case, layout))
    return m


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
    attrs[bname] = buildmethod(case, bname, 'auto')
    attrs[bname + '_single'] = buildmethod(case, bname + '_single', 'single')
    if case in test_util.custom:
            attrs[bname + '_custom'] = buildmethod(case,
                                                   bname + '_custom',
                                                   'single')

RebuildMetaTests = type('RebuildMetaTests', (test_util.TestBase,), attrs)
