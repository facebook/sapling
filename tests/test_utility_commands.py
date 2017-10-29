import test_util

import os
import unittest
import re

from hgext import rebase
from mercurial import hg
from mercurial import revlog
from mercurial import context
from mercurial import node
from mercurial import commands
from mercurial import util as hgutil

from hgsubversion import util
from hgsubversion import svncommands
from hgsubversion import verify
from hgsubversion import wrappers
from hgsubversion import compathacks

expected_info_output = '''URL: %(repourl)s/%(branch)s
Repository Root: %(repourl)s
Repository UUID: df2126f7-00ab-4d49-b42c-7e981dde0bcf
Revision: %(rev)s
Node Kind: directory
Last Changed Author: durin
Last Changed Rev: %(rev)s
Last Changed Date: %(date)s
'''

def repourl(repo_path):
    return util.normalize_url(test_util.fileurl(repo_path))

class UtilityTests(test_util.TestBase):
    stupid_mode_tests = True

    def test_info_output(self, custom=False):
        if custom:
            config = {
                'hgsubversionbranch.default': 'trunk',
                'hgsubversionbranch.the_branch': 'branches/the_branch',
                }
        else:
            config = {}
        repo, repo_path = self.load_and_fetch('two_heads.svndump', config=config)
        hg.update(self.repo, 'the_branch')
        u = self.ui()
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repourl': repourl(repo_path),
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertMultiLineEqual(actual, expected)
        hg.update(self.repo, 'default')
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repourl': repourl(repo_path),
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertMultiLineEqual(actual, expected)
        hg.update(self.repo, 'default')
        u.pushbuffer()
        svncommands.info(u, self.repo, rev=3)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:05 +0000 (Wed, 08 Oct 2008)',
                     'repourl': repourl(repo_path),
                     'branch': 'branches/the_branch',
                     'rev': 5,
                     })
        self.assertMultiLineEqual(actual, expected)
        destpath = self.wc_path + '_clone'
        test_util.hgclone(u, self.repo, destpath)
        repo2 = hg.repository(u, destpath)
        repo2.ui.setconfig('paths', 'default-push',
                           self.repo.ui.config('paths', 'default'))
        hg.update(repo2, 'default')
        svncommands.rebuildmeta(u, repo2, [])
        u.pushbuffer()
        svncommands.info(u, repo2)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repourl': repourl(repo_path),
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertMultiLineEqual(actual, expected)

    def test_info_output_custom(self):
        self.test_info_output(custom=True)

    def test_info_single(self, custom=False):
        if custom:
            subdir=None
            config = {
                'hgsubversionbranch.default': 'trunk/'
                }
        else:
            subdir='trunk'
            config = {}
        repo, repo_path = self.load_and_fetch('two_heads.svndump',
                                              subdir=subdir,
                                              config=config)
        hg.update(self.repo, 'tip')
        u = self.ui()
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = (expected_info_output %
                    {'date': '2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)',
                     'repourl': repourl(repo_path),
                     'branch': 'trunk',
                     'rev': 6,
                     })
        self.assertMultiLineEqual(expected, actual)

    def test_info_custom_single(self):
        self.test_info_single(custom=True)

    def test_missing_metadata(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        os.remove(self.repo.vfs.join('svn/branch_info'))
        svncommands.updatemeta(self.ui(), self.repo, [])

        test_util.rmtree(self.repo.vfs.join('svn'))
        self.assertRaises(hgutil.Abort,
                          self.repo.svnmeta)
        self.assertRaises(hgutil.Abort,
                          svncommands.info,
                          self.ui(), repo=self.repo, args=[])
        self.assertRaises(hgutil.Abort,
                          svncommands.genignore,
                          self.ui(), repo=self.repo, args=[])

        os.remove(self.repo.vfs.join('hgrc'))
        self.assertRaises(hgutil.Abort,
                          self.repo.svnmeta)
        self.assertRaises(hgutil.Abort,
                          svncommands.info,
                          self.ui(), repo=self.repo, args=[])
        self.assertRaises(hgutil.Abort,
                          svncommands.genignore,
                          self.ui(), repo=self.repo, args=[])

        self.assertRaises(hgutil.Abort,
                          svncommands.rebuildmeta,
                          self.ui(), repo=self.repo, args=[])

    def test_parent_output(self):
        self._load_fixture_and_fetch('two_heads.svndump')
        u = self.ui()
        u.pushbuffer()
        parents = (self.repo['the_branch'].node(), revlog.nullid,)
        def filectxfn(repo, memctx, path):
            return compathacks.makememfilectx(repo,
                                              path=path,
                                              data='added',
                                              islink=False,
                                              isexec=False,
                                              copied=False)
        lr = self.repo
        ctx = context.memctx(lr,
                             parents,
                             'automated test',
                             ['added_bogus_file', 'other_added_file', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        new = lr.commitctx(ctx)
        hg.update(self.repo, new)
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True)
        actual = u.popbuffer()
        self.assertEqual(actual, '3:4e256962fc5d\n')

        hg.update(self.repo, 'default')

        # Make sure styles work
        u.pushbuffer()
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True, style='compact')
        actual = u.popbuffer()
        self.assertEqual(actual, '4:1083037b18d8\n')

        # custom templates too
        u.pushbuffer()
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True, template='{node}\n')
        actual = u.popbuffer()
        self.assertEqual(actual, '1083037b18d85cd84fa211c5adbaeff0fea2cd9f\n')

        u.pushbuffer()
        wrappers.parents(lambda x, y: None, u, self.repo, svn=True)
        actual = u.popbuffer()
        self.assertEqual(actual, '4:1083037b18d8\n')

    def test_outgoing_output(self):
        repo, repo_path = self.load_and_fetch('two_heads.svndump')
        u = self.ui()
        parents = (self.repo['the_branch'].node(), revlog.nullid,)
        def filectxfn(repo, memctx, path):
            return compathacks.makememfilectx(repo,
                                              path=path,
                                              data='added',
                                              islink=False,
                                              isexec=False,
                                              copied=False)
        lr = self.repo
        ctx = context.memctx(lr,
                             parents,
                             'automated test',
                             ['added_bogus_file', 'other_added_file', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        new = lr.commitctx(ctx)
        hg.update(self.repo, new)
        u.pushbuffer()
        commands.outgoing(u, self.repo, repourl(repo_path))
        actual = u.popbuffer()
        self.assertTrue(node.hex(self.repo['localbranch'].node())[:8] in actual)
        self.assertEqual(actual.strip(), '5:6de15430fa20')
        hg.update(self.repo, 'default')
        u.pushbuffer()
        commands.outgoing(u, self.repo, repourl(repo_path))
        actual = u.popbuffer()
        self.assertEqual(actual, '')

    def test_rebase(self):
        self._load_fixture_and_fetch('two_revs.svndump')
        parents = (self.repo[0].node(), revlog.nullid,)
        def filectxfn(repo, memctx, path):
            return compathacks.makememfilectx(repo,
                                              path=path,
                                              data='added',
                                              islink=False,
                                              isexec=False,
                                              copied=False)
        lr = self.repo
        ctx = context.memctx(lr,
                             parents,
                             'automated test',
                             ['added_bogus_file', 'other_added_file', ],
                             filectxfn,
                             'testy',
                             '2008-12-21 16:32:00 -0500',
                             {'branch': 'localbranch', })
        lr.commitctx(ctx)
        self.assertEqual(self.repo['tip'].branch(), 'localbranch')
        beforerebasehash = self.repo['tip'].node()
        hg.update(self.repo, 'tip')
        wrappers.rebase(rebase.rebase, self.ui(), self.repo, svn=True)
        self.assertEqual(self.repo['tip'].branch(), 'localbranch')
        self.assertEqual(self.repo['tip'].parents()[0].parents()[0], self.repo[0])
        self.assertNotEqual(beforerebasehash, self.repo['tip'].node())

    def test_genignore(self, layout='auto'):
        """ Test generation of .hgignore file. """
        if layout == 'custom':
            config = {
                'hgsubversionbranch.default': 'trunk',
                }
        else:
            config = {}
        repo = self._load_fixture_and_fetch('ignores.svndump',
                                            layout=layout,
                                            noupdate=False,
                                            config=config)
        u = self.ui()
        u.pushbuffer()
        svncommands.genignore(u, repo, self.wc_path)
        self.assertMultiLineEqual(open(os.path.join(self.wc_path, '.hgignore')).read(),
                         '.hgignore\nsyntax:glob\nblah\notherblah\nbaz/magic\n')

    def test_genignore_single(self):
        self.test_genignore(layout='single')

    def test_genignore_custom(self):
        self.test_genignore(layout='custom')

    def test_list_authors(self):
        repo_path = self.load_svndump('replace_trunk_with_branch.svndump')
        u = self.ui()
        u.pushbuffer()
        svncommands.listauthors(u,
                                     args=[test_util.fileurl(repo_path)],
                                     authors=None)
        actual = u.popbuffer()
        self.assertMultiLineEqual(actual, 'Augie\nevil\n')

    def test_list_authors_map(self):
        repo_path = self.load_svndump('replace_trunk_with_branch.svndump')
        author_path = os.path.join(repo_path, 'authors')
        svncommands.listauthors(self.ui(),
                                args=[test_util.fileurl(repo_path)],
                                authors=author_path)
        self.assertMultiLineEqual(open(author_path).read(), 'Augie=\nevil=\n')

    def test_svnverify(self):
        repo, repo_path = self.load_and_fetch('binaryfiles.svndump',
                                              noupdate=False)
        ret = verify.verify(self.ui(), repo, [], rev=1)
        self.assertEqual(0, ret)
        repo_path = self.load_svndump('binaryfiles-broken.svndump')
        u = self.ui()
        u.pushbuffer()
        ret = verify.verify(u, repo, [test_util.fileurl(repo_path)],
                            rev=1)
        output = u.popbuffer()
        self.assertEqual(1, ret)
        output = re.sub(r'file://\S+', 'file://', output)
        self.assertMultiLineEqual("""\
verifying d51f46a715a1 against file://
difference in: binary2
unexpected file: binary1
missing file: binary3
""", output)

    def test_corruption(self):
        SUCCESS = 0
        FAILURE = 1

        repo, repo_path = self.load_and_fetch('correct.svndump', layout='single',
                                              subdir='')

        ui = self.ui()

        self.assertEqual(SUCCESS, verify.verify(ui, self.repo, rev='tip'))

        corrupt_source = test_util.fileurl(self.load_svndump('corrupt.svndump'))

        repo.ui.setconfig('paths', 'default', corrupt_source)

        ui.pushbuffer()
        code = verify.verify(ui, repo, rev='tip')
        actual = ui.popbuffer()

        actual = actual.replace(corrupt_source, '$REPO')
        actual = set(actual.splitlines())

        expected = set([
            'verifying 78e965230a13 against $REPO@1',
            'missing file: missing-file',
            'wrong flags for: executable-file',
            'wrong flags for: symlink',
            'wrong flags for: regular-file',
            'difference in: another-regular-file',
            'difference in: regular-file',
            'unexpected file: empty-file',
        ])

        self.assertEqual((FAILURE, expected), (code, actual))

    def test_svnrebuildmeta(self):
        otherpath = self.load_svndump('binaryfiles-broken.svndump')
        otherurl = test_util.fileurl(otherpath)
        self.load_and_fetch('replace_trunk_with_branch.svndump')
        # rebuildmeta with original repo
        svncommands.rebuildmeta(self.ui(), repo=self.repo, args=[])
        # rebuildmeta with unrelated repo
        self.assertRaises(hgutil.Abort,
                          svncommands.rebuildmeta,
                          self.ui(), repo=self.repo, args=[otherurl])
        # rebuildmeta --unsafe-skip-uuid-check with unrelated repo
        svncommands.rebuildmeta(self.ui(), repo=self.repo, args=[otherurl],
                                unsafe_skip_uuid_check=True)
