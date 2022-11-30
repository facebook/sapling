#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import argparse
import os
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.fs.cli.redirect import FixupCmd, RedirectionState, RedirectionType
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from ..redirect import Redirection, RepoPathDisposition


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

    @patch("eden.fs.cli.redirect.Redirection.apply")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.cmd_util.require_checkout")
    @patch("eden.fs.cli.redirect.get_effective_redirections")
    def test_fixup_all_resources(
        self,
        mock_get_effective_redirections: MagicMock,
        mock_require_checkout: MagicMock,
        mock_remove_existing: MagicMock,
        mock_apply: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")
        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        eden_path = os.path.join(temp_dir, "mount_dir")
        mock_require_checkout.return_value = (instance, checkout, eden_path)

        mock_argument_parser = MagicMock(spec=argparse.ArgumentParser)
        args = argparse.Namespace(mount=eden_path, only_repo_source=False)

        redir = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )
        mock_get_effective_redirections.return_value = {repo_path: redir}

        test_fixup_cmd = FixupCmd(mock_argument_parser)
        test_fixup_cmd.run(args)

        mock_remove_existing.assert_called_once()
        mock_apply.assert_called_once()

    @patch("eden.fs.cli.redirect.Redirection.apply")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.cmd_util.require_checkout")
    @patch("eden.fs.cli.redirect.get_effective_redirections")
    def test_fixup_only_eden_redirection(
        self,
        mock_get_effective_redirections: MagicMock,
        mock_require_checkout: MagicMock,
        mock_remove_existing: MagicMock,
        mock_apply: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")
        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        eden_path = os.path.join(temp_dir, "mount_dir")
        mock_require_checkout.return_value = (instance, checkout, eden_path)

        mock_argument_parser = MagicMock(spec=argparse.ArgumentParser)
        args = argparse.Namespace(mount=eden_path, only_repo_source=True)

        redir = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )
        mock_get_effective_redirections.return_value = {repo_path: redir}

        test_fixup_cmd = FixupCmd(mock_argument_parser)
        test_fixup_cmd.run(args)

        mock_remove_existing.assert_not_called()
        mock_apply.assert_not_called()

    @patch("eden.fs.cli.redirect.Redirection.apply")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.cmd_util.require_checkout")
    @patch("eden.fs.cli.redirect.get_effective_redirections")
    def test_fixup_dir_in_eden_redirection(
        self,
        mock_get_effective_redirections: MagicMock,
        mock_require_checkout: MagicMock,
        mock_remove_existing: MagicMock,
        mock_apply: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")
        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        eden_path = os.path.join(temp_dir, "mount_dir")
        mock_require_checkout.return_value = (instance, checkout, eden_path)

        mock_argument_parser = MagicMock(spec=argparse.ArgumentParser)
        args = argparse.Namespace(mount=eden_path, only_repo_source=True)

        redir = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source=".eden-redirections",
            state=RedirectionState.UNKNOWN_MOUNT,
        )
        mock_get_effective_redirections.return_value = {repo_path: redir}

        test_fixup_cmd = FixupCmd(mock_argument_parser)
        test_fixup_cmd.run(args)

        mock_remove_existing.assert_called_once()
        mock_apply.assert_called_once()
