#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import threading

from eden.integration.lib import eden_server_inspector, hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class RebaseTest(EdenHgTestCase):
    _base_commit: str
    _c11: str
    _c12: str
    _c13: str
    _c14: str
    _c15: str
    _c21: str
    _c22: str
    _c23: str
    _c24: str
    _c25: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.mkdir("numbers")
        repo.write_file("numbers/README", "this will have two directories")
        self._base_commit = repo.commit("commit")

        repo.mkdir("numbers/1")
        repo.write_file("numbers/1/11", "11\n")
        self._c11 = repo.commit("c11")
        repo.write_file("numbers/1/12", "12\n")
        self._c12 = repo.commit("c12")
        repo.write_file("numbers/1/13", "13\n")
        self._c13 = repo.commit("c13")
        repo.write_file("numbers/1/14", "14\n")
        self._c14 = repo.commit("c14")
        repo.write_file("numbers/1/15", "15\n")
        self._c15 = repo.commit("c15")

        repo.update(self._base_commit)
        repo.mkdir("numbers/2")
        repo.write_file("numbers/2/21", "21\n")
        self._c21 = repo.commit("c21")
        repo.write_file("numbers/2/22", "22\n")
        self._c22 = repo.commit("c22")
        repo.write_file("numbers/2/23", "23\n")
        self._c23 = repo.commit("c23")
        repo.write_file("numbers/2/24", "24\n")
        self._c24 = repo.commit("c24")
        repo.write_file("numbers/2/25", "25\n")
        self._c25 = repo.commit("c25")

        repo.update(self._base_commit)

    def test_rebase_commit_with_independent_folder(self) -> None:
        stdout = self.hg("--debug", "rebase", "-s", self._c11, "-d", self._c25)
        self.assertIn(f'rebasing {self._c11[:12]} "c11"\n', stdout)
        self.assertIn(f'rebasing {self._c12[:12]} "c12"\n', stdout)
        self.assertIn(f'rebasing {self._c13[:12]} "c13"\n', stdout)
        self.assertIn(f'rebasing {self._c14[:12]} "c14"\n', stdout)
        self.assertIn(f'rebasing {self._c15[:12]} "c15"\n', stdout)
        # Note that these are empirical values, not desired values.
        # We need to figure out why this hits the slow path and fix it!
        self.assert_update_logic(stdout, num_fast_path=2, num_slow_path=5)

        # Get the hash of the new head created as a result of the rebase.
        new_head = self.repo.log(revset=f"successors({self._c15})-{self._c15}")[0]

        # Record the pre-update inode count.
        inspector = eden_server_inspector.EdenServerInspector(self.eden, self.repo.path)
        inspector.unload_inode_for_path("numbers")
        pre_update_count = inspector.get_inode_count("numbers")
        print(f"loaded inode count before `hg update`: {pre_update_count}")

        # Verify that updating to the new head that was created as a result of
        # the rebase leaves Hg in the correct state.
        self.assertEqual(
            1,
            len(self.repo.log()),
            msg=("At the base commit, `hg log` should have only one entry."),
        )
        stdout = self.hg("--debug", "update", new_head)
        self.assert_update_logic(stdout, num_fast_path=1)
        self.assertEqual(
            11,
            len(self.repo.log()),
            msg=("The new head should include all the commits."),
        )

        # Verify the post-update inode count.
        post_update_count = inspector.get_inode_count("numbers")
        print(f"loaded inode count after `hg update`: {post_update_count}")
        self.assertGreaterEqual(
            post_update_count,
            pre_update_count,
            msg=("The inode count should not decrease due to `hg update`."),
        )
        num_new_inodes = post_update_count - pre_update_count
        self.assertLessEqual(
            num_new_inodes,
            2,
            msg=(
                "There should be no more than 2 new inodes as a result of the "
                "update. At the time this test was created, num_new_inodes is 0, "
                "but if we included unloaded inodes, there would be 2: one for "
                "numbers/1 and one for numbers/2."
            ),
        )

    def test_rebasing_a_commit_that_removes_a_file(self) -> None:
        # Rebase a commit that removes the numbers/README file.
        self.hg("rm", "numbers/README")
        removal_commit = self.repo.commit("removing README")
        self.hg("rebase", "-s", removal_commit, "-d", self._c15)

        # Verify we end up in the expected state.
        self.assert_status_empty()
        self.assertFalse(os.path.exists(self.get_path("numbers/README")))
        self.assertEqual(7, len(self.repo.log()))

    def test_rebase_stack_with_conflicts(self) -> None:
        """Create a stack of commits that has conflicts with the stack onto
        which we rebase and verify that if we merge the expected conflicts along
        the way, then we end up in the expected state."""
        self.mkdir("numbers/1")

        self.write_file("numbers/1/11", "new 11\n")
        self.repo.add_file("numbers/1/11")
        self.write_file("numbers/1/12", "new 12\n")
        self.repo.add_file("numbers/1/12")
        commit = self.repo.commit("Introduce 1/11 and 1/12.")

        self.write_file("numbers/1/12", "change 12 again\n")
        self.write_file("numbers/1/13", "new 13\n")
        self.repo.add_file("numbers/1/13")
        self.write_file("numbers/1/14", "new 14\n")
        self.repo.add_file("numbers/1/14")
        self.repo.commit("Introduce 1/13 and 1/14.")

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("rebase", "-s", commit, "-d", self._c15)
        self.assertIn(
            b"conflicts while merging numbers/1/11! "
            b"(edit, then use 'hg resolve --mark')",
            context.exception.stderr,
        )
        self.assert_unresolved(unresolved=["numbers/1/11", "numbers/1/12"])
        self.assert_status({"numbers/1/11": "M", "numbers/1/12": "M"}, op="rebase")
        self.assert_file_regex(
            "numbers/1/11",
            """\
            <<<<<<< dest: .*
            11
            =======
            new 11
            >>>>>>> source: .*
            """,
        )
        self.assert_file_regex(
            "numbers/1/12",
            """\
            <<<<<<< dest: .*
            12
            =======
            new 12
            >>>>>>> source: .*
            """,
        )

        self.write_file("numbers/1/11", "11 merged.\n")
        self.write_file("numbers/1/12", "12 merged.\n")
        self.hg("resolve", "--mark", "numbers/1/11", "numbers/1/12")

        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("rebase", "--continue")
        self.assertIn(
            b"conflicts while merging numbers/1/12! "
            b"(edit, then use 'hg resolve --mark')",
            context.exception.stderr,
        )
        self.assert_unresolved(
            unresolved=["numbers/1/12", "numbers/1/13", "numbers/1/14"]
        )
        self.assert_status(
            {"numbers/1/12": "M", "numbers/1/13": "M", "numbers/1/14": "M"}, op="rebase"
        )
        self.assert_file_regex(
            "numbers/1/12",
            """\
            <<<<<<< dest: .*
            12 merged.
            =======
            change 12 again
            >>>>>>> source: .*
            """,
        )
        self.assert_file_regex(
            "numbers/1/13",
            """\
            <<<<<<< dest: .*
            13
            =======
            new 13
            >>>>>>> source: .*
            """,
        )
        self.assert_file_regex(
            "numbers/1/14",
            """\
            <<<<<<< dest: .*
            14
            =======
            new 14
            >>>>>>> source: .*
            """,
        )

        self.write_file("numbers/1/12", "merged.\n")
        self.write_file("numbers/1/13", "merged.\n")
        self.write_file("numbers/1/14", "merged.\n")
        self.hg("resolve", "--mark", "numbers/1/12", "numbers/1/13", "numbers/1/14")
        self.hg("rebase", "--continue")
        commits = self.repo.log()
        self.assertEqual(8, len(commits))
        self.assertEqual(
            [self._base_commit, self._c11, self._c12, self._c13, self._c14, self._c15],
            commits[0:6],
        )

    def assert_update_logic(
        self, stdout: str, num_fast_path: int = 0, num_slow_path: int = 0
    ) -> None:
        """Helper function to examine the stdout of an `hg --debug update` call
        and verify the number of times our Hg extension exercised the "fast
        path" for Eden when doing an update versus the number of times it
        exercised the "slow path."
        """
        self.assertEqual(
            num_fast_path,
            stdout.count("using eden update code path\n"),
            msg=(
                "Number of times `hg update` should exercise the fast path: "
                + str(num_fast_path)
            ),
        )
        self.assertEqual(
            num_slow_path,
            stdout.count("falling back to non-eden update code path: "),
            msg=(
                "Number of times `hg update` should exercise the slow path: "
                + str(num_slow_path)
            ),
        )

    def test_rebase_with_concurrent_status(self) -> None:
        """
        Test using `hg rebase` to rebase a stack while simultaneously running
        `hg status`
        """
        stop = threading.Event()

        def status_thread():
            while not stop.is_set():
                self.repo.run_hg("status", stdout=None, stderr=None)

        # Spawn several threads to run "hg status" in parallel with the rebase
        num_threads = 6
        threads = []
        for _ in range(num_threads):
            t = threading.Thread(target=status_thread)
            threads.append(t)
            t.start()

        # Run the rebase.  Explicitly disable inmemory rebase so that eden
        # will need to update the working directory state as tehe rebase progresses
        self.repo.run_hg(
            "--debug",
            "--config",
            "rebase.experimental.inmemory=False",
            "rebase",
            "-s",
            self._c11,
            "-d",
            self._c25,
            stdout=None,
            stderr=None,
        )
        new_commit = self.hg("log", "-rtip", "-T{node}")

        stop.set()
        for t in threads:
            t.join()

        self.assert_status_empty()

        # Verify that the new commit looks correct
        self.repo.update(new_commit)
        self.assert_status_empty()
        self.assert_file_regex("numbers/1/15", "15\n")
        self.assert_file_regex("numbers/2/25", "25\n")
