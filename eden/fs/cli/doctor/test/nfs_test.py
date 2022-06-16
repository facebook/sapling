#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import typing
from unittest.mock import patch

import eden.fs.cli.doctor as doctor
from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.fs.cli.doctor.test.lib.fake_fs_util import FakeFsUtil
from eden.fs.cli.doctor.test.lib.fake_kerberos_checker import FakeKerberosChecker
from eden.fs.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.fs.cli.test.lib.output import TestOutput


class NfsDoctorResult(typing.NamedTuple):
    mount_dir: str
    shared_path: str
    client_path: str
    instance: FakeEdenInstance
    exit_code: int
    stdout: str


class NfsTest(DoctorTestBase):
    maxDiff: typing.Optional[int] = None

    @patch("eden.fs.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_mounted(self, mock_is_nfs_mounted) -> None:
        mock_is_nfs_mounted.return_value = True
        instance = FakeEdenInstance(self.make_temporary_directory())
        checkout = instance.create_test_mount("mount_dir")

        dry_run = True
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run,
            instance.mount_table,
            fs_util=FakeFsUtil(),
            proc_utils=self.make_proc_utils(),
            kerberos_checker=FakeKerberosChecker(),
            out=out,
        )
        expected = f"""<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.  Make sure that ~/local is a symlink pointing to local disk and then run `eden restart`.

Checking {checkout.path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {checkout.path}/.hg/sharedpath is at {instance.default_backing_repo}/.hg which is on a NFS filesystem. Accessing files and directories in this repository will be slow.
<yellow>Discovered 2 problems during --dry-run<reset>
"""
        self.assertEqual(expected, out.getvalue())
        self.assertEqual(1, exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.fs.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_no_nfs(self, mock_is_nfs_mounted, mock_path_read_text) -> None:
        mock_is_nfs_mounted.side_effect = [False, False]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""Checking {v.client_path}
<green>No issues detected.<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(0, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.fs.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_client_path(self, mock_is_nfs_mounted, mock_path_read_text) -> None:
        mock_is_nfs_mounted.side_effect = [True, False]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {v.instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.  Make sure that ~/local is a symlink pointing to local disk and then run `eden restart`.

Checking {v.client_path}
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.fs.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_shared_path(self, mock_is_nfs_mounted, mock_path_read_text) -> None:
        mock_is_nfs_mounted.side_effect = [False, True]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""Checking {v.client_path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {v.client_path}/.hg/sharedpath is at {v.shared_path} which is on a NFS filesystem. Accessing files and directories in this repository will be slow.
<yellow>Discovered 1 problem during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    @patch("pathlib.Path.read_text")
    @patch("eden.fs.cli.doctor.check_filesystems.is_nfs_mounted")
    def test_nfs_on_client_path_and_shared_path(
        self, mock_is_nfs_mounted, mock_path_read_text
    ) -> None:
        mock_is_nfs_mounted.side_effect = [True, True]
        v = self.run_varying_nfs(mock_path_read_text)
        out = f"""<yellow>- Found problem:<reset>
Eden's state directory is on an NFS file system: {v.instance.state_dir}
  This will likely cause performance problems and/or other errors.
The most common cause for this is if your ~/local symlink does not point to local disk.  Make sure that ~/local is a symlink pointing to local disk and then run `eden restart`.

Checking {v.client_path}
<yellow>- Found problem:<reset>
The Mercurial data directory for {v.client_path}/.hg/sharedpath is at {v.shared_path} which is on a NFS filesystem. Accessing files and directories in this repository will be slow.
<yellow>Discovered 2 problems during --dry-run<reset>
"""
        self.assertEqual(mock_is_nfs_mounted.call_count, 2)
        self.assertEqual(out, v.stdout)
        self.assertEqual(1, v.exit_code)

    def run_varying_nfs(self, mock_path_read_text) -> NfsDoctorResult:
        instance = FakeEdenInstance(self.make_temporary_directory())
        shared_path = "shared_path"
        mount_dir = "mount_dir"
        mock_path_read_text.return_value = shared_path
        client_path = str(instance.create_test_mount(mount_dir).path)

        dry_run = True
        out = TestOutput()
        exit_code = doctor.cure_what_ails_you(
            typing.cast(EdenInstance, instance),
            dry_run,
            instance.mount_table,
            fs_util=FakeFsUtil(),
            proc_utils=self.make_proc_utils(),
            kerberos_checker=FakeKerberosChecker(),
            out=out,
        )
        return NfsDoctorResult(
            mount_dir, shared_path, client_path, instance, exit_code, out.getvalue()
        )
