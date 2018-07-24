import test_hgsubversion_util
from mercurial import commands, hg, ui


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

    def _assert_globalrev(self, repo, expected_log):
        class CapturingUI(ui.ui):
            def __init__(self, *args, **kwds):
                super(CapturingUI, self).__init__(*args, **kwds)
                self._output = ""

            def write(self, msg, *args, **kwds):
                self._output += msg

        capturing_ui = CapturingUI()
        defaults = {"date": None, "rev": None, "user": None, "graph": True}
        commands.log(
            capturing_ui,
            repo,
            template=("  rev:{rev} globalrev:{globalrev}\n"),
            **defaults
        )

        self.assertEqual(capturing_ui._output.strip(), expected_log.strip())

    def test_nochanges(self):
        repo, _ = self._loadupdate("single_rev.svndump")
        state = repo[None].parents()
        _pull(repo)
        self.assertEqual(state, repo[None].parents())
        self._assert_globalrev(
            repo,
            """
  rev:0 globalrev:
@
""",
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
  rev:2 globalrev:5001
o
|
  rev:1 globalrev:5000
o
|
  rev:0 globalrev:
@
""",
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
  rev:2 globalrev:5001
@
|
  rev:1 globalrev:5000
o
|
  rev:0 globalrev:
o
""",
        )


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
