# no-check-code -- see T24862348

from __future__ import absolute_import

import os
import re

import test_hgsubversion_util
from edenscm.hgext import rebase
from edenscm.hgext.hgsubversion import (
    compathacks,
    svncommands,
    svnmeta,
    util,
    verify,
    wrappers,
)
from edenscm.mercurial import commands, context, hg, node, revlog, util as hgutil


expected_info_output = """URL: %(repourl)s/%(branch)s
Repository Root: %(repourl)s
Repository UUID: df2126f7-00ab-4d49-b42c-7e981dde0bcf
Revision: %(rev)s
Node Kind: directory
Last Changed Author: %(author)s
Last Changed Rev: %(rev)s
Last Changed Date: %(date)s
"""


def repourl(repo_path):
    return util.normalize_url(test_hgsubversion_util.fileurl(repo_path))


class UtilityTests(test_hgsubversion_util.TestBase):
    stupid_mode_tests = True

    def test_info_single(self, custom=False):
        if custom:
            subdir = None
            config = {"hgsubversionbranch.default": "trunk/"}
        else:
            subdir = "trunk"
            config = {}
        repo, repo_path = self.load_and_fetch(
            "two_heads.svndump", subdir=subdir, config=config
        )
        hg.update(self.repo, "tip")
        u = self.ui()
        u.pushbuffer()
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = expected_info_output % {
            "date": "2008-10-08 01:39:29 +0000 (Wed, 08 Oct 2008)",
            "repourl": repourl(repo_path),
            "branch": "trunk",
            "rev": 6,
            "author": "durin",
        }
        self.assertMultiLineEqual(expected, actual)
        # let's set the config and test again
        self.commitchanges(
            changes=[("a", "a", "random string")],
            extras={
                "global_rev": 123456789,
                "convert_revision": "svn:df2126f7-00ab-4d49-b42c-7e981dde0bcf/trunk@7",
            },
        )
        hg.update(self.repo, "tip")
        svncommands.updatemeta(u, self.repo, [])
        u.pushbuffer()
        u.setconfig("hgsubversion", "useglobalrevinsvninfo", True)
        svncommands.info(u, self.repo)
        actual = u.popbuffer()
        expected = expected_info_output % {
            "date": "2008-10-07 20:59:48 -0500 (Tue, 07 Oct 2008)",
            "repourl": repourl(repo_path),
            "branch": "trunk",
            "rev": 123456789,
            "author": "an_author",
        }
        self.assertMultiLineEqual(expected, actual)

    def test_info_custom_single(self):
        self.test_info_single(custom=True)

    def test_missing_metadata(self):
        self._load_fixture_and_fetch("two_heads.svndump")
        os.remove(self.repo.sharedvfs.join("svn/branch_info"))
        svncommands.updatemeta(self.ui(), self.repo, [])

        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        self.assertRaises(hgutil.Abort, self.repo.svnmeta)
        self.assertRaises(
            hgutil.Abort, svncommands.info, self.ui(), repo=self.repo, args=[]
        )
        self.assertRaises(
            hgutil.Abort, svncommands.genignore, self.ui(), repo=self.repo, args=[]
        )

        os.remove(self.repo.localvfs.join("hgrc"))
        self.assertRaises(hgutil.Abort, self.repo.svnmeta)
        self.assertRaises(
            hgutil.Abort, svncommands.info, self.ui(), repo=self.repo, args=[]
        )
        self.assertRaises(
            hgutil.Abort, svncommands.genignore, self.ui(), repo=self.repo, args=[]
        )

        self.assertRaises(
            hgutil.Abort, svncommands.rebuildmeta, self.ui(), repo=self.repo, args=[]
        )

    def test_rebase(self):
        self._load_fixture_and_fetch("two_revs.svndump")
        parents = (self.repo[0].node(), revlog.nullid)

        def filectxfn(repo, memctx, path):
            return compathacks.makememfilectx(
                repo,
                memctx=memctx,
                path=path,
                data="added",
                islink=False,
                isexec=False,
                copied=False,
            )

        lr = self.repo
        ctx = context.memctx(
            lr,
            parents,
            "automated test",
            ["added_bogus_file", "other_added_file"],
            filectxfn,
            "testy",
            "2008-12-21 16:32:00 -0500",
            {"branch": "localbranch"},
        )
        lr.commitctx(ctx)
        self.assertEqual(self.repo["tip"].branch(), "localbranch")
        beforerebasehash = self.repo["tip"].node()
        hg.update(self.repo, "tip")
        wrappers.rebase(rebase.rebase, self.ui(), self.repo, svn=True)
        self.assertEqual(self.repo["tip"].branch(), "localbranch")
        self.assertEqual(self.repo["tip"].parents()[0].parents()[0], self.repo[0])
        self.assertNotEqual(beforerebasehash, self.repo["tip"].node())

    def test_genignore(self, layout="auto"):
        """ Test generation of .hgignore file. """
        if layout == "custom":
            config = {"hgsubversionbranch.default": "trunk"}
        else:
            config = {}
        repo = self._load_fixture_and_fetch(
            "ignores.svndump", layout=layout, noupdate=False, config=config
        )
        u = self.ui()
        u.pushbuffer()
        svncommands.genignore(u, repo, self.wc_path)
        self.assertMultiLineEqual(
            open(os.path.join(self.wc_path, ".hgignore")).read(),
            ".hgignore\nsyntax:glob\nblah\notherblah\nbaz/magic\n",
        )

    def test_genignore_single(self):
        self.test_genignore(layout="single")

    def test_genignore_custom(self):
        self.test_genignore(layout="custom")

    def test_list_authors(self):
        repo_path = self.load_svndump("replace_trunk_with_branch.svndump")
        u = self.ui()
        u.pushbuffer()
        svncommands.listauthors(
            u, args=[test_hgsubversion_util.fileurl(repo_path)], authors=None
        )
        actual = u.popbuffer()
        self.assertMultiLineEqual(actual, "Augie\nevil\n")

    def test_list_authors_map(self):
        repo_path = self.load_svndump("replace_trunk_with_branch.svndump")
        author_path = os.path.join(repo_path, "authors")
        svncommands.listauthors(
            self.ui(),
            args=[test_hgsubversion_util.fileurl(repo_path)],
            authors=author_path,
        )
        self.assertMultiLineEqual(open(author_path).read(), "Augie=\nevil=\n")

    def test_svnverify(self):
        repo, repo_path = self.load_and_fetch("binaryfiles.svndump", noupdate=False)
        ret = verify.verify(self.ui(), repo, [], rev=1)
        self.assertEqual(0, ret)
        repo_path = self.load_svndump("binaryfiles-broken.svndump")
        u = self.ui()
        u.pushbuffer()
        ret = verify.verify(u, repo, [test_hgsubversion_util.fileurl(repo_path)], rev=1)
        output = u.popbuffer()
        self.assertEqual(1, ret)
        output = re.sub(r"file://\S+", "file://", output)
        self.assertMultiLineEqual(
            """\
verifying d51f46a715a1 against file://
difference in: binary2
unexpected file: binary1
missing file: binary3
""",
            output,
        )

    def test_corruption(self):
        SUCCESS = 0
        FAILURE = 1

        repo, repo_path = self.load_and_fetch(
            "correct.svndump", layout="single", subdir=""
        )

        ui = self.ui()

        self.assertEqual(SUCCESS, verify.verify(ui, self.repo, rev="tip"))

        corrupt_source = test_hgsubversion_util.fileurl(
            self.load_svndump("corrupt.svndump")
        )

        repo.ui.setconfig("paths", "default", corrupt_source)

        ui.pushbuffer()
        code = verify.verify(ui, repo, rev="tip")
        actual = ui.popbuffer()

        actual = actual.replace(corrupt_source, "$REPO")
        actual = set(actual.splitlines())

        expected = set(
            [
                "verifying 78e965230a13 against $REPO@1",
                "missing file: missing-file",
                "wrong flags for: executable-file",
                "wrong flags for: symlink",
                "wrong flags for: regular-file",
                "difference in: another-regular-file",
                "difference in: regular-file",
                "unexpected file: empty-file",
            ]
        )

        self.assertEqual((FAILURE, expected), (code, actual))

    def test_svnrebuildmeta(self):
        otherpath = self.load_svndump("binaryfiles-broken.svndump")
        otherurl = test_hgsubversion_util.fileurl(otherpath)
        _, repopath = self.load_and_fetch("replace_trunk_with_branch.svndump")
        repourl = test_hgsubversion_util.fileurl(repopath)
        # rebuildmeta with original repo
        svncommands.rebuildmeta(self.ui(), repo=self.repo, args=[])
        # rebuildmeta with original repo explicitly
        svncommands.rebuildmeta(self.ui(), repo=self.repo, args=[repourl])
        # rebuildmeta with unrelated repo
        self.assertRaises(
            hgutil.Abort,
            svncommands.rebuildmeta,
            self.ui(),
            repo=self.repo,
            args=[otherurl],
        )
        # rebuildmeta --unsafe-skip-uuid-check with unrelated repo
        svncommands.rebuildmeta(
            self.ui(), repo=self.repo, args=[otherurl], unsafe_skip_uuid_check=True
        )

    def test_svntruncatemeta(self):
        repo, repo_path = self.load_and_fetch("two_heads.svndump", config={})
        orig_tip_rev = len(repo) - 1
        orig_tip_svn_rev = util.getsvnrevnum(repo["tip"])

        orig_tip = repo["tip"]

        # Create a new commit that has the same svnrev as the old one
        new_ctx = context.metadataonlyctx(
            repo, orig_tip, text="foo", extra=orig_tip.extra()
        )
        new_tip = repo[repo.commitctx(new_ctx)]

        # Before the truncate, the svnrev points at the old commit
        self.assertEqual(
            list(repo.revs("svnrev(%s)" % orig_tip_svn_rev)), [orig_tip_rev]
        )

        svncommands.truncatemeta(self.ui(), repo=self.repo, args=[])
        repo.invalidate()

        # After the truncate, the svnrev points at the new commit
        new_tip_rev = new_tip.rev()
        self.assertEqual(
            list(repo.revs("svnrev(%s)" % orig_tip_svn_rev)), [new_tip_rev]
        )

    def test_svn_reposubdir_config(self):
        otherpath = self.load_svndump("binaryfiles-broken.svndump")
        otherurl = test_hgsubversion_util.fileurl(otherpath)
        _, repopath = self.load_and_fetch(
            "subdir_is_file_prefix.svndump", subdir="flaf"
        )
        repourl = test_hgsubversion_util.fileurl(repopath)
        ui = self.ui()

        # rebuildmeta with original repo
        svncommands.rebuildmeta(ui, repo=self.repo, args=[repourl])
        old_subdir = svnmeta.SVNMeta(self.repo).subdir
        # remove metadata, rebuild with a wrong subdir config
        # this should raise
        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        ui.setconfig("hgsubversion", "reposubdir", "new_wrong_subdir")
        self.assertRaises(
            AssertionError,
            svncommands.rebuildmeta,
            ui=ui,
            repo=self.repo,
            args=[repourl],
        )
        # remove metadata, rebuild with the correct subdir config
        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        ui.setconfig("hgsubversion", "reposubdir", old_subdir)
        svncommands.rebuildmeta(ui, repo=self.repo, args=[repourl])
        new_subdir = svnmeta.SVNMeta(self.repo).subdir
        self.assertEqual(old_subdir, new_subdir)
        # remove metadata, rebuild with the correct subdir config, with a unrealted url
        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        ui.setconfig("hgsubversion", "reposubdir", old_subdir)
        ui.setconfig("hgsubversion", "repouuid", "924a052a-5e5a-4a8e-a677-da5565bec340")
        svncommands.rebuildmeta(ui, repo=self.repo, args=[otherurl])
        another_subdir = svnmeta.SVNMeta(self.repo).subdir
        self.assertEqual(old_subdir, another_subdir)

    def test_svn_repouuid_config(self):
        otherpath = self.load_svndump("binaryfiles-broken.svndump")
        otherurl = test_hgsubversion_util.fileurl(otherpath)
        _, repopath = self.load_and_fetch("replace_trunk_with_branch.svndump")
        repourl = test_hgsubversion_util.fileurl(repopath)
        # rebuildmeta with original repo
        svncommands.rebuildmeta(self.ui(), repo=self.repo, args=[])
        # updatemeta with the wrong, pre-configured uuid
        ui = self.ui()
        ui.setconfig("hgsubversion", "repouuid", "a wrong uuid")
        self.assertRaises(
            hgutil.Abort, svncommands.updatemeta, ui=ui, repo=self.repo, args=[repourl]
        )
        # updatemeta with the right, pre-configured uuid
        ui.setconfig("hgsubversion", "repouuid", "5b65bade-98f3-4993-a01f-b7a6710da339")
        svncommands.rebuildmeta(ui, repo=self.repo, args=[repourl])
        # updatemeta with the right, pre-configured uuid, and unrelated repo
        ui.setconfig("hgsubversion", "repouuid", "5b65bade-98f3-4993-a01f-b7a6710da339")
        svncommands.rebuildmeta(ui, repo=self.repo, args=[otherurl])
        # updatemeta with empty meta
        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        ui.setconfig("hgsubversion", "repouuid", "5b65bade-98f3-4993-a01f-b7a6710da339")
        svncommands.rebuildmeta(ui, repo=self.repo, args=[otherurl])

    def test_svn_repouuid_reposuburl_config_not_accessing_svn(self):
        _, repopath = self.load_and_fetch(
            "subdir_is_file_prefix.svndump", subdir="flaf"
        )
        repourl = test_hgsubversion_util.fileurl(repopath)
        ui = self.ui()

        # rebuildmeta with original repo
        svncommands.rebuildmeta(ui, repo=self.repo, args=[repourl])
        subdir = svnmeta.SVNMeta(self.repo).subdir
        uuid = svnmeta.SVNMeta(self.repo).uuid
        # remove metadata, rebuild without configs.
        # In attempt to use the some_random_string as url, this should raise.
        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        self.assertRaises(
            hgutil.Abort,
            svncommands.rebuildmeta,
            ui=ui,
            repo=self.repo,
            args=["svn+ssh://some_random_string"],
        )
        # remove metadata, rebuild with the correct configs
        # This time it doesn't try to use the wrong url, this should pass.
        test_hgsubversion_util.rmtree(self.repo.sharedvfs.join("svn"))
        ui.setconfig("hgsubversion", "reposubdir", subdir)
        ui.setconfig("hgsubversion", "repouuid", uuid)
        svncommands.rebuildmeta(
            ui=ui, repo=self.repo, args=["svn+ssh://some_random_string"]
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
