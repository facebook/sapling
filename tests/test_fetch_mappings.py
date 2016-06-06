"""Tests for author maps and file maps.
"""
import test_util

import os
import unittest

from mercurial import commands
from mercurial import hg
from mercurial import node
from mercurial import util as hgutil

from hgsubversion import maps
from hgsubversion import svncommands
from hgsubversion import util
from hgsubversion import verify

class MapTests(test_util.TestBase):
    stupid_mode_tests = True

    @property
    def authors(self):
        return os.path.join(self.tmpdir, 'authormap')

    @property
    def filemap(self):
        return os.path.join(self.tmpdir, 'filemap')

    @property
    def branchmap(self):
        return os.path.join(self.tmpdir, 'branchmap')

    @property
    def tagmap(self):
        return os.path.join(self.tmpdir, 'tagmap')

    def test_author_map(self):
        repo_path = self.load_svndump('replace_trunk_with_branch.svndump')
        authormap = open(self.authors, 'w')
        authormap.write('Augie=Augie Fackler <durin42@gmail.com> # stuffy\n')
        authormap.write("Augie Fackler <durin42@gmail.com>\n")
        authormap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, authors=self.authors)
        self.assertEqual(self.repo[0].user(),
                         'Augie Fackler <durin42@gmail.com>')
        self.assertEqual(self.repo['tip'].user(),
                        'evil@5b65bade-98f3-4993-a01f-b7a6710da339')

    def test_author_map_closing_author(self):
        repo_path = self.load_svndump('replace_trunk_with_branch.svndump')
        authormap = open(self.authors, 'w')
        authormap.write("evil=Testy <test@test>")
        authormap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, authors=self.authors)
        self.assertEqual(self.repo[0].user(),
                         'Augie@5b65bade-98f3-4993-a01f-b7a6710da339')
        self.assertEqual(self.repo['tip'].user(),
                        'Testy <test@test>')

    def test_author_map_no_author(self):
        repo, repo_path = self.load_and_fetch('no-author.svndump')
        users = set(self.repo[r].user() for r in self.repo)
        expected_users = ['(no author)@%s' % self.repo.svnmeta().uuid]
        self.assertEqual(sorted(users), expected_users)
        test_util.rmtree(self.wc_path)

        authormap = open(self.authors, 'w')
        authormap.write("(no author)=Testy <test@example.com>")
        authormap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, authors=self.authors)
        users = set(self.repo[r].user() for r in self.repo)
        expected_users = ['Testy <test@example.com>']
        self.assertEqual(sorted(users), expected_users)

    def test_author_map_no_overwrite(self):
        cwd = os.path.dirname(__file__)
        orig = os.path.join(cwd, 'fixtures', 'author-map-test.txt')
        # create a fake hgsubversion repo
        repopath = os.path.join(self.wc_path, '.hg')
        repopath = os.path.join(repopath, 'svn')
        if not os.path.isdir(repopath):
            os.makedirs(repopath)
        new = open(os.path.join(repopath, 'authors'), 'w')
        new.write(open(orig).read())
        new.close()
        meta = self.repo.svnmeta(skiperrorcheck=True)
        test = maps.AuthorMap(
            meta.ui, meta.authormap_file, meta.defaulthost,
            meta.caseignoreauthors, meta.mapauthorscmd, meta.defaultauthors)
        fromself = set(test)
        test.load(orig)
        all_tests = set(test)
        self.assertEqual(fromself.symmetric_difference(all_tests), set())

    def test_author_map_caseignore(self):
        repo_path = self.load_svndump('replace_trunk_with_branch.svndump')
        authormap = open(self.authors, 'w')
        authormap.write('augie=Augie Fackler <durin42@gmail.com> # stuffy\n')
        authormap.write("Augie Fackler <durin42@gmail.com>\n")
        authormap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'authormap', self.authors)
        ui.setconfig('hgsubversion', 'caseignoreauthors', True)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, authors=self.authors)
        self.assertEqual(self.repo[0].user(),
                         'Augie Fackler <durin42@gmail.com>')
        self.assertEqual(self.repo['tip'].user(),
                        'evil@5b65bade-98f3-4993-a01f-b7a6710da339')

    def test_author_map_mapauthorscmd(self):
        repo_path = self.load_svndump('replace_trunk_with_branch.svndump')
        ui = self.ui()
        ui.setconfig('hgsubversion', 'mapauthorscmd', 'echo "svn: %s"')
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path)
        self.assertEqual(self.repo[0].user(), 'svn: Augie')
        self.assertEqual(self.repo['tip'].user(), 'svn: evil')

    def _loadwithfilemap(self, svndump, filemapcontent,
            failonmissing=True):
        repo_path = self.load_svndump(svndump)
        filemap = open(self.filemap, 'w')
        filemap.write(filemapcontent)
        filemap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'filemap', self.filemap)
        ui.setconfig('hgsubversion', 'failoninvalidreplayfile', 'true')
        ui.setconfig('hgsubversion', 'failonmissing', failonmissing)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, filemap=self.filemap)
        return self.repo

    @test_util.requiresreplay
    def test_file_map(self):
        repo = self._loadwithfilemap('replace_trunk_with_branch.svndump',
            "include alpha\n")
        self.assertEqual(node.hex(repo[0].node()), '88e2c7492d83e4bf30fbb2dcbf6aa24d60ac688d')
        self.assertEqual(node.hex(repo['default'].node()), 'e524296152246b3837fe9503c83b727075835155')

    @test_util.requiresreplay
    def test_file_map_exclude(self):
        repo = self._loadwithfilemap('replace_trunk_with_branch.svndump',
            "exclude alpha\n")
        self.assertEqual(node.hex(repo[0].node()), '2c48f3525926ab6c8b8424bcf5eb34b149b61841')
        self.assertEqual(node.hex(repo['default'].node()), 'b37a3c0297b71f989064d9b545b5a478bbed7cc1')

    @test_util.requiresreplay
    def test_file_map_rule_order(self):
        repo = self._loadwithfilemap('replace_trunk_with_branch.svndump',
            "exclude alpha\ninclude .\nexclude gamma\n")
        # The exclusion of alpha is overridden by the later rule to
        # include all of '.', whereas gamma should remain excluded
        # because it's excluded after the root directory.
        self.assertEqual(self.repo[0].manifest().keys(),
                         ['alpha', 'beta'])
        self.assertEqual(self.repo['default'].manifest().keys(),
                         ['alpha', 'beta'])

    @test_util.requiresreplay
    def test_file_map_copy(self):
        # Exercise excluding files copied from a non-excluded directory.
        # There will be missing files as we are copying from an excluded
        # directory.
        repo = self._loadwithfilemap('copies.svndump', "exclude dir2\n",
                failonmissing=False)
        self.assertEqual(['dir/a', 'dir3/a'], list(repo[2]))

    @test_util.requiresreplay
    def test_file_map_exclude_copy_source_and_dest(self):
        # dir3 is excluded and copied from dir2 which is also excluded.
        # dir3 files should not be marked as missing and fetched.
        repo = self._loadwithfilemap('copies.svndump',
                "exclude dir2\nexclude dir3\n")
        self.assertEqual(['dir/a'], list(repo[2]))

    @test_util.requiresreplay
    def test_file_map_include_file_exclude_dir(self):
        # dir3 is excluded but we want dir3/a, which is also copied from
        # an exluded dir2. dir3/a should be fetched.
        repo = self._loadwithfilemap('copies.svndump',
                "include .\nexclude dir2\nexclude dir3\ninclude dir3/a\n",
                failonmissing=False)
        self.assertEqual(['dir/a', 'dir3/a'], list(repo[2]))

    @test_util.requiresreplay
    def test_file_map_delete_dest(self):
        repo = self._loadwithfilemap('copies.svndump', 'exclude dir3\n')
        self.assertEqual(['dir/a', 'dir2/a'], list(repo[3]))

    def test_branchmap(self):
        repo_path = self.load_svndump('branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = good-name # stuffy\n")
        branchmap.write("feature = default\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assert_('badname' not in branches)
        self.assert_('good-name' in branches)
        self.assertEquals(self.repo[2].branch(), 'default')

    def test_branchmap_regex_and_glob(self):
        repo_path = self.load_svndump('branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("syntax:re\n")
        branchmap.write("bad(.*) = good-\\1 # stuffy\n")
        branchmap.write("glob:feat* = default\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assert_('badname' not in branches)
        self.assert_('good-name' in branches)
        self.assertEquals(self.repo[2].branch(), 'default')

    def test_branchmap_tagging(self):
        '''test tagging a renamed branch, which used to raise an exception'''
        repo_path = self.load_svndump('commit-to-tag.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("magic = art\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['art', 'closeme'])

    def test_branchmap_empty_commit(self):
        '''test mapping an empty commit on a renamed branch'''
        repo_path = self.load_svndump('propset-branch.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("the-branch = bob\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['bob', 'default'])

    def test_branchmap_combine(self):
        '''test combining two branches, but retaining heads'''
        repo_path = self.load_svndump('branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = default\n")
        branchmap.write("feature = default\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['default'])
        self.assertEquals(len(self.repo.heads()), 2)
        self.assertEquals(len(self.repo.branchheads('default')), 2)

        # test that the mapping does not affect branch info
        branches = self.repo.svnmeta().branches
        self.assertEquals(sorted(branches.keys()),
                          [None, 'badname', 'feature'])

    def test_branchmap_rebuildmeta(self):
        '''test rebuildmeta on a branchmapped clone'''
        repo_path = self.load_svndump('branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = dit\n")
        branchmap.write("feature = dah\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        originfo = self.repo.svnmeta().branches

        # clone & rebuild
        ui = self.ui()
        src, dest = test_util.hgclone(ui, self.wc_path, self.wc_path + '_clone',
                                      update=False)
        src = test_util.getlocalpeer(src)
        dest = test_util.getlocalpeer(dest)
        svncommands.rebuildmeta(ui, dest,
                                args=[test_util.fileurl(repo_path)])

        # just check the keys; assume the contents are unaffected by the branch
        # map and thus properly tested by other tests
        self.assertEquals(sorted(src.svnmeta().branches),
                          sorted(dest.svnmeta().branches))

    def test_branchmap_verify(self):
        '''test verify on a branchmapped clone'''
        repo_path = self.load_svndump('branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname = dit\n")
        branchmap.write("feature = dah\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        repo = self.repo

        for r in repo:
            self.assertEquals(verify.verify(ui, repo, rev=r), 0)

    def test_branchmap_no_replacement(self):
        '''test that empty mappings are accepted

        Empty mappings are lines like 'this ='. We check that such branches are
        not converted.
        '''
        repo_path = self.load_svndump('branchmap.svndump')
        branchmap = open(self.branchmap, 'w')
        branchmap.write("badname =\n")
        branchmap.close()
        ui = self.ui()
        ui.setconfig('hgsubversion', 'branchmap', self.branchmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, branchmap=self.branchmap)
        branches = set(self.repo[i].branch() for i in self.repo)
        self.assertEquals(sorted(branches), ['default', 'feature'])

    def test_tagmap(self):
        repo_path = self.load_svndump('basic_tag_tests.svndump')
        tagmap = open(self.tagmap, 'w')
        tagmap.write("tag_r3 = 3.x # stuffy\n")
        tagmap.write("copied_tag = \n")
        tagmap.close()

        ui = self.ui()
        ui.setconfig('hgsubversion', 'tagmap', self.tagmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, tagmap=self.tagmap)
        tags = self.repo.tags()
        assert 'tag_r3' not in tags
        assert '3.x' in tags
        assert 'copied_tag' not in tags

    def test_tagren_changed(self):
        repo_path = self.load_svndump('commit-to-tag.svndump')
        tagmap = open(self.tagmap, 'w')
        tagmap.write("edit-at-create = edit-past\n")
        tagmap.write("also-edit = \n")
        tagmap.write("will-edit = edit-future\n")
        tagmap.close()

        ui = self.ui()
        ui.setconfig('hgsubversion', 'tagmap', self.tagmap)
        commands.clone(ui, test_util.fileurl(repo_path),
                       self.wc_path, tagmap=self.tagmap)
        tags = self.repo.tags()

    def test_empty_log_message(self):
        repo, repo_path = self.load_and_fetch('empty-log-message.svndump')

        self.assertEqual(repo['tip'].description(), '')

        test_util.rmtree(self.wc_path)

        ui = self.ui()
        ui.setconfig('hgsubversion', 'defaultmessage', 'blyf')
        commands.clone(ui, test_util.fileurl(repo_path), self.wc_path)

        self.assertEqual(self.repo['tip'].description(), 'blyf')
