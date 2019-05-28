#!/usr/bin/env python3
#
# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import itertools
from typing import Tuple

import eden.cli.doctor as doctor
from eden.cli import process_finder
from eden.cli.doctor import check_rogue_edenfs
from eden.cli.doctor.test.lib.testcase import DoctorTestBase


class MultipleEdenfsRunningTest(DoctorTestBase):
    maxDiff = None

    def run_check(
        self, process_finder: process_finder.ProcessFinder, dry_run: bool
    ) -> Tuple[doctor.ProblemFixer, str]:
        fixer, out = self.create_fixer(dry_run)
        check_rogue_edenfs.check_many_edenfs_are_running(fixer, process_finder)
        return fixer, out.getvalue()

    def test_when_there_are_rogue_pids(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(123, "/home/someuser/.eden", set_lockfile=False)
        process_finder.add_edenfs(124, "/home/someuser/.eden", set_lockfile=False)
        process_finder.add_edenfs(125, "/home/someuser/.eden", set_lockfile=True)
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 123 124

""",
        )

    def test_when_no_rogue_edenfs_process_running(self) -> None:
        process_finder = self.make_process_finder()
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_error_reading_proc(self) -> None:
        process_finder = self.make_process_finder()
        # Add some rogue edenfs processes, but then blow away parts of their proc
        # directories so that the process finder won't be able to read all of their
        # information.
        #
        # This sort of simulates what would happen if the processes exited while the
        # code was trying to read their information.
        process_finder.add_edenfs(123, "/home/someuser/.eden", set_lockfile=False)
        process_finder.add_edenfs(124, "/home/someuser/.eden", set_lockfile=False)
        process_finder.add_edenfs(125, "/home/someuser/.eden", set_lockfile=True)

        (process_finder.proc_path / "124" / "comm").unlink()
        (process_finder.proc_path / "125" / "cmdline").unlink()

        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_no_pids_at_all(self) -> None:
        process_finder = self.make_process_finder()
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_pids_but_edenDir_not_in_cmdline(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_process(1614248, ["edenfs"])
        process_finder.add_process(1639164, ["edenfs"])
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_many_edenfs_procs_run_for_same_config(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(
            475203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=True
        )
        process_finder.add_edenfs(
            475204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.add_edenfs(
            475205, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475204 475205

""",
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_other_processes_with_similar_names_running(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(475203, "/home/user/.eden")
        process_finder.add_process(
            475204, ["/foobar/fooedenfs", "--edenDir", "/home/user/.eden", "--edenfs"]
        )
        process_finder.add_process(
            475205, ["/foobar/edenfsbar", "--edenDir", "/home/user/.eden", "--edenfs"]
        )
        process_finder.add_process(
            475206, ["/foobar/edenfs", "--edenDir", "/home/user/.eden", "--edenfs"]
        )

        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual(
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475206

""",
            out,
        )
        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)

    def test_when_only_valid_edenfs_process_running(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(475203, "/home/someuser/.eden")
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_os_found_pids_but_edenDir_value_not_in_cmdline(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_process(1614248, ["edenfs", "--edenDir"])
        process_finder.add_process(1639164, ["edenfs"])
        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual("", out)
        self.assert_results(fixer, num_problems=0)

    def test_when_differently_configured_edenfs_processes_running_with_rogue_pids(
        self
    ) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(475203, "/tmp/config1/.eden")
        process_finder.add_edenfs(475204, "/tmp/config1/.eden", set_lockfile=False)
        process_finder.add_edenfs(475205, "/tmp/config1/.eden", set_lockfile=False)
        process_finder.add_edenfs(575203, "/tmp/config2/.eden")
        process_finder.add_edenfs(575204, "/tmp/config2/.eden", set_lockfile=False)
        process_finder.add_edenfs(575205, "/tmp/config2/.eden", set_lockfile=False)
        fixer, out = self.run_check(process_finder, dry_run=False)

        self.assert_results(fixer, num_problems=1, num_manual_fixes=1)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 475204 475205 575204 575205

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
        process_finder = self.make_process_finder()
        # In config1/ replace the lock file contents with a different process ID
        process_finder.add_edenfs(123203, "/tmp/config1/.eden", set_lockfile=False)
        process_finder.set_file_contents("/tmp/config1/.eden/lock", b"9765\n")
        # In config2/ do not write a lock file at all
        process_finder.add_edenfs(123456, "/tmp/config2/.eden", set_lockfile=False)
        # In config3/ report two separate edenfs processes, with one legitimate rogue
        # process
        process_finder.add_edenfs(123900, "/tmp/config3/.eden")
        process_finder.add_edenfs(123901, "/tmp/config3/.eden", set_lockfile=False)

        fixer, out = self.run_check(process_finder, dry_run=False)
        self.assertEqual(
            out,
            f"""\
<yellow>- Found problem:<reset>
Many edenfs processes are running. Please keep only one for each config directory.
kill -9 123901

""",
        )

    def test_when_lock_file_op_has_io_exception(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(
            475203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.add_edenfs(
            475204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(process_finder, dry_run=False)
            self.assertEqual("", out)
            self.assert_results(fixer, num_problems=0)
            logs = "\n".join(logs_assertion.output)
            self.assertIn(
                "WARNING:eden.cli.process_finder:Lock file cannot be read for",
                logs,
                "when lock file can't be opened",
            )

    def test_when_lock_file_data_is_garbage(self) -> None:
        process_finder = self.make_process_finder()
        process_finder.add_edenfs(
            475203, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.add_edenfs(
            475204, "/tmp/eden_test.68yxptnx/.eden", set_lockfile=False
        )
        process_finder.set_file_contents("/tmp/eden_test.68yxptnx/.eden/lock", b"asdf")
        with self.assertLogs() as logs_assertion:
            fixer, out = self.run_check(process_finder, dry_run=False)
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
                    rogue_pids_list=rogue_pids_list
                )
                message = problem.get_manual_remediation_message()
                assert message is not None
                self.assertIn(expected_pid_order, message)
