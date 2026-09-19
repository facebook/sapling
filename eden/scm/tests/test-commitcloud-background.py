# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest
from types import SimpleNamespace
from unittest.mock import patch

import silenttestrunner
from sapling import ui as uimod
from sapling.ext.commitcloud import background


class BackgroundBackupTest(unittest.TestCase):
    def setUp(self):
        self.ui = uimod.ui()
        self.repo = SimpleNamespace(ui=self.ui, sharedroot="/checkout/repo")
        self.ui.setconfig("infinitepushbackup", "logdir", "")
        self.ui.setconfig("infinitepushbackup", "bgdebug", False)
        self.ui.setconfig("infinitepushbackup", "bgdebuglocks", False)
        self.ui.setconfig("infinitepush", "bgssh", "")
        self.ui.setconfig("commitcloud", "autojoinworkspace", "user/test/raw")

    def backgroundcommand(self):
        with (
            patch.object(background.bindings, "process") as process,
            patch.object(background.util, "hgcmd", return_value=["sl"]),
        ):
            background.backgroundbackup(self.repo, reason="commit")
            process.Command.new.assert_called_once_with("sl")
            return process.Command.new.return_value.args.call_args.args[0]

    @patch.object(background.workspace, "currentworkspace", return_value=None)
    @patch.object(background.service, "get")
    def test_existing_workspace_joins(self, service, currentworkspace):
        self.assertEqual(
            self.backgroundcommand(),
            ["cloud", "join", "--raw-workspace", "user/test/raw"],
        )
        service.return_value.getworkspace.assert_called_once_with(
            "repo", "user/test/raw"
        )

    @patch.object(background.workspace, "currentworkspace", return_value=None)
    @patch.object(background.service, "get")
    def test_missing_workspace_falls_back_to_upload(self, service, currentworkspace):
        service.return_value.getworkspace.return_value = None
        self.assertEqual(self.backgroundcommand(), ["cloud", "upload"])

    @patch.object(background.workspace, "currentworkspace", return_value=None)
    @patch.object(background.service, "get")
    def test_lookup_failure_falls_back_to_upload(self, service, currentworkspace):
        service.side_effect = background.error.HttpError("offline")
        with patch.object(self.ui, "warn") as warn:
            self.assertEqual(self.backgroundcommand(), ["cloud", "upload"])
            self.assertIn("user/test/raw", warn.call_args.args[0])
            self.assertIn("offline", warn.call_args.args[0])


if __name__ == "__main__":
    silenttestrunner.main(__name__)
