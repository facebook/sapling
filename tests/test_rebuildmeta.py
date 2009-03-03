import os
import pickle
import unittest

from mercurial import hg
from mercurial import ui

import test_util
import rebuildmeta
import hg_delta_editor

# List of expected "missing" branches - these are really files that happen
# to be in the branches dir. This will be fixed at a later date.
expected_branch_deltas = {'unrelatedbranch.svndump': [ ],
                          'file_mixed_with_branches.svndump': [ ],
                          }

def _do_case(self, name, stupid):
    subdir = test_util.subdir.get(name, '')
    self._load_fixture_and_fetch(name, subdir=subdir, stupid=stupid)
    assert len(self.repo) > 0
    wc2_path = self.wc_path + '_clone'
    u = ui.ui()
    src, dest = hg.clone(u, self.wc_path, wc2_path, update=False)
    rebuildmeta.rebuildmeta(u,
                            dest,
                            os.path.dirname(dest.path),
                            args=[test_util.fileurl(self.repo_path +
                                                    subdir), ])
    dest = hg.repository(u, os.path.dirname(dest.path))
    self.assert_(open(os.path.join(src.path, 'svn', 'last_rev')).read() >=
                     open(os.path.join(dest.path, 'svn', 'last_rev')).read())
    for tf in ('rev_map', 'uuid', 'url'):
        self.assertEqual(open(os.path.join(src.path, 'svn', tf)).read(),
                         open(os.path.join(dest.path, 'svn', tf)).read())
    self.assertEqual(pickle.load(open(os.path.join(src.path, 'svn',
                                                   'tag_info'))),
                     pickle.load(open(os.path.join(dest.path, 'svn',
                                                   'tag_info'))))
    self.assertEqual(src.branchtags(), dest.branchtags())
    srcbi = pickle.load(open(os.path.join(src.path, 'svn', 'branch_info')))
    for mustpop in expected_branch_deltas.get(name, []):
        del srcbi[mustpop]
    destbi = pickle.load(open(os.path.join(dest.path, 'svn', 'branch_info')))
    self.assertEqual(sorted(srcbi.keys()), sorted(destbi.keys()))
    for branch in destbi:
        srcinfo = srcbi[branch]
        destinfo = destbi[branch]
        hge = hg_delta_editor.HgChangeReceiver(path=os.path.dirname(dest.path),
                                                   repo=dest,
                                                   ui_=u)
        if destinfo[:2] == (None, 0):
            self.assert_(srcinfo[2] <= destinfo[2])
            self.assertEqual(srcinfo[0], destinfo[0])
        else:
            pr = sorted(filter(lambda x: x[1] == srcinfo[0] and x[0] <= srcinfo[1],
                        hge.revmap.keys()), reverse=True)[0][0]
            self.assertEqual(pr, destinfo[1])
            self.assertEqual(srcinfo[2], destinfo[2])


def buildmethod(case, name, stupid):
    m = lambda self: self._do_case(case, stupid)
    m.__name__ = name
    m.__doc__ = ('Test rebuildmeta on %s with %s replay.' %
                 (case, (stupid and 'stupid') or 'real'))
    return m


attrs = {'_do_case': _do_case,
         }
for case in [f for f in os.listdir(test_util.FIXTURES) if f.endswith('.svndump')]:
    name = 'test_' + case[:-len('.svndump')]
    attrs[name] = buildmethod(case, name, False)
    name += '_stupid'
    attrs[name] = buildmethod(case, name, True)
RebuildMetaTests = type('RebuildMetaTests', (test_util.TestBase, ), attrs)


def suite():
    all = [unittest.TestLoader().loadTestsFromTestCase(RebuildMetaTests),
          ]
    return unittest.TestSuite(all)
