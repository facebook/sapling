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
from eden.fs.cli.redirect import (
    check_redirection,
    FixupCmd,
    get_effective_redirections,
    RedirectionState,
    RedirectionType,
)
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from ..redirect import Redirection, RepoPathDisposition


class RedirectTest(unittest.TestCase, TemporaryDirectoryMixin):
    def test_darwin_symlink_bind_redirection_matches_configuration(self) -> None:
        temp_dir = self.make_temporary_directory()
        checkout_path = Path(temp_dir) / "checkout"
        checkout_path.mkdir()
        target = Path(temp_dir) / "target"
        target.mkdir()
        (checkout_path / "foo").symlink_to(target)

        instance = MagicMock()
        instance.get_mount_paths.return_value = []
        instance.get_config_value.return_value = "symlink"
        checkout = MagicMock()
        checkout.path = checkout_path
        checkout.instance = instance
        redir = Redirection(
            repo_path=Path("foo"),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
        )
        mount_table = MagicMock()
        mount_table.read.return_value = []

        with (
            patch("eden.fs.cli.redirect.sys.platform", "darwin"),
            patch(
                "eden.fs.cli.redirect.get_configured_redirections",
                return_value={"foo": redir},
            ),
            patch("eden.fs.cli.redirect.make_scratch_dir", return_value=target),
        ):
            redirs = get_effective_redirections(checkout, mount_table, instance)

        # FIXME: This should be MATCHES_CONFIGURATION once macOS bind
        # redirections backed by symlinks use symlink state detection.
        self.assertEqual(redirs["foo"].state, RedirectionState.NOT_MOUNTED)

    @patch("eden.fs.cli.redirect.Redirection._bind_unmount")
    @patch("eden.fs.cli.redirect.RepoPathDisposition.analyze")
    @patch("eden.fs.cli.redirect.Redirection.expand_repo_path")
    @patch("eden.fs.cli.buck.is_buckd_running_for_repo")
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

        mock_remove_existing.assert_not_called()
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

        mock_remove_existing.assert_not_called()
        mock_apply.assert_called_once()

    @patch("eden.fs.cli.redirect.Redirection.apply")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.cmd_util.require_checkout")
    @patch("eden.fs.cli.redirect.get_effective_redirections")
    def test_fixup_unknown_mount(
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
            redir_type=RedirectionType.UNKNOWN,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )
        mock_get_effective_redirections.return_value = {repo_path: redir}

        test_fixup_cmd = FixupCmd(mock_argument_parser)
        test_fixup_cmd.run(args)

        mock_remove_existing.assert_called_once()
        mock_apply.assert_not_called()

    @patch("eden.fs.cli.redirect.is_bind_mount")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.redirect.Redirection.apply")
    def test_misconfigured_redirection_bind_unknown(
        self,
        mock_apply: MagicMock,
        mock_remove_existing: MagicMock,
        mock_is_bind_mount: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")

        mock_is_bind_mount.side_effect = [False, True]
        mock_remove_existing.return_value = None
        mock_apply.return_value = None

        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        redir_fixed = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )
        redir_broken = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )

        result_fixed = check_redirection(redir_fixed, checkout)
        self.assertEqual(result_fixed, True)

        result_broken = check_redirection(redir_broken, checkout)
        self.assertEqual(result_broken, False)

    @patch("eden.fs.cli.redirect.is_bind_mount")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.redirect.Redirection.apply")
    def test_misconfigured_redirection_bind_not_mounted(
        self,
        mock_apply: MagicMock,
        mock_remove_existing: MagicMock,
        mock_is_bind_mount: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")

        mock_is_bind_mount.side_effect = [True, False]
        mock_remove_existing.return_value = None
        mock_apply.return_value = None

        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        redir_fixed = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.NOT_MOUNTED,
        )
        redir_broken = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.NOT_MOUNTED,
        )

        result_fixed = check_redirection(redir_fixed, checkout)
        self.assertEqual(result_fixed, True)

        result_broken = check_redirection(redir_broken, checkout)
        self.assertEqual(result_broken, False)

    @patch("pathlib.Path.readlink")
    @patch("eden.fs.cli.redirect.Redirection.remove_existing")
    @patch("eden.fs.cli.redirect.Redirection.apply")
    def test_misconfigured_redirection_symlink_symlink_missing(
        self,
        mock_apply: MagicMock,
        mock_remove_existing: MagicMock,
        mock_readlink: MagicMock,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        repo_path = os.path.join(temp_dir, "test")

        mock_remove_existing.return_value = None
        mock_apply.return_value = None

        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        redir_fixed = Redirection(
            repo_path=Path(repo_path),
            redir_type=RedirectionType.SYMLINK,
            target=None,
            source="mount",
            state=RedirectionState.SYMLINK_MISSING,
        )
        mock_readlink.return_value = redir_fixed.expand_target_abspath(checkout)

        result_fixed = check_redirection(redir_fixed, checkout)
        self.assertEqual(result_fixed, True)
