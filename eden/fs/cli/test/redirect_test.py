#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.fs.cli.redirect import RedirectionType, RedirectionState
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from ..redirect import RepoPathDisposition, Redirection


class RedirectTest(unittest.TestCase, TemporaryDirectoryMixin):
    @patch("eden.fs.cli.redirect.Redirection._bind_unmount")
    @patch("eden.fs.cli.redirect.RepoPathDisposition.analyze")
    @patch("eden.fs.cli.redirect.Redirection.expand_repo_path")
    @patch("eden.fs.cli.buck.is_buckd_running_for_path")
    def test_twice_failed_bind_unmount(
        self,
        mock_buckd_running: MagicMock,
        mock_expand_path: MagicMock,
        mock_analyze: MagicMock,
        mock_bind_unmount: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")

        mock_bind_unmount.return_value = None
        mock_analyze.return_value = RepoPathDisposition.IS_BIND_MOUNT
        mock_expand_path.return_value = Path(repo_path)
        mock_buckd_running.return_value = False

        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        redir = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )

        with self.assertRaises(Exception) as ex:
            redir.remove_existing(checkout)

        error_msg = f"Failed to remove {repo_path} since the bind unmount failed"
        self.assertEqual(str(ex.exception), error_msg)
