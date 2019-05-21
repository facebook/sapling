# no-check-code -- see T24862348
# @nolint

from __future__ import absolute_import

import cStringIO
import difflib
import os

import test_hgsubversion_util
from edenscm.hgext.hgsubversion import compathacks, svncommands
from edenscm.mercurial import commands, error


class TestTags(test_hgsubversion_util.TestBase):
    stupid_mode_tests = True

    def test_tags(self):
        repo = self._load_fixture_and_fetch("basic_tag_tests.svndump")
        self.assertEqual(sorted(repo.tags()), ["copied_tag", "tag_r3", "tip"])
        self.assertEqual(repo["tag_r3"], repo["copied_tag"])
        self.assertEqual(repo["tag_r3"].rev(), 1)

    def test_remove_tag(self):
        repo = self._load_fixture_and_fetch("remove_tag_test.svndump")
        self.assertEqual(repo["tag_r3"].rev(), 1)
        self.assert_("copied_tag" not in repo.tags())

    def test_rename_tag(self):
        expected = """\
node: hg=default@2:svn=trunk@4
tagging r3
  tag_r3: hg=default@1:svn=trunk@3

node: hg=default@3:svn=trunk@5
tag from a tag
  tag_r3: hg=default@1:svn=trunk@3
  copied_tag: hg=default@1:svn=trunk@3

node: hg=default@4:svn=trunk@6
rename a tag
  tag_r3: hg=default@1:svn=trunk@3
  copied_tag: hg=default@1:svn=trunk@3
  copied_tag: hg=default@-1:svn=unk@unk
  other_tag_r3: hg=default@1:svn=trunk@3
"""
        self._test_tags("rename_tag_test.svndump", expected)

    def test_tags_in_unusual_location(self):
        repo = self._load_fixture_and_fetch("unusual_tags.svndump")
        branches = set(repo[h].extra()["branch"] for h in repo.heads())
        self.assertEqual(branches, set(["default", "dev_branch"]))
        tags = repo.tags()
        del tags["tip"]
        self.assertEqual(
            tags,
            {
                "blah/trunktag": "\xd3$@\xd7\xd8Nu\xce\xa6%\xf1u\xd9b\x1a\xb2\x81\xc2\xb0\xb1",
                "versions/branch_version": "I\x89\x1c>z#\xfc._K#@:\xd6\x1f\x96\xd6\x83\x1b|",
            },
        )

    def test_most_recent_is_edited(self):
        repo, repo_path = self.load_and_fetch("most-recent-is-edit-tag.svndump")
        self.repo.ui.status(
            "Note: this test failing may be because of a rebuildmeta failure.\n"
            "You should check that before assuming issues with this test.\n"
        )
        wc2_path = self.wc_path + "2"
        src, dest = test_hgsubversion_util.hgclone(
            repo.ui, self.wc_path, wc2_path, update=False
        )
        dest = test_hgsubversion_util.getlocalpeer(dest)
        svncommands.rebuildmeta(
            repo.ui, dest, args=[test_hgsubversion_util.fileurl(repo_path)]
        )
        commands.pull(self.repo.ui, self.repo)
        dtags, srctags = dest.tags(), self.repo.tags()
        dtags.pop("tip")
        srctags.pop("tip")
        self.assertEqual(dtags, srctags)
        self.assertEqual(dest.heads(), self.repo.heads())

    def test_tags_in_unusual_location(self):
        repo = self._load_fixture_and_fetch("tag_name_same_as_branch.svndump")
        self.assertEqual(len(repo.heads()), 1)
        branches = set(repo[h].extra()["branch"] for h in repo.heads())
        self.assertEqual(branches, set(["magic"]))
        tags = repo.tags()
        del tags["tip"]
        self.assertEqual(
            tags,
            {
                "magic": "\xa2b\xb9\x03\xc6\xbd\x903\x95\xf5\x0f\x94\xcey\xc4E\xfaE6\xaa",
                "magic2": "\xa3\xa2D\x86aM\xc0v\xb9\xb0\x18\x14\xad\xacwBUi}\xe2",
            },
        )

    def test_old_tag_map_aborts(self):
        repo = self._load_fixture_and_fetch("tag_name_same_as_branch.svndump")
        tm = os.path.join(repo.path, "svn", "tagmap")
        open(tm, "w").write("1\n")
        # force tags to load since it is lazily loaded when needed
        self.assertRaises(error.Abort, lambda: repo.svnmeta().tags)

    def _debug_print_tags(self, repo, ctx, fp):
        def formatnode(ctx):
            crev = ctx.extra().get("convert_revision", "unk/unk@unk")
            path, rev = crev.rsplit("@", 1)
            path = path.split("/", 1)[-1]
            branch = ctx.branch() or "default"
            return "hg=%s@%d:svn=%s@%s" % (branch, ctx.rev(), path, rev)

        w = fp.write
        desc = ctx.description().splitlines()[0].strip()
        if ".hgtags" not in ctx or not ctx[".hgtags"].data().strip():
            return
        w("node: %s\n" % formatnode(ctx))
        w("%s\n" % desc)
        for line in ctx[".hgtags"].data().splitlines(False):
            node, name = line.split(None, 1)
            w("  %s: %s\n" % (name, formatnode(repo[node])))
        w("\n")

    def _test_tags(self, testpath, expected):
        repo = self._load_fixture_and_fetch(testpath)
        fp = cStringIO.StringIO()
        for r in repo:
            self._debug_print_tags(repo, repo[r], fp=fp)
        output = fp.getvalue().strip()
        expected = expected.strip()
        if expected == output:
            return
        expected = expected.splitlines()
        output = output.splitlines()
        diff = difflib.unified_diff(expected, output, "expected", "output")
        self.assert_(False, "\n" + "\n".join(diff))

    def test_tagging_into_tag(self):
        expected = """\
node: hg=test@2:svn=branches/test@4
First tag.
  test-0.1: hg=test@1:svn=branches/test@3

node: hg=test@3:svn=branches/test@5
Weird tag.
  test-0.1: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=test@1:svn=branches/test@3

node: hg=test@4:svn=branches/test@6
Fix tag pt 1.
  test-0.1: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=default@-1:svn=unk@unk
  test-0.1-real: hg=test@1:svn=branches/test@3

node: hg=test@5:svn=branches/test@7
Remove weird.
  test-0.1: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=default@-1:svn=unk@unk
  test-0.1-real: hg=test@1:svn=branches/test@3
  test-0.1: hg=default@-1:svn=unk@unk

node: hg=test@6:svn=branches/test@8
Fix tag pt 2.
  test-0.1: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=test@1:svn=branches/test@3
  test-0.1/test: hg=default@-1:svn=unk@unk
  test-0.1-real: hg=test@1:svn=branches/test@3
  test-0.1: hg=default@-1:svn=unk@unk
  test-0.1-real: hg=default@-1:svn=unk@unk
  test-0.1: hg=test@1:svn=branches/test@3
"""
        self._test_tags("renametagdir.svndump", expected)


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
