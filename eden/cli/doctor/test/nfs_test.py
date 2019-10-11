#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from types import SimpleNamespace
from typing import Optional
from unittest.mock import patch

import eden.cli.doctor as doctor
from eden.cli import filesystem
from eden.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.cli.test.lib.output import TestOutput


class NfsTest(DoctorTestBase):
    maxDiff: Optional[int] = None

    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_mounted(self, mock_is_nfs_mounted):
        mock_is_nfs_mounted.return_value = True
        instance = FakeEdenInstance(self.make_temporary_directory())
        checkout = instance.create_test_mount("mount_dir")

        dry_run = True
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=self.make_process_finder(),
            out=out,
        )
        expected = f"""\
<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.\
  Make sure that ~/local is a symlink pointing to local disk and then restart Eden.

Checking {checkout.path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {checkout.path}/.hg/sharedpath is at \
{instance.default_backing_repo}/.hg which is on a NFS filesystem. \
Accessing files and directories in this repository will be slow.
<yellow>Discovered 2 problems during --dry-run<reset>
"""
        self.assertEqual(expected, out.getvalue())
        self.assertEqual(1, exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_no_nfs(self, mock_is_nfs_mounted, mock_path_read_text):
        mock_is_nfs_mounted.side_effect = [False, False]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
Checking {v.client_path}
<green>No issues detected.<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(0, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_client_path(self, mock_is_nfs_mounted, mock_path_read_text):
        mock_is_nfs_mounted.side_effect = [True, False]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {v.instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.\
  Make sure that ~/local is a symlink pointing to local disk and then restart Eden.

Checking {v.client_path}
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_shared_path(self, mock_is_nfs_mounted, mock_path_read_text):
        mock_is_nfs_mounted.side_effect = [False, True]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
Checking {v.client_path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {v.client_path}/.hg/sharedpath \
is at {v.shared_path} which is on a NFS filesystem. \
Accessing files and directories in this repository will be slow.
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_client_path_and_shared_path(
        self, mock_is_nfs_mounted, mock_path_read_text
    ):
        mock_is_nfs_mounted.side_effect = [True, True]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""\
<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {v.instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.\
  Make sure that ~/local is a symlink pointing to local disk and then restart Eden.

Checking {v.client_path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {v.client_path}/.hg/sharedpath is at\
 {v.shared_path} which is on a NFS filesystem. Accessing files and directories\
 in this repository will be slow.
<yellow>Discovered 2 problems during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    def run_varying_nfs(self, mock_path_read_text):
        instance = FakeEdenInstance(self.make_temporary_directory())
        v = SimpleNamespace(
            mount_dir="mount_dir", shared_path="shared_path", instance=instance
        )
        mock_path_read_text.return_value = v.shared_path
        v.client_path = str(instance.create_test_mount(v.mount_dir).path)

        dry_run = True
        out = TestOutput()
        v.exit_code = doctor.cure_what_ails_you(
            instance,
            dry_run,
            instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=self.make_process_finder(),
            out=out,
        )
        v.stdout = out.getvalue()
        return v
