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
from eden.fs.cli.redirect import (
    check_redirection,
    determine_bind_redirection_type,
    get_effective_redirections,
    is_valid_symlink,
    RedirectionState,
    RedirectionType,
)
from eden.test_support.temporary_directory import TemporaryDirectoryMixin

from ..redirect import Redirection, RepoPathDisposition


class RedirectTest(unittest.TestCase, TemporaryDirectoryMixin):
    def test_symlink_target_comparison_resolves_parent_symlinks(self) -> None:
        temp_dir = Path(self.make_temporary_directory())
        target_root = temp_dir / "target-root"
        target_root.mkdir()
        target_alias = temp_dir / "target-alias"
        target_alias.symlink_to(target_root)
        expected_target = target_alias / "target"
        expected_target.mkdir()
        symlink_path = temp_dir / "redirect"
        symlink_path.symlink_to(expected_target)

        self.assertTrue(is_valid_symlink(expected_target, symlink_path))

    @patch("eden.fs.cli.redirect.have_apfs_helper")
    def test_symlink_bind_redirection_does_not_require_helper(
        self, mock_have_apfs_helper: MagicMock
    ) -> None:
        instance = MagicMock()
        instance.get_config_value.return_value = "symlink"

        self.assertEqual(determine_bind_redirection_type(instance), "symlink")
        mock_have_apfs_helper.assert_not_called()

    def test_darwin_symlink_bind_redirection_resolves_target(self) -> None:
        temp_dir = self.make_temporary_directory()
        checkout_path = Path(temp_dir) / "checkout"
        checkout_path.mkdir()
        target_root = Path(temp_dir) / "target-root"
        target_root.mkdir()
        target_alias = Path(temp_dir) / "target-alias"
        target_alias.symlink_to(target_root)
        target = target_alias / "target"
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

        self.assertEqual(redirs["foo"].state, RedirectionState.MATCHES_CONFIGURATION)

    def test_darwin_symlink_bind_redirection_matches_after_config_unset(
        self,
    ) -> None:
        temp_dir = self.make_temporary_directory()
        checkout_path = Path(temp_dir) / "checkout"
        checkout_path.mkdir()
        target = Path(temp_dir) / "target"
        target.mkdir()
        (checkout_path / "foo").symlink_to(target)

        instance = MagicMock()
        instance.get_mount_paths.return_value = []
        instance.get_config_value.side_effect = lambda _key, default: default
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
            patch(
                "eden.fs.cli.redirect.make_scratch_dir", return_value=target
            ) as mock_make_scratch_dir,
        ):
            redirs = get_effective_redirections(checkout, mount_table, instance)

        self.assertEqual(redirs["foo"].state, RedirectionState.MATCHES_CONFIGURATION)
        mock_make_scratch_dir.assert_called_once_with(
            checkout, Path("foo"), no_create=True
        )

    def test_dangling_symlink_redirection_is_missing(self) -> None:
        temp_dir = self.make_temporary_directory()
        checkout_path = Path(temp_dir) / "checkout"
        checkout_path.mkdir()
        target = Path(temp_dir) / "missing-target"
        (checkout_path / "foo").symlink_to(target)

        instance = MagicMock()
        instance.get_mount_paths.return_value = []
        checkout = MagicMock()
        checkout.path = checkout_path
        checkout.instance = instance
        redir = Redirection(
            repo_path=Path("foo"),
            redir_type=RedirectionType.SYMLINK,
            target=None,
            source="mount",
        )
        mount_table = MagicMock()
        mount_table.read.return_value = []

        with (
            patch(
                "eden.fs.cli.redirect.get_configured_redirections",
                return_value={"foo": redir},
            ),
            patch("eden.fs.cli.redirect.make_scratch_dir", return_value=target),
        ):
            redirs = get_effective_redirections(checkout, mount_table, instance)

        self.assertEqual(redirs["foo"].state, RedirectionState.SYMLINK_MISSING)

    def test_apply_repairs_dangling_darwin_symlink_bind_redirection(self) -> None:
        temp_dir = self.make_temporary_directory()
        checkout_path = Path(temp_dir) / "checkout"
        checkout_path.mkdir()
        target = Path(temp_dir) / "missing-target"
        symlink_path = checkout_path / "foo"
        symlink_path.symlink_to(target)

        instance = MagicMock()
        instance.get_config_value.return_value = "symlink"
        checkout = MagicMock()
        checkout.path = checkout_path
        checkout.instance = instance
        redir = Redirection(
            repo_path=Path("foo"),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.SYMLINK_MISSING,
        )

        def make_scratch_dir(
            _checkout: MagicMock, _subdir: Path, *, no_create: bool = False
        ) -> Path:
            self.assertFalse(no_create)
            target.mkdir()
            return target

        with (
            patch("eden.fs.cli.redirect.sys.platform", "darwin"),
            patch(
                "eden.fs.cli.redirect.make_scratch_dir",
                side_effect=make_scratch_dir,
            ),
        ):
            redir.apply(checkout)

        self.assertEqual(symlink_path.readlink(), target)

    @patch("eden.fs.cli.redirect.run_cmd_quietly")
    def test_bind_unmount_darwin_ignores_configured_backing(
        self, mock_run_cmd_quietly: MagicMock
    ) -> None:
        checkout = MagicMock()
        checkout.path = Path("/checkout")
        checkout.instance.get_config_value.return_value = "symlink"
        redir = Redirection(
            repo_path=Path("foo"),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.UNKNOWN_MOUNT,
        )

        redir._bind_unmount_darwin(checkout)

        mock_run_cmd_quietly.assert_called_once_with(
            ["diskutil", "unmount", "force", Path("/checkout/foo")]
        )
        checkout.instance.get_config_value.assert_not_called()

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

    def test_misconfigured_redirection_darwin_symlink_backed_bind(self) -> None:
        temp_dir = self.make_temporary_directory()
        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("checkout")
        target = Path(temp_dir) / "target"
        target.mkdir()
        (checkout.path / "foo").symlink_to(target)
        redir = Redirection(
            repo_path=Path("foo"),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.SYMLINK_MISSING,
        )

        with (
            patch("eden.fs.cli.redirect.sys.platform", "darwin"),
            patch("eden.fs.cli.redirect.make_scratch_dir", return_value=target),
        ):
            self.assertTrue(check_redirection(redir, checkout))

    @patch("eden.fs.cli.redirect.is_valid_windows_symlink", return_value=True)
    def test_misconfigured_redirection_windows_bind(
        self, mock_is_valid_windows_symlink: MagicMock
    ) -> None:
        temp_dir = self.make_temporary_directory()
        target = Path(temp_dir) / "target"
        instance = FakeEdenInstance(temp_dir)
        checkout = instance.create_test_mount("mount_dir")
        redir = Redirection(
            repo_path=Path("foo"),
            redir_type=RedirectionType.BIND,
            target=None,
            source="mount",
            state=RedirectionState.SYMLINK_MISSING,
        )

        with (
            patch("eden.fs.cli.redirect.sys.platform", "win32"),
            patch("eden.fs.cli.redirect.make_scratch_dir", return_value=target),
        ):
            self.assertTrue(check_redirection(redir, checkout))

        mock_is_valid_windows_symlink.assert_called_once_with(
            target, checkout.path / "foo"
        )

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
