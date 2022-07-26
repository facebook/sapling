#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import itertools
import sys
import unittest
from typing import Optional, Tuple

import eden.fs.cli.doctor as doctor
from eden.fs.cli import proc_utils
from eden.fs.cli.doctor import check_rogue_edenfs
from eden.fs.cli.doctor.test.lib.testcase import DoctorTestBase
from eden.fs.cli.test.lib.fake_proc_utils import FakeProcUtils


TEST_UID = 99


class MultipleEdenfsRunningTest(DoctorTestBase):
    maxDiff: Optional[int] = None

    def run_check(
        self, proc_utils: proc_utils.ProcUtils, dry_run: bool
    ) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        check_rogue_edenfs.check_many_edenfs_are_running(
            fixer, proc_utils, uid=TEST_UID
        )
        return fixer, out.getvalue()

    def make_proc_utils(self) -> FakeProcUtils:
        return FakeProcUtils(self.make_temporary_directory(), default_uid=TEST_UID)

    def test_when_there_are_rogue_pids(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(123, "/home/someuser/.eden", set_lockfile=False)
        proc_utils.add_edenfs(456, "/home/someuser/.eden", set_lockfile=False)
        proc_utils.add_edenfs(789, "/home/someuser/.eden", set_lockfile=True)
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 123 456

""",
        )

    def test_when_no_rogue_edenfs_process_running(self) -> None:
        proc_utils = self.make_proc_utils()
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_error_reading_proc(self) -> None:
        proc_utils = self.make_proc_utils()
        # Add some rogue edenfs processes, but then blow away parts of their proc
        # directories so that the process finder won't be able to read all of their
        # information.
        #
        # This sort of simulates what would happen if the processes exited while the
        # code was trying to read their information.
        proc_utils.add_edenfs(123, "/home/someuser/.eden", set_lockfile=False)
        proc_utils.add_edenfs(456, "/home/someuser/.eden", set_lockfile=False)
        proc_utils.add_edenfs(789, "/home/someuser/.eden", set_lockfile=True)

        (proc_utils.proc_path / "456" / "comm").unlink()
        (proc_utils.proc_path / "789" / "cmdline").unlink()

        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_no_pids_at_all(self) -> None:
        proc_utils = self.make_proc_utils()
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_pids_but_edenDir_not_in_cmdline(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_process(1614248, ["edenfs"])
        proc_utils.add_process(1639164, ["edenfs"])
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_many_edenfs_procs_run_for_same_config(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(
            475203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=True
        )
        proc_utils.add_edenfs(
            575204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        proc_utils.add_edenfs(
            675205, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 575204 675205

""",
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_other_processes_with_similar_names_running(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(475203, "/home/user/.eden")
        proc_utils.add_process(
            575204, ["/foobar/fooedenfs", "--edenDir", "/home/user/.eden", "--edenfs"]
        )
        proc_utils.add_process(
            675205, ["/foobar/edenfsbar", "--edenDir", "/home/user/.eden", "--edenfs"]
        )
        proc_utils.add_process(
            775206, ["/foobar/edenfs", "--edenDir", "/home/user/.eden", "--edenfs"]
        )
        proc_utils.add_edenfs(775310, "/home/user/.eden", set_lockfile=False)

        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 775310

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_only_valid_edenfs_process_running(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(475203, "/home/someuser/.eden")
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_pids_but_edenDir_value_not_in_cmdline(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_process(1614248, ["edenfs", "--edenDir"])
        proc_utils.add_process(1639164, ["edenfs"])
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_state_directory_from_lock_fd(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(
            1234,
            "/home/someuser/.eden",
            cmdline=["edenfs", "--edenfs"],
            set_lockfile=True,
        )
        proc_utils.add_edenfs(
            5678,
            "/home/someuser/.eden",
            cmdline=["edenfs", "--edenfs"],
            set_lockfile=False,
        )
        proc_utils.add_edenfs(
            9876,
            "/home/someuser/.eden",
            cmdline=["edenfs", "--edenfs"],
            set_lockfile=False,
        )
        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 5678 9876

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_differently_configured_edenfs_processes_running_with_rogue_pids(
        self,
    ) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(475203, "/tmp/config1/.eden")
        proc_utils.add_edenfs(475304, "/tmp/config1/.eden", set_lockfile=False)
        proc_utils.add_edenfs(475405, "/tmp/config1/.eden", set_lockfile=False)
        proc_utils.add_edenfs(575203, "/tmp/config2/.eden")
        proc_utils.add_edenfs(575304, "/tmp/config2/.eden", set_lockfile=False)
        proc_utils.add_edenfs(575405, "/tmp/config2/.eden", set_lockfile=False)
        fixer, out = self.run_check(proc_utils, dry_run=False)

        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475304 475405 575304 575405

""",
        )

    def test_single_edenfs_process_per_dir_okay(self) -> None:
        # The rogue process finder should not complain about edenfs processes
        # when there is just a single edenfs process running per directory, even if the
        # pid file does not appear to currently contain that pid.
        #
        # The pid file check is inherently racy.  `eden doctor` may not read the correct
        # pid if edenfs was in the middle of (re)starting.  Therefore we intentionally
        # only report rogue processes when we can actually confirm there is more than
        # one edenfs process running for a given directory.
        proc_utils = self.make_proc_utils()
        # In config1/ replace the lock file contents with a different process ID
        proc_utils.add_edenfs(123203, "/tmp/config1/.eden", set_lockfile=False)
        proc_utils.set_file_contents("/tmp/config1/.eden/lock", b"9765\n")
        # In config2/ do not write a lock file at all
        proc_utils.add_edenfs(123456, "/tmp/config2/.eden", set_lockfile=False)
        # In config3/ report two separate edenfs processes, with one legitimate rogue
        # process
        proc_utils.add_edenfs(123900, "/tmp/config3/.eden")
        proc_utils.add_edenfs(123991, "/tmp/config3/.eden", set_lockfile=False)

        fixer, out = self.run_check(proc_utils, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 123991

""",
        )

    def test_when_lock_file_op_has_io_exception(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(
            475203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        proc_utils.add_edenfs(
            475304, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(proc_utils, dry_run=False)
            self.assertEqual("", out)
            self.assert_results(fixer, num_problems=0)
            logs = "\n".join(logs_assertion.output)
            self.assertRegex(
                logs,
                r"WARNING:.*:Lock file cannot be read for",
                "when lock file can't be opened",
            )

    def test_when_lock_file_data_is_garbage(self) -> None:
        proc_utils = self.make_proc_utils()
        proc_utils.add_edenfs(
            475203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        proc_utils.add_edenfs(
            475304, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        proc_utils.set_file_contents("/tmp/eden_test.68yxptnx/.eden/lock", b"asdf")
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(proc_utils, dry_run=False)
            self.assertEqual("", out)
            self.assert_results(fixer, num_problems=0)
        self.assertIn(
            "lock file contains data that cannot be parsed",
            "\n".join(logs_assertion.output),
        )

    def test_process_ids_are_ordered_consistently(self) -> None:
        pids = [1, 200, 30]
        expected_pid_order = "1 30 200"
        for rogue_pids_list in itertools.permutations(pids):
            with self.subTest(rogue_pids_list=rogue_pids_list):
                problem = check_rogue_edenfs.ManyEdenFsRunning(
                    rogue_pids_list=list(rogue_pids_list)
                )
                message = problem.get_manual_remediation_message()
                assert message is not None
                self.assertIn(expected_pid_order, message)
