import test_util

import os
import unittest
import urllib

from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import ui
from mercurial import encoding

class TestBasicRepoLayout(test_util.TestBase):
    stupid_mode_tests = True

    def test_no_dates(self):
        repo = self._load_fixture_and_fetch('test_no_dates.svndump')
        local_epoch = repo[0].date()
        self.assertEqual(local_epoch[0], local_epoch[1])
        self.assertEqual(repo[1].date(), repo[2].date())

    def test_fresh_fetch_single_rev(self):
        repo = self._load_fixture_and_fetch('single_rev.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(repo['tip'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/trunk@2')
        self.assertEqual(repo['tip'], repo[0])

    def test_fresh_fetch_two_revs(self):
        repo = self._load_fixture_and_fetch('two_revs.svndump')
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         'c95251e0dd04697deee99b79cc407d7db76e6a5f')
        self.assertEqual(repo['tip'], repo[1])

    def test_branches(self):
        repo = self._load_fixture_and_fetch('simple_branch.svndump')
        self.assertEqual(node.hex(repo[0].node()),
                         'a1ff9f5d90852ce7f8e607fa144066b0a06bdc57')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '545e36ed13615e39c5c8fb0c325109d8cb8e00c3')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'].parents()[0], repo['default'])
        self.assertEqual(repo['tip'].extra()['convert_revision'],
                         'svn:3cd547df-371e-4add-bccf-aba732a2baf5/branches/the_branch@4')
        self.assertEqual(repo['default'].extra()['convert_revision'],
                         'svn:3cd547df-371e-4add-bccf-aba732a2baf5/trunk@3')
        self.assertEqual(len(repo.heads()), 1)

    def test_two_branches_with_heads(self):
        repo = self._load_fixture_and_fetch('two_heads.svndump')
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1083037b18d85cd84fa211c5adbaeff0fea2cd9f')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '4e256962fc5df545e2e0a51d0d1dc61c469127e6')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_many_special_cases(self):
        repo = self._load_fixture_and_fetch('many_special_cases.svndump')

        self.assertEquals(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        # two possible hashes for bw compat to hg < 1.5, since hg 1.5
        # sorts entries in extra()
        self.assertTrue(node.hex(repo['tip'].node()) in
                         ('e92012d8c170a0236c84166167f149c2e28548c6',
                         'b7bdc73041b1852563deb1ef3f4153c2fe4484f2'))
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '4e256962fc5df545e2e0a51d0d1dc61c469127e6')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_file_mixed_with_branches(self):
        repo = self._load_fixture_and_fetch('file_mixed_with_branches.svndump')
        self.assertEqual(node.hex(repo['default'].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        assert 'README' not in repo
        assert '../branches' not in repo

    def test_files_copied_from_outside_btt(self):
        repo = self._load_fixture_and_fetch(
            'test_files_copied_from_outside_btt.svndump')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '3c78170e30ddd35f2c32faa0d8646ab75bba4f73')
        self.assertEqual(test_util.repolen(repo), 2)

    def test_file_renamed_in_from_outside_btt(self):
        repo = self._load_fixture_and_fetch(
                    'file_renamed_in_from_outside_btt.svndump')
        self.assert_('LICENSE.file' in repo['default'])

    def test_renamed_dir_in_from_outside_btt_not_repo_root(self):
        repo = self._load_fixture_and_fetch(
                    'fetch_missing_files_subdir.svndump', subdir='foo')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '269dcdd4361b2847e9f4288d4500e55d35df1f52')
        self.assert_('bar/alpha' in repo['tip'])
        self.assert_('foo' in repo['tip'])
        self.assert_('bar/alpha' not in repo['tip'].parents()[0])
        self.assert_('foo' in repo['tip'].parents()[0])

    def test_oldest_not_trunk_and_tag_vendor_branch(self):
        repo = self._load_fixture_and_fetch(
            'tagged_vendor_and_oldest_not_trunk.svndump')
        self.assertEqual(node.hex(repo['oldest'].node()),
                         '926671740dec045077ab20f110c1595f935334fa')
        self.assertEqual(repo['tip'].parents()[0].parents()[0],
                         repo['oldest'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1a6c3f30911d57abb67c257ec0df3e7bc44786f7')

    def test_propedit_with_nothing_else(self):
        repo = self._load_fixture_and_fetch('branch_prop_edit.svndump')
        self.assertEqual(repo['tip'].description(), 'Commit bogus propchange.')
        self.assertEqual(repo['tip'].branch(), 'dev_branch')

    def test_entry_deletion(self):
        repo = self._load_fixture_and_fetch('delentries.svndump')
        files = list(sorted(repo['tip'].manifest()))
        self.assertEqual(['aa', 'd1/c', 'd1/d2prefix'], files)

    def test_fetch_when_trunk_has_no_files(self):
        repo = self._load_fixture_and_fetch('file_not_in_trunk_root.svndump')
        self.assertEqual(repo['tip'].branch(), 'default')

    def test_path_quoting(self):
        repo_path = self.load_svndump('non_ascii_path_1.svndump')
        subdir = '/b\xC3\xB8b'
        quoted_subdir = urllib.quote(subdir)

        repo_url = test_util.fileurl(repo_path)
        wc_path = self.wc_path
        wc2_path = wc_path + '-2'

        ui = self.ui()

        commands.clone(ui, repo_url + subdir, wc_path)
        commands.clone(ui, repo_url + quoted_subdir, wc2_path)
        repo = hg.repository(ui, wc_path)
        repo2 = hg.repository(ui, wc2_path)

        self.assertEqual(repo['tip'].extra()['convert_revision'],
                         repo2['tip'].extra()['convert_revision'])
        self.assertEqual(test_util.repolen(repo), test_util.repolen(repo2))

        for r in repo:
            self.assertEqual(repo[r].hex(), repo2[r].hex())

    def test_identical_fixtures(self):
        '''ensure that the non_ascii_path_N fixtures are identical'''
        fixturepaths = [
            os.path.join(test_util.FIXTURES, 'non_ascii_path_1.svndump'),
            os.path.join(test_util.FIXTURES, 'non_ascii_path_2.svndump'),
        ]
        self.assertMultiLineEqual(open(fixturepaths[0]).read(),
                                  open(fixturepaths[1]).read())

    def test_invalid_message(self):
        repo = self._load_fixture_and_fetch('invalid_utf8.tar.gz')
        # changelog returns descriptions in local encoding
        desc = encoding.fromlocal(repo[0].description())
        self.assertEqual(desc.decode('utf8'),
                         u'bl\xe5b\xe6rgr\xf8d')


class TestStupidPull(test_util.TestBase):
    stupid_mode_tests = True

    def test_stupid(self):
        repo = self._load_fixture_and_fetch('two_heads.svndump')
        self.assertEqual(node.hex(repo[0].node()),
                         '434ed487136c1b47c1e8f952edb4dc5a8e6328df')
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1083037b18d85cd84fa211c5adbaeff0fea2cd9f')
        self.assertEqual(node.hex(repo['the_branch'].node()),
                         '4e256962fc5df545e2e0a51d0d1dc61c469127e6')
        self.assertEqual(repo['the_branch'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/branches/the_branch@5')
        self.assertEqual(node.hex(repo['the_branch'].parents()[0].node()),
                         'f1ff5b860f5dbb9a59ad0921a79da77f10f25109')
        self.assertEqual(len(repo['tip'].parents()), 1)
        self.assertEqual(repo['default'].extra()['convert_revision'],
                         'svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/trunk@6')
        self.assertEqual(repo['tip'], repo['default'])
        self.assertEqual(len(repo.heads()), 2)

    def test_oldest_not_trunk_and_tag_vendor_branch(self):
        repo = self._load_fixture_and_fetch(
            'tagged_vendor_and_oldest_not_trunk.svndump')
        self.assertEqual(node.hex(repo['oldest'].node()),
                         '926671740dec045077ab20f110c1595f935334fa')
        self.assertEqual(repo['tip'].parents()[0].parents()[0],
                         repo['oldest'])
        self.assertEqual(node.hex(repo['tip'].node()),
                         '1a6c3f30911d57abb67c257ec0df3e7bc44786f7')

    def test_empty_repo(self):
        # This used to crash HgEditor because it could be closed without
        # having been initialized again.
        self._load_fixture_and_fetch('emptyrepo2.svndump')

    def test_fetch_revert(self):
        repo = self._load_fixture_and_fetch('revert.svndump')
        graph = self.getgraph(repo)
        refgraph = """\
o  changeset: 3:937dcd1206d4 (r4)
|  branch:
|  tags:      tip
|  summary:   revert2
|  files:     a dir/b
|
o  changeset: 2:9317a748b7c3 (r3)
|  branch:
|  tags:
|  summary:   revert
|  files:     a dir/b
|
o  changeset: 1:243259a4138a (r2)
|  branch:
|  tags:
|  summary:   changefiles
|  files:     a dir/b
|
o  changeset: 0:ab86791fc857 (r1)
   branch:
   tags:
   summary:   init
   files:     a dir/b

"""
        self.assertMultiLineEqual(refgraph, graph)

    def test_fetch_movetotrunk(self):
        repo = self._load_fixture_and_fetch('movetotrunk.svndump',
                subdir='sub1/sub2')
        graph = self.getgraph(repo)
        refgraph = """\
o  changeset: 0:02996a5980ba (r3)
   branch:
   tags:      tip
   summary:   move to trunk
   files:     a dir/b

"""
        self.assertMultiLineEqual(refgraph, graph)

