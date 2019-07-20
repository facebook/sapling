# @nolint

from __future__ import absolute_import

import os

import test_hgsubversion_util
from edenscm.mercurial import commands, dispatch, hg


def _dispatch(ui, cmd):
    assert "--quiet" in cmd
    try:
        req = dispatch.request(cmd, ui=ui)
        req.earlyoptions = {
            "config": [],
            "configfile": [],
            "cwd": "",
            "debugger": False,
            "profile": False,
            "repository": "",
        }
        dispatch._dispatch(req)
    except AttributeError:
        dispatch._dispatch(ui, cmd)


class TestMercurialCore(test_hgsubversion_util.TestBase):
    """
    Test that the core Mercurial operations aren't broken by hgsubversion.
    """

    @test_hgsubversion_util.requiresoption("updaterev")
    def test_update(self):
        """ Test 'clone --updaterev' """
        ui = self.ui()
        _dispatch(ui, ["init", "--quiet", self.wc_path])
        repo = self.repo
        repo.ui.setconfig("ui", "username", "anonymous")

        fpath = os.path.join(self.wc_path, "it")
        f = file(fpath, "w")
        f.write("C1")
        f.flush()
        commands.add(ui, repo)
        commands.commit(ui, repo, message="C1")
        f.write("C2")
        f.flush()
        commands.commit(ui, repo, message="C2")
        f.write("C3")
        f.flush()
        commands.commit(ui, repo, message="C3")

        self.assertEqual(test_hgsubversion_util.repolen(repo), 3)

        updaterev = 1
        _dispatch(
            ui,
            [
                "clone",
                "--quiet",
                self.wc_path,
                self.wc_path + "2",
                "--updaterev=%s" % updaterev,
            ],
        )

        repo2 = hg.repository(ui, self.wc_path + "2")

        self.assertEqual(str(repo[updaterev]), str(repo2["."]))


if __name__ == "__main__":
    import silenttestrunner

    silenttestrunner.main(__name__)
