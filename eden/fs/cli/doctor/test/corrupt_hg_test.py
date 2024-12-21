#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict


import os
import shutil
import subprocess
import sys
import typing
from pathlib import Path
from unittest.mock import patch

import eden.fs.cli.doctor as doctor
from eden.fs.cli.config import EdenInstance
from eden.fs.cli.doctor.test.lib.fake_eden_instance import FakeEdenInstance
from eden.fs.cli.doctor.test.lib.fake_fs_util import FakeFsUtil
from eden.fs.cli.doctor.test.lib.fake_network_checker import FakeNetworkChecker
from eden.fs.cli.doctor.test.lib.fake_vscode_extensions_checker import (
    getFakeVSCodeExtensionsChecker,
)
from eden.fs.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.fs.cli.test.lib.output import TestOutput


class CorruptHgTest(DoctorTestBase):
    # pyre-fixme[4]: Attribute must be annotated.
    maxDiff = None

    def format_win_path_for_regex(self, path: str) -> str:
        # Formats the path to be compatible with regex matching on windows
        if sys.platform == "win32":
            return path.replace("\\", "\\\\")
        return path

    def setUp(self) -> None:
        self.instance = FakeEdenInstance(self.make_temporary_directory())
        self.checkout = self.instance.create_test_mount("test_mount", scm_type="hg")
        self.backing_repo = typing.cast(
            FakeEdenInstance, self.checkout.instance
        ).default_backing_repo

    def test_truncated_hg_dirstate_is_a_problem(
        self,
    ) -> None:
        dirstate_path = self.checkout.path / ".hg" / "dirstate"
        os.truncate(dirstate_path, dirstate_path.stat().st_size - 1)

        out = self.cure_what_ails_you(dry_run=True)
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {self.checkout.hg_dot_path}:
  error parsing .hg{os.sep}dirstate: Reached EOF while reading checksum \
hash in {self.checkout.path}{os.sep}.hg{os.sep}dirstate.

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
Found inconsistent/missing data in {self.checkout.hg_dot_path}:
  error reading .hg{os.sep}requires: [Errno 2] No such file or directory: \
{str(requires_path)!r}
  error reading .hg{os.sep}sharedpath: [Errno 2] No such file or directory: \
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
Missing hg directory: {self.checkout.hg_dot_path}
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
No contents present in hg directory: {self.checkout.hg_dot_path}
Repairing hg directory contents for {self.checkout.path}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
            out.getvalue(),
        )
        self._verify_hg_dir()

    def test_interrupted_transaction(self) -> None:
        store = self.backing_repo / ".hg" / "store"
        store.mkdir()
        journal = store / "journal"
        journal.write_text("")
        with patch(
            "eden.fs.cli.doctor.check_hg.AbandonedTransactionChecker.repair",
            wraps=lambda: os.unlink(journal),
        ) as mock_run_hg:
            out = self.cure_what_ails_you(dry_run=False)
            mock_run_hg.assert_called_once()
        self.assertEqual(
            f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {self.checkout.hg_dot_path}:
  Found a journal file in backing repo, might have an interrupted transaction
Repairing hg directory contents for {self.checkout.path}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
            out.getvalue(),
        )

    def test_interrupted_transaction_race_condition(self) -> None:
        store = self.backing_repo / ".hg" / "store"
        store.mkdir()
        journal: Path = store / "journal"
        journal.write_text("")
        outputs: typing.List[str] = [
            "Found a journal file in backing repo, might have an interrupted transaction"
        ]

        def patched_func() -> typing.List[str]:
            if outputs:
                os.unlink(journal)
                return [outputs.pop()]
            return []

        with patch(
            "eden.fs.cli.doctor.test.lib.fake_hg_repo.FakeHgRepo._run_hg"
        ) as mock_run_hg:
            mock_run_hg.side_effect = subprocess.CalledProcessError(
                1, "hg", b"", stderr=b"no interrupted transaction available\n"
            )
            with patch(
                "eden.fs.cli.doctor.check_hg.AbandonedTransactionChecker.check_for_error",
                wraps=patched_func,
            ) as mock_check_error:
                out = self.cure_what_ails_you(dry_run=False)
                mock_check_error.assert_called_with()
            self.assertEqual(
                f"""\
Checking {self.checkout.path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {self.checkout.hg_dot_path}:
  Found a journal file in backing repo, might have an interrupted transaction
Repairing hg directory contents for {self.checkout.path}...<green>fixed<reset>

<yellow>Successfully fixed 1 problem.<reset>
""",
                out.getvalue(),
            )

    def test_interrupted_transaction_repair_error(self) -> None:
        store = self.backing_repo / ".hg" / "store"
        store.mkdir()
        journal: Path = store / "journal"
        journal.write_text("")
        outputs: typing.List[str] = [
            "Found a journal file in backing repo, might have an interrupted transaction"
        ]

        def patched_func() -> typing.List[str]:
            if outputs:
                os.unlink(journal)
                return [outputs.pop()]
            return []

        with patch(
            "eden.fs.cli.doctor.test.lib.fake_hg_repo.FakeHgRepo._run_hg"
        ) as mock_run_hg:
            mock_run_hg.side_effect = subprocess.CalledProcessError(
                1, "hg", b"", stderr=b"repair error\n"
            )
            with patch(
                "eden.fs.cli.doctor.check_hg.AbandonedTransactionChecker.check_for_error",
                wraps=patched_func,
            ) as mock_check_error:
                out = self.cure_what_ails_you(dry_run=False)
                mock_check_error.assert_called_with()
            checkout_path = self.format_win_path_for_regex(str(self.checkout.path))
            hg_dot_path = self.format_win_path_for_regex(str(self.checkout.hg_dot_path))
            self.assertRegex(
                out.getvalue(),
                rf"""Checking {checkout_path}
<yellow>- Found problem:<reset>
Found inconsistent/missing data in {hg_dot_path}:
  Found a journal file in backing repo, might have an interrupted transaction
Repairing hg directory contents for {checkout_path}...<red>error<reset>
Failed to fix or verify fix for problem HgDirectoryError: CalledProcessError: Command 'hg' returned non-zero exit status 1.
│ Traceback .*
(.*\n){{15}}.*
│ subprocess.CalledProcessError: Command 'hg' returned non-zero exit status 1.
stdout:

stderr:
repair error


<red>Failed to fix 1 problem.<reset>
.*""",
                msg=f"formatted output:\n{out.getvalue()}",
            )

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
            mount_table=self.instance.mount_table,
            fs_util=FakeFsUtil(),
            proc_utils=self.make_proc_utils(),
            vscode_extensions_checker=getFakeVSCodeExtensionsChecker(),
            network_checker=FakeNetworkChecker(),
            out=out,
        )
        return out
