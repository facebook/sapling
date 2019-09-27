from __future__ import absolute_import

import test_hgsubversion_util
from edenscm.mercurial import hg


class _DbCommand(object):
    def __init__(self, ui, hg_repo_path):
        self.ui = ui
        self.hg_repo_path = hg_repo_path

    def execute(self, cmd, start):
        full_cmd = [
            "--quiet",
            "--cwd",
            self.hg_repo_path,
            cmd,
            "--i-know-what-i-am-doing",
            str(start),
        ]

        r = test_hgsubversion_util.dispatch(full_cmd, self.ui)
        assert not r, "%s failed for %s" % (cmd, self.hg_repo_path)


def _pull(repo, update=False):
    cmd = ["--quiet", "pull"]
    if update:
        cmd.append("--update")

    r = test_hgsubversion_util.dispatch(cmd, repo.ui, repo)
    assert not r, "pull of %s failed" % repo.root


class TestGlobalRev(test_hgsubversion_util.TestBase):
    def setUp(self):
        super(TestGlobalRev, self).setUp()
        self.ui = self._testui()

    def _testui(self):
        def _set_globalrevs_config(ui):
            ui.setconfig("extensions", "globalrevs", "")
            ui.setconfig("extensions", "hgsql", "")
            ui.setconfig("extensions", "pushrebase", "")
            ui.setconfig("globalrevs", "onlypushrebase", False)
            ui.setconfig("globalrevs", "startrev", 5000)
            _set_hgsql_config(ui)

        def _set_hgsql_config(ui):
            db = test_hgsubversion_util.TestDb()

            ui.setconfig("hgsql", "database", db.name)
            ui.setconfig("hgsql", "enabled", True)
            ui.setconfig("hgsql", "engine", db.engine)
            ui.setconfig("hgsql", "host", db.host)
            ui.setconfig("hgsql", "password", db.password)
            ui.setconfig("hgsql", "port", db.port)
            ui.setconfig("hgsql", "user", db.user)

            # The database name is random right now. So, lets just use it for the
            # repository name as well.
            ui.setconfig("hgsql", "reponame", db.name)

        ui = test_hgsubversion_util.testui()
        _set_globalrevs_config(ui)
        return ui

    def _loadupdate(self, fixture_name, *args, **kwargs):
        kwargs = kwargs.copy()
        kwargs.update(noupdate=False)
        hg_repo_path, svn_repo_path = self.load_and_clone(fixture_name, *args, **kwargs)

        def _init_db(ui, hg_repo_path):
            dbcommand = _DbCommand(ui, hg_repo_path)
            dbcommand.execute("sqlrefill", 0)
            dbcommand.execute("initglobalrev", 5000)

        _init_db(self.ui, hg_repo_path)
        repo = hg.repository(self.ui, hg_repo_path)
        return repo, svn_repo_path

    def _assert_globalrev(self, repo, expected_log, rev=None, showgraph=False):
        cmd = ["log", "--quiet"]

        if showgraph:
            cmd.append("-G")

        if rev is not None:
            cmd.append("-r")
            cmd.append(rev)

        cmd.append("-T")
        cmd.append("svnrev:{svnrev} globalrev:{globalrev}\n")

        ui = repo.ui
        ui.pushbuffer()
        test_hgsubversion_util.dispatch(cmd, ui, repo)
        self.assertEqual(ui.popbuffer().strip(), expected_log.strip())

    def test_nochanges(self):
        repo, _ = self._loadupdate("single_rev.svndump")
        state = repo[None].parents()
        _pull(repo)
        self.assertEqual(state, repo[None].parents())
        self._assert_globalrev(
            repo,
            """
@  svnrev:2 globalrev:2

""",
            showgraph=True,
        )

    def test_pull_noupdate(self):
        repo, repo_path = self._loadupdate("single_rev.svndump")
        state = repo[None].parents()
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed"})
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed Again"})
        _pull(repo)
        self.assertEqual(state, repo[None].parents())
        self.assertTrue("tip" not in repo["."].tags())
        self._assert_globalrev(
            repo,
            """
o  svnrev:4 globalrev:5001
|
o  svnrev:3 globalrev:5000
|
@  svnrev:2 globalrev:2
""",
            showgraph=True,
        )

    def test_pull_doupdate(self):
        repo, repo_path = self._loadupdate("single_rev.svndump")
        state = repo[None].parents()
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed"})
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed Again"})
        _pull(repo, update=True)
        self.failIfEqual(state, repo[None].parents())
        self.assertTrue("tip" in repo["."].tags())
        self._assert_globalrev(
            repo,
            """
@  svnrev:4 globalrev:5001
|
o  svnrev:3 globalrev:5000
|
o  svnrev:2 globalrev:2
""",
            showgraph=True,
        )

    def test_revsets_interoperability(self):
        repo, repo_path = self._loadupdate("single_rev.svndump")
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed"})
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed Again"})
        self.add_svn_rev(repo_path, {"trunk/alpha": "Changed Once Again"})
        _pull(repo)
        expected_out = """
  svnrev:4 globalrev:5001
"""

        self._assert_globalrev(repo, expected_out, rev="r4")
        self._assert_globalrev(repo, expected_out, rev="m4")

        self._assert_globalrev(repo, expected_out, rev="m5001")
        self._assert_globalrev(repo, expected_out, rev="r5001")

        self._assert_globalrev(repo, expected_out, rev="svnrev(4)")
        self._assert_globalrev(repo, expected_out, rev="svnrev(5001)")

        self._assert_globalrev(repo, expected_out, rev="globalrev(5001)")
        self._assert_globalrev(repo, expected_out, rev="globalrev(4)")


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
