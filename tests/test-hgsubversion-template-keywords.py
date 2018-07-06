# no-check-code -- see T24862348

import test_hgsubversion_util
from mercurial import commands, error, ui


try:
    from mercurial import templatekw

    templatekw.keywords
except ImportError:
    templatekw = None

try:
    from mercurial import revset

    revset.methods
except ImportError:
    revset = None


class CapturingUI(ui.ui):
    def __init__(self, *args, **kwds):
        super(CapturingUI, self).__init__(*args, **kwds)
        self._output = ""

    def write(self, msg, *args, **kwds):
        self._output += msg


class TestLogKeywords(test_hgsubversion_util.TestBase):
    @test_hgsubversion_util.requiresmodule(templatekw)
    def test_svn_keywords(self):
        defaults = {"date": None, "rev": None, "user": None, "graph": True}
        repo = self._load_fixture_and_fetch("two_revs.svndump")

        # we want one commit that isn't from Subversion
        self.commitchanges([("foo", "foo", "frobnicate\n")])

        ui = CapturingUI()
        commands.log(ui, repo, template=("  rev: {rev} svnrev:{svnrev}\n"), **defaults)
        print(ui._output)
        self.assertEqual(
            ui._output.strip(),
            """
  rev: 2 svnrev:
@
|
  rev: 1 svnrev:3
o
|
  rev: 0 svnrev:2
o
""".strip(),
        )

    @test_hgsubversion_util.requiresmodule(revset)
    @test_hgsubversion_util.requiresmodule(templatekw)
    def test_svn_revsets(self):
        repo = self._load_fixture_and_fetch("two_revs.svndump")

        # we want one commit that isn't from Subversion
        self.commitchanges([("foo", "foo", "frobnicate\n")])

        defaults = {"date": None, "rev": ["fromsvn()"], "user": None}

        ui = CapturingUI()
        commands.log(ui, repo, template="{rev}:{svnrev} ", **defaults)
        self.assertEqual(ui._output, "0:2 1:3 ")

        defaults = {"date": None, "rev": ["svnrev(2)"], "user": None}

        ui = CapturingUI()
        commands.log(ui, repo, template="{rev}:{svnrev} ", **defaults)
        self.assertEqual(ui._output, "0:2 ")

        defaults = {"date": None, "rev": ["fromsvn(1)"], "user": None}

        self.assertRaises(
            error.ParseError,
            commands.log,
            self.ui(),
            repo,
            template="{rev}:{svnrev} ",
            **defaults
        )

        defaults = {"date": None, "rev": ["svnrev(1, 2)"], "user": None}

        self.assertRaises(
            error.ParseError,
            commands.log,
            self.ui(),
            repo,
            template="{rev}:{svnrev} ",
            **defaults
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
