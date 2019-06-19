#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import shutil
import typing

import eden.cli.doctor as doctor
from eden.cli import filesystem
from eden.cli.config import EdenInstance
from eden.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.cli.test.lib.output import TestOutput


class CorruptHgTest(DoctorTestBase):
    maxDiff = None

    def setUp(self) -> None:
        self.instance = FakeEdenInstance(self.make_temporary_directory())
        self.checkout = self.instance.create_test_mount("test_mount", scm_type="hg")
        self.backing_repo = typing.cast(
            FakeEdenInstance, self.checkout.instance
        ).default_backing_repo

    def test_unreadable_hg_shared_path_is_a_problem(self) -> None:
        sharedpath_path = self.checkout.path / ".hg" / "sharedpath"
        sharedpath_path.unlink()
        sharedpath_path.symlink_to(sharedpath_path.name)

        out = self.cure_what_ails_you(dry_run=True)
        self.assertIn(
            "Failed to read .hg/sharedpath: "
            "[Errno 40] Too many levels of symbolic links",
            out.getvalue(),
        )

    def test_truncated_hg_dirstate_is_a_problem(self) -> None:
        dirstate_path = self.checkout.path / ".hg" / "dirstate"
        os.truncate(dirstate_path, dirstate_path.stat().st_size - 1)

        out = self.cure_what_ails_you(dry_run=True)
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {self.checkout.path}/.hg:
  error parsing .hg/dirstate: Reached EOF while reading checksum \
hash in {self.checkout.path}/.hg/dirstate.

Would repair hg directory contents for {self.checkout.path}

<yellow>Discovered 1 problem during --dry-run<reset>
""",
            out.getvalue(),
        )

    def test_missing_sharedpath_and_requires(self) -> None:
        sharedpath_path = self.checkout.path / ".hg" / "sharedpath"
        sharedpath_path.unlink()
        requires_path = self.checkout.path / ".hg" / "requires"
        requires_path.unlink()

        out = self.cure_what_ails_you(dry_run=False)
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {self.checkout.path}/.hg:
  error reading .hg/requires: [Errno 2] No such file or directory: \
{str(requires_path)!r}
  error reading .hg/sharedpath: [Errno 2] No such file or directory: \
{str(sharedpath_path)!r}
Repairing hg directory contents for {self.checkout.path}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
            out.getvalue(),
        )
        self.assertIn("eden\n", requires_path.read_text())
        self.assertEqual(sharedpath_path.read_text(), str(self.backing_repo / ".hg"))

    def test_missing_hg_dir(self) -> None:
        hg_dir = self.checkout.path / ".hg"
        shutil.rmtree(hg_dir)

        out = self.cure_what_ails_you(dry_run=False)
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Missing hg directory: {self.checkout.path}/.hg
Repairing hg directory contents for {self.checkout.path}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
            out.getvalue(),
        )
        self._verify_hg_dir()

    def test_empty_hg_dir(self) -> None:
        hg_dir = self.checkout.path / ".hg"
        shutil.rmtree(hg_dir)
        hg_dir.mkdir()

        out = self.cure_what_ails_you(dry_run=False)
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
No contents present in hg directory: {self.checkout.path}/.hg
Repairing hg directory contents for {self.checkout.path}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
            out.getvalue(),
        )
        self._verify_hg_dir()

    def _verify_hg_dir(self) -> None:
        hg_dir = self.checkout.path / ".hg"
        self.assertTrue((hg_dir / "dirstate").is_file())
        self.assertTrue((hg_dir / "hgrc").is_file())
        self.assertTrue((hg_dir / "requires").is_file())
        self.assertTrue((hg_dir / "sharedpath").is_file())
        self.assertTrue((hg_dir / "shared").is_file())
        self.assertTrue((hg_dir / "bookmarks").is_file())
        self.assertTrue((hg_dir / "branch").is_file())

        self.assert_dirstate_p0(self.checkout, FakeEdenInstance.default_commit_hash)
        self.assertIn("[extensions]\neden =\n", (hg_dir / "hgrc").read_text())
        self.assertIn("eden\n", (hg_dir / "requires").read_text())
        self.assertEqual(
            (hg_dir / "sharedpath").read_text(), str(self.backing_repo / ".hg")
        )
        self.assertEqual((hg_dir / "shared").read_text(), "bookmarks\n")
        self.assertEqual((hg_dir / "bookmarks").read_text(), "")
        self.assertEqual((hg_dir / "branch").read_text(), "default\n")

    def cure_what_ails_you(self, dry_run: bool) -> TestOutput:
        out = TestOutput()
        doctor.cure_what_ails_you(
            typing.cast(EdenInstance, self.instance),
            dry_run,
            self.instance.mount_table,
            fs_util=filesystem.LinuxFsUtil(),
            process_finder=self.make_process_finder(),
            out=out,
        )
        return out
