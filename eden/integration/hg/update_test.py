#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging
import os
import re
import sys
import threading
import unittest
from multiprocessing import Process
from textwrap import dedent
from typing import Dict, List, Optional, Set

from eden.fs.cli import util
from eden.integration.hg.lib.hg_extension_test_base import EdenHgTestCase, hg_test
from eden.integration.lib import hgrepo
from facebook.eden.constants import DIS_ENABLE_FLAGS
from facebook.eden.ttypes import (
    EdenError,
    EdenErrorType,
    FaultDefinition,
    GetScmStatusParams,
    SyncBehavior,
    UnblockFaultArg,
)


if sys.platform == "win32":
    from eden.fs.cli.proc_utils_win import Handle


@hg_test
# pyre-ignore[13]: T62487924
class UpdateTest(EdenHgTestCase):
    commit1: str
    commit2: str
    commit3: str
    enable_fault_injection: bool = True

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.fs.inodes.TreeInode": "DBG5",
            "eden.fs.inodes.CheckoutAction": "DBG5",
            "eden.fs.inodes.CheckoutContext": "DBG5",
        }

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("experimental", []).append("allow-resume-checkout = true")
        return result

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.write_file(".gitignore", "ignoreme\n")
        repo.write_file("foo/.gitignore", "*.log\n")
        repo.write_file("foo/bar.txt", "test\n")
        repo.write_file("foo/subdir/test.txt", "test\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("foo/.gitignore", "*.log\n/_*\n")
        self.commit2 = repo.commit("Update foo/.gitignore")

        repo.write_file("foo/bar.txt", "updated in commit 3\n")
        self.commit3 = repo.commit("Update foo/.gitignore")

    def test_update_clean_reverts_modified_files(self) -> None:
        """Test using `hg update --clean .` to revert file modifications."""
        self.assert_status_empty()

        self.write_file("hello.txt", "saluton")
        self.assert_status({"hello.txt": "M"})

        self.repo.update(".", clean=True)
        self.assertEqual("hola", self.read_file("hello.txt"))
        self.assert_status_empty()

    def test_update_clean_removes_added_and_removed_statuses(self) -> None:
        """Test using `hg update --clean .` in the presence of added and removed
        files."""
        self.write_file("bar/some_new_file.txt", "new file\n")
        self.hg("add", "bar/some_new_file.txt")
        self.hg("remove", "foo/bar.txt")
        self.assertFalse(os.path.isfile(self.get_path("foo/bar.txt")))
        self.assert_status({"foo/bar.txt": "R", "bar/some_new_file.txt": "A"})

        self.repo.update(".", clean=True)
        self.assert_status({"bar/some_new_file.txt": "?"})
        self.assertTrue(os.path.isfile(self.get_path("foo/bar.txt")))
        self.assert_dirstate_empty()

    def test_update_with_gitignores(self) -> None:
        """
        Test `hg update` with gitignore files.

        This exercises the normal checkout and ignore logic, but also exercises
        some additional interesting cases:  The `hg status` calls cause eden to
        create FileInode objects for the .gitignore files, even though they
        have never been requested via FUSE APIs.  When we update them via
        checkout, this triggers FUSE inode invalidation events.  We want to
        make sure the invalidation doesn't cause any errors even though the
        kernel didn't previously know that these inode objects existed.
        """
        # Call `hg status`, which causes eden to internally create FileInode
        # objects for the .gitignore files.
        self.assert_status_empty()

        self.write_file("foo/subdir/test.log", "log data")
        self.write_file("foo/_data", "data file")
        self.assert_status_empty(
            check_ignored=False, msg="test.log and _data should be ignored"
        )
        self.assert_status({"foo/subdir/test.log": "I", "foo/_data": "I"})

        # Call `hg update` to move from commit2 to commit1, which will
        # change the contents of foo/.gitignore.  This will cause edenfs
        # to send an inode invalidation event to FUSE, but FUSE never knew
        # about this inode in the first place.  edenfs should ignore the
        # resulting ENOENT error in response to the invalidation request.
        self.repo.update(self.commit1)
        self.assert_status({"foo/_data": "?"}, check_ignored=False)
        self.assert_status({"foo/subdir/test.log": "I", "foo/_data": "?"})
        self.assertEqual("*.log\n", self.read_file("foo/.gitignore"))
        self.assertEqual("test\n", self.read_file("foo/bar.txt"))

    def test_update_with_new_commits(self) -> None:
        """
        Test running `hg update` to check out commits that were created after
        the edenfs daemon originally started.

        This makes sure edenfs can correctly import new commits that appear in
        the backing store repository.
        """
        new_contents = "New contents for bar.txt\n"
        self.backing_repo.write_file("foo/bar.txt", new_contents)
        new_commit = self.backing_repo.commit("Update foo/bar.txt")

        self.assert_status_empty()
        self.assertNotEqual(new_contents, self.read_file("foo/bar.txt"))

        self.repo.update(new_commit)
        self.assertEqual(new_contents, self.read_file("foo/bar.txt"))
        self.assert_status_empty()

    def test_reset(self) -> None:
        """
        Test `hg reset`
        """
        self.assert_status_empty()
        self.assertEqual("updated in commit 3\n", self.read_file("foo/bar.txt"))

        self.repo.reset(self.commit2, keep=True)
        self.assert_status({"foo/bar.txt": "M"})
        self.assertEqual("updated in commit 3\n", self.read_file("foo/bar.txt"))

        self.repo.update(self.commit2, clean=True)
        self.assert_status_empty()
        self.assertEqual("test\n", self.read_file("foo/bar.txt"))

    def test_update_replace_untracked_dir(self) -> None:
        """
        Create a local untracked directory, then run "hg update -C" to
        checkout a commit where this directory exists in source control.
        """
        self.assert_status_empty()
        # Write some new files in the eden working directory
        self.mkdir("new_project")
        self.write_file("new_project/newcode.c", "test\n")
        self.write_file("new_project/Makefile", "all:\n\techo done!\n")
        self.write_file("new_project/.gitignore", "*.o\n")
        self.write_file("new_project/newcode.o", "\x00\x01\x02\x03\x04")

        # Add the same files to a commit in the backing repository
        self.backing_repo.write_file("new_project/newcode.c", "test\n")
        self.backing_repo.write_file("new_project/Makefile", "all:\n\techo done!\n")
        self.backing_repo.write_file("new_project/.gitignore", "*.o\n")
        new_commit = self.backing_repo.commit("Add new_project")

        # Check the status before we update
        self.assert_status(
            {
                "new_project/newcode.o": "I",
                "new_project/newcode.c": "?",
                "new_project/Makefile": "?",
                "new_project/.gitignore": "?",
            }
        )

        # Now run "hg update -C new_commit"
        self.repo.update(new_commit, clean=True)
        self.assert_status({"new_project/newcode.o": "I"})

    def test_update_with_merge_flag_and_conflict(self) -> None:
        self.write_file("foo/bar.txt", "changing yet again\n")
        with self.assertRaises(hgrepo.HgError) as context:
            self.hg("update", ".^", "--merge")
        self.assertIn(
            b"1 conflicts while merging foo/bar.txt! "
            b"(edit, then use 'hg resolve --mark')",
            context.exception.stderr,
        )
        self.assert_status({"foo/bar.txt": "M"}, op="updatemerge")
        self.assert_file_regex(
            "foo/bar.txt",
            """\
            <<<<<<< working copy.*
            changing yet again
            =======
            test
            >>>>>>> destination.*
            """,
        )

    def test_merge_update_added_file_with_same_contents_in_destination(self) -> None:
        base_commit = self.repo.get_head_hash()

        file_contents = "new file\n"
        self.write_file("bar/some_new_file.txt", file_contents)
        self.hg("add", "bar/some_new_file.txt")
        self.write_file("foo/bar.txt", "Modify existing file.\n")
        new_commit = self.repo.commit("add some_new_file.txt")
        self.assert_status_empty()

        self.repo.update(base_commit)
        self.assert_status_empty()
        self.write_file("bar/some_new_file.txt", file_contents)
        self.hg("add", "bar/some_new_file.txt")
        self.assert_status({"bar/some_new_file.txt": "A"})

        # Note the update fails even though some_new_file.txt is the same in
        # both the working copy and the destination.
        with self.assertRaises(hgrepo.HgError) as context:
            self.repo.update(new_commit)
        self.assertIn(b"abort: conflicting changes", context.exception.stderr)
        self.assertEqual(
            base_commit,
            self.repo.get_head_hash(),
            msg="We should still be on the base commit because "
            "the merge was aborted.",
        )
        self.assert_dirstate({"bar/some_new_file.txt": ("a", 0, "MERGE_BOTH")})
        self.assert_status({"bar/some_new_file.txt": "A"})
        self.assertEqual(file_contents, self.read_file("bar/some_new_file.txt"))

        # Now do the update with --merge specified.
        self.repo.update(new_commit, merge=True)
        self.assert_status_empty()
        self.assertEqual(
            new_commit,
            self.repo.get_head_hash(),
            msg="Should be expected commit hash because nothing has changed.",
        )

    def test_merge_update_added_file_with_conflict_in_destination(self) -> None:
        self._test_merge_update_file_with_conflict_in_destination(True)

    def test_merge_update_untracked_file_with_conflict_in_destination(self) -> None:
        self._test_merge_update_file_with_conflict_in_destination(False)

    def _test_merge_update_file_with_conflict_in_destination(
        self, add_before_updating: bool
    ) -> None:
        base_commit = self.repo.get_head_hash()
        original_contents = "Original contents.\n"
        self.write_file("some_new_file.txt", original_contents)
        self.hg("add", "some_new_file.txt")
        self.write_file("foo/bar.txt", "Modify existing file.\n")
        commit = self.repo.commit("Commit a new file.")
        self.assert_status_empty()

        # Do an `hg prev` and re-create the new file with different contents.
        self.repo.update(base_commit)
        self.assert_status_empty()
        self.assertFalse(os.path.exists(self.get_path("some_new_file.txt")))
        modified_contents = "Re-create the file with different contents.\n"
        self.write_file("some_new_file.txt", modified_contents)

        if add_before_updating:
            self.hg("add", "some_new_file.txt")
            self.assert_status({"some_new_file.txt": "A"})
        else:
            self.assert_status({"some_new_file.txt": "?"})

        # Verify `hg next` updates such that the original contents and commit
        # hash are restored. No conflicts should be reported.
        path_to_backup = ".hg/origbackups/some_new_file.txt"
        expected_backup_file = os.path.join(self.mount, path_to_backup)
        self.assertFalse(os.path.isfile(expected_backup_file))
        with self.assertRaises(hgrepo.HgError) as context:
            self.repo.update(commit, merge=True)
        self.assertIn(
            b"warning: 1 conflicts while merging some_new_file.txt! "
            b"(edit, then use 'hg resolve --mark')",
            context.exception.stderr,
        )
        self.assertEqual(
            commit,
            self.repo.get_head_hash(),
            msg="Even though we have a merge conflict, "
            "we should still be at the new commit.",
        )
        self.assert_dirstate_empty()
        self.assert_status({"some_new_file.txt": "M"}, op="updatemerge")
        merge_contents = dedent(
            """\
        <<<<<<< working copy.*
        Re-create the file with different contents.
        =======
        Original contents.
        >>>>>>> destination.*
        """
        )
        self.assertRegex(self.read_file("some_new_file.txt"), merge_contents)
        self.assert_unresolved(["some_new_file.txt"])

        # Verify the previous version of the file was backed up as expected.
        self.assertTrue(os.path.isfile(expected_backup_file))
        self.assertEqual(modified_contents, self.read_file(path_to_backup))

        # Resolve the merge conflict and complete the update
        resolved_contents = "Merged contents.\n"
        self.write_file("some_new_file.txt", resolved_contents)
        self.hg("resolve", "--mark", "some_new_file.txt")
        self.assert_dirstate_empty()
        self.assert_status({"some_new_file.txt": "M"}, op="updatemerge")
        self.repo.commit("Resolved file changes.")
        self.assert_dirstate_empty()
        self.hg("update", "--continue")
        self.assert_status_empty()
        self.assertEqual(resolved_contents, self.read_file("some_new_file.txt"))

    def test_update_modified_file_to_removed_file_taking_other(self) -> None:
        self.write_file("some_new_file.txt", "I am new!\n")
        self.hg("add", "some_new_file.txt")
        self.repo.commit("Commit a new file.")
        self.write_file("some_new_file.txt", "Make some changes to that new file.\n")

        self.hg("update", ".^", "--merge", "--tool", ":other")
        self.assertFalse(os.path.exists(self.get_path("some_new_file.txt")))
        self.assertFalse(
            os.path.isfile(
                os.path.join(self.mount, ".hg/origbackups/some_new_file.txt")
            ),
            msg="There should not be a backup file because "
            ":other was specified explicitly.",
        )

    def test_update_modified_file_to_removed_file_taking_local(self) -> None:
        self.write_file("some_new_file.txt", "I am new!\n")
        self.hg("add", "some_new_file.txt")
        self.repo.commit("Commit a new file.")
        new_contents = "Make some changes to that new file.\n"
        self.write_file("some_new_file.txt", new_contents)

        self.hg("update", ".^", "--merge", "--tool", ":local")
        self.assertEqual(new_contents, self.read_file("some_new_file.txt"))
        self.assert_status({"some_new_file.txt": "A"})

    def test_update_untracked_added_conflict(self) -> None:
        # Create a commit with a newly-created file foo/new_file.txt
        self.write_file("foo/new_file.txt", "new file\n")
        self.hg("add", "foo/new_file.txt")
        new_commit = self.repo.commit("Add foo/new_file.txt")

        # Switch back to commit 3
        self.hg("update", self.commit3)

        # Write foo/new_file.txt as an untracked file
        self.write_file("foo/new_file.txt", "different contents\n")

        # Try to switch back to the new commit
        result = self.repo.run_hg(
            "update",
            new_commit,
            "--config",
            "experimental.updatecheck=noconflict",
            check=False,
            traceback=False,
        )
        self.maxDiff = None
        # TODO: Make this an assertEquals() once "goto" renaming in docs is
        # rolled out everywhere.
        self.assertRegex(
            result.stderr.decode("utf-8"),
            re.compile(
                "abort: conflicting changes:\n"
                "  foo/new_file.txt\n"
                "\\(commit or (goto|update) --clean to discard changes\\)\n",
                re.MULTILINE,
            ),
        )
        self.assertNotEqual(0, result.returncode)

        self.assert_status({"foo/new_file.txt": "?"})

    def test_update_ignores_untracked_directory(self) -> None:
        base_commit = self.repo.get_head_hash()
        self.mkdir("foo/bar")
        self.write_file("foo/bar/a.txt", "File in directory two levels deep.\n")
        self.write_file("foo/bar/b.txt", "Another file.\n")
        self.hg("add", "foo/bar/a.txt")
        self.assert_status({"foo/bar/a.txt": "A", "foo/bar/b.txt": "?"})
        self.repo.commit("Commit only a.txt.")
        self.assert_status({"foo/bar/b.txt": "?"})
        self.repo.update(base_commit)
        self.assert_status({"foo/bar/b.txt": "?"})
        self.assertFalse(os.path.exists(self.get_path("foo/bar/a.txt")))
        self.assertTrue(os.path.exists(self.get_path("foo/bar/b.txt")))

    def test_mount_state_during_unmount_with_in_progress_checkout(self) -> None:
        mounts = self.eden.run_cmd("list")
        self.assertEqual(f"{self.mount}\n", mounts)

        self.backing_repo.write_file("foo/bar.txt", "new contents")
        new_commit = self.backing_repo.commit("Update foo/bar.txt")

        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass="inodeCheckout", keyValueRegex=".*", block=True
                )
            )

            # Run a checkout
            p1 = Process(target=self.repo.update, args=(new_commit,))
            p1.start()

            hg_parent = self.hg("log", "-r.", "-T{node}")

            # Ensure the checkout has started
            def checkout_in_progress() -> Optional[bool]:
                try:
                    client.getScmStatusV2(
                        GetScmStatusParams(
                            mountPoint=bytes(self.mount, encoding="utf-8"),
                            commit=bytes(hg_parent, encoding="utf-8"),
                            listIgnored=False,
                        )
                    )
                except EdenError as ex:
                    if ex.errorType == EdenErrorType.CHECKOUT_IN_PROGRESS:
                        return True
                    else:
                        raise ex
                return None

            util.poll_until(checkout_in_progress, timeout=30)

            p2 = Process(target=self.eden.unmount, args=(self.mount,))
            p2.start()

            # Wait for the state to be shutting down
            def state_shutting_down() -> Optional[bool]:
                mounts = self.eden.run_cmd("list")
                print(mounts)
                if mounts.find("SHUTTING_DOWN") != -1:
                    return True
                if mounts.find("(not mounted)") != -1:
                    self.fail(
                        "mount should not list status as not mounted while "
                        "checkout is in progress"
                    )
                return None

            util.poll_until(state_shutting_down, timeout=30)

            # Unblock the server shutdown and wait for the checkout to complete.
            client.unblockFault(
                UnblockFaultArg(keyClass="inodeCheckout", keyValueRegex=".*")
            )

            # join the checkout before the unmount because the unmount call
            # won't finish until the checkout has finished
            p1.join()
            p2.join()

    def test_dir_locking(self) -> None:
        """
        Test performing checkouts that modify the directory foo/ while other
        clients are simultaneously renaming untracked files under foo/

        This exercises the interaction of the kernel's inode locks and Eden's
        own user-space locking.  We previously had some situations where
        deadlock could occur because FUSE requests holding kernel inode lock
        were blocked on userspace locks that were held by other threads blocked
        on the kernel inode lock.
        """
        num_checkout_changed_files = 500
        num_rename_threads = 4
        num_checkouts = 4

        # Create a new commit in the backing repository with many new files in
        # the foo/ directory
        for n in range(num_checkout_changed_files):
            path = os.path.join(self.backing_repo.path, "foo", "tracked.%d" % n)
            with open(path, "w") as f:
                f.write("file %d\n" % n)
        self.backing_repo.add_files(["foo"])
        new_commit = self.backing_repo.commit("Add many files under foo/")

        # Spawn several threads that repeatedly rename ignored files under foo/
        stop = threading.Event()

        def rename_worker(thread_id):
            logging.info("rename thread %d starting", thread_id)
            path1 = os.path.join(self.repo.path, "foo", "_%d.log" % thread_id)
            path2 = os.path.join(self.repo.path, "foo", "_%d.log2" % thread_id)

            with open(path1, "w") as f:
                f.write("ignored %d\n" % thread_id)

            count = 0
            while not stop.is_set():
                os.rename(path1, path2)
                os.rename(path2, path1)
                count += 1

            logging.info("rename thread %d performed %d renames", thread_id, count)

        threads = []
        for n in range(num_rename_threads):
            thread = threading.Thread(target=rename_worker, args=(n,))
            threads.append(thread)
            thread.start()

        logging.info("===== starting checkouts")

        commits = [new_commit, self.commit3]
        for n in range(num_checkouts):
            self.repo.update(commits[n % len(commits)])

        logging.info("===== checkouts complete")

        stop.set()
        for thread in threads:
            thread.join()

        logging.info("===== threads stopped")

        # For the most part this test is mainly checking to ensure that
        # we reach this point without causing a deadlock.
        # However go ahead and check that the repository is left in an expected
        # state too.
        if num_checkouts % 2 == 0:
            self.assertEqual(self.commit3, self.repo.get_head_hash())
        else:
            self.assertEqual(new_commit, self.repo.get_head_hash())

        # Assert that the status is empty other than the ignored files
        # created by the rename threads
        self.assert_status(
            {f"foo/_{thread_id}.log": "I" for thread_id in range(num_rename_threads)}
        )

    def test_change_casing_of_populated(self) -> None:
        self.repo.update(self.commit1)

        self.repo.write_file("DIR2/FILE1", "one upper")
        self.repo.write_file("DIR2/FILE2", "two upper")
        upper = self.repo.commit("Upper")

        self.repo.update(self.commit1)

        self.repo.write_file("dir2/file1", "one lower")
        self.repo.write_file("dir2/file2", "two lower")
        self.repo.commit("Lower")

        # Make sure that everything was committed
        self.assert_status_empty()

        # Now update to the first commit...
        self.repo.update(upper)

        # And verify that status is clean
        self.assert_status_empty()

        self.assertEqual(self.read_file("DIR2/FILE1"), "one upper")
        self.assertEqual(self.read_file("DIR2/FILE2"), "two upper")

        if sys.platform == "win32":
            # Double check that the the old names could still be read thanks to
            # the insensitivity of the FS
            self.assertEqual(self.read_file("dir2/file1"), "one upper")
            self.assertEqual(self.read_file("dir2/file2"), "two upper")

        # Finally, make sure that the on-disk casing is the expected one.
        rootlisting = os.listdir(self.repo.path)
        self.assertIn("DIR2", rootlisting)
        self.assertNotIn("dir2", rootlisting)
        self.assertEqual(
            set(os.listdir(os.path.join(self.repo.path, "DIR2"))),
            {"FILE1", "FILE2"},
        )

    def test_change_casing_of_materialized_file(self) -> None:
        self.repo.update(self.commit1)
        self.repo.write_file("dir2/FILE1", "one upper")
        upper = self.repo.commit("Upper")

        self.repo.remove_file("dir2/FILE1")
        self.repo.commit("Removed")

        self.repo.write_file("dir2/file1", "one lower")
        self.repo.commit("Lower")

        self.repo.update(upper)

        # And verify that status is clean
        self.assert_status_empty()

        self.assertEqual(self.read_file("dir2/FILE1"), "one upper")

        dir2listing = os.listdir(os.path.join(self.repo.path, "dir2"))
        self.assertEqual({"FILE1"}, set(dir2listing))

    def test_change_casing_non_populated(self) -> None:
        self.repo.update(self.commit1)
        self.repo.write_file("dir2/FILE1", "one upper")
        upper = self.repo.commit("Upper")

        self.repo.remove_file("dir2/FILE1")
        self.repo.commit("Removed")

        self.repo.write_file("dir2/file1", "one lower")
        lower = self.repo.commit("Lower")

        # First, let's dematerialize everything
        self.repo.update(self.commit1)

        # Go to the lower cased commit
        self.repo.update(lower)

        # Then simply list all the entries
        os.listdir(os.path.join(self.repo.path, "dir2"))

        # Finally, update to the upper cased commit
        self.repo.update(upper)

        # And verify that status is clean
        self.assert_status_empty()

        self.assertEqual(self.read_file("dir2/FILE1"), "one upper")

        dir2listing = os.listdir(os.path.join(self.repo.path, "dir2"))
        self.assertEqual({"FILE1"}, set(dir2listing))

    def test_change_casing_with_untracked(self) -> None:
        self.repo.update(self.commit1)
        self.repo.write_file("DIR2/FILE1", "one upper")
        upper = self.repo.commit("Upper")

        self.repo.remove_file("DIR2/FILE1")
        self.repo.commit("Removed")

        self.repo.write_file("dir2/file1", "one lower")
        self.repo.commit("Lower")

        self.repo.write_file("dir2/untracked", "untracked")

        self.repo.update(upper)

        # On Windows, due to the untracked file, the casing of the directory
        # stays lower case, hence we do not expect "DIR2" to be present in the
        # working copy.
        dirname = "dir2" if sys.platform == "win32" else "DIR2"
        self.assertIn(dirname, set(os.listdir(self.repo.path)))

        if sys.platform == "win32":
            self.assertEqual(
                {"untracked", "FILE1"},
                set(os.listdir(os.path.join(self.repo.path, "DIR2"))),
            )
            self.assertEqual(self.read_file("DIR2/untracked"), "untracked")

        untrackedPath = "dir2/untracked"
        self.assert_status({untrackedPath: "?"})

    def test_update_to_null_with_untracked_directory(self) -> None:
        self.mkdir("foo/subdir/bar")
        self.repo.update("null")
        self.assertEqual(os.listdir(self.get_path("foo")), ["subdir"])

    if sys.platform == "win32":

        def test_remove_materialized_while_stopped(self) -> None:
            # Materialize the file so it's present on disk and will stay there when EdenFS is stopped
            self.repo.write_file("foo/bar.txt", "Materialized\n")

            # When EdenFS is stopped, materialized files can be modified,
            # causing the working copy to differ from EdenFS view of what it
            # should be.
            self.eden.shutdown()
            os.unlink(self.get_path("foo/bar.txt"))
            self.eden.start()

            self.assert_status_empty()
            self.assertEqual(self.read_file("foo/bar.txt"), "updated in commit 3\n")

    def test_update_dir_to_file(self) -> None:
        self.repo.remove_file("foo/subdir")
        self.repo.write_file("foo/subdir", "I am not a directory any more\n")
        commit4 = self.repo.commit("directory to file")

        # test without the directory materialized
        self.repo.update(self.commit3)
        self.repo.update(commit4)

        # and with the directory materialized
        self.repo.update(self.commit3)
        self.read_dir("foo/subdir")
        self.repo.update(commit4)

    def test_resume_interrupted_update(self) -> None:
        """
        Test resuming a hg checkout after Eden was killed mid-checkout
        previously.
        """
        self.backing_repo.write_file("dir1/foo.txt", "Content 1")
        self.backing_repo.write_file("dir2/bar.txt", "Content 1")
        self.backing_repo.write_file("dir3/dog.txt", "Content 1")
        bottom = self.backing_repo.commit("Add")
        self.backing_repo.write_file("dir1/foo.txt", "Content 2")
        self.backing_repo.write_file("dir2/bar.txt", "Content 2")
        self.backing_repo.write_file("dir3/dog.txt", "Content 2")
        middle = self.backing_repo.commit("Edit")
        self.backing_repo.write_file("dir1/foo.txt", "Content 3")
        self.backing_repo.write_file("dir2/bar.txt", "Content 3")
        self.backing_repo.write_file("dir3/dog.txt", "Content 3")
        top = self.backing_repo.commit("Edit again")
        self.repo.update(top)

        # Do no-op write to trigger materialization of dir2. This will force the
        # checkout to process that inode, allowing us to hit the kill fault.
        # At that point, dir1 will have been processed and be pointing to
        # `bottom` while dir2 and dir3 won't have been processed, and be
        # pointing at top still. After we recover, we'll verify that dir2 and
        # dir3 were not materialized during the resumed checkout.
        self.repo.write_file("dir2/bar.txt", "Content 3")

        self.maxDiff = None
        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass="TreeInode::checkout",
                    keyValueRegex="dir2, false",
                    kill=True,
                )
            )

            try:
                self.repo.update(bottom)
            except Exception:
                pass
            else:
                self.fail("'hg update' should've failed if eden crashes")

        # Restart eden
        if self.eden._process is not None:
            util.poll_until(self.eden._process.poll, timeout=30)
        self.eden = self.init_eden_client()
        self.eden.start()

        with self.assertRaisesRegex(
            hgrepo.HgError, f"checkout is in progress.*{bottom}"
        ):
            self.assert_status_empty()

        with self.assertRaisesRegex(
            hgrepo.HgError, f"a previous checkout was interrupted.*{bottom}"
        ):
            self.repo.update(middle)

        output = self.repo.update(bottom, clean=True)
        self.assertEqual("update complete\n", output)

        with self.eden.get_thrift_client_legacy() as client:
            inode_status = client.debugInodeStatus(
                self.repo.path.encode("utf8"),
                b"",
                flags=DIS_ENABLE_FLAGS,
                sync=SyncBehavior(),
            )
            inodes = dict((i.path.decode("utf8"), i) for i in inode_status)
            self.assertNotIn("dir1", inodes)
            self.assertFalse(inodes["dir2"].materialized)
            self.assertNotIn("dir3", inodes)


@hg_test
# pyre-ignore[13]: T62487924
class UpdateCacheInvalidationTest(EdenHgTestCase):
    commit1: str
    commit2: str
    commit3: str
    commit4: str

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.fs.inodes.TreeInode": "DBG5",
            "eden.fs.inodes.CheckoutAction": "DBG5",
            "eden.fs.inodes.CheckoutContext": "DBG5",
            "eden.fs.fuse.FuseChannel": "DBG3",
        }

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("dir/file1", "one")
        repo.write_file("dir/file2", "two")
        self.commit1 = repo.commit("Initial commit.")

        repo.remove_file("dir/file1")
        self.commit2 = repo.commit("Remove file1")

        repo.write_file("dir/file3", "three")
        self.commit3 = repo.commit("Add file3")

        repo.update(self.commit1)
        repo.write_file("dir/file2", "new two")
        self.commit4 = repo.commit("Change file2")

    def _populate_kernel_caches(self) -> None:
        # Populate the kernel's readdir caches.
        for _dirpath, _dirnames, _filenames in os.walk(self.repo.path):
            pass

    def _list_contents(self, path) -> Set[str]:
        return set(os.listdir(os.path.join(self.repo.path, path)))

    def _scan_contents(self, path) -> List[os.DirEntry]:
        entries = list(os.scandir(os.path.join(self.repo.path, path)))
        entries.sort(key=lambda entry: entry.name)
        return entries

    def test_update_adding_file_invalidates_tree_inode_caches(self) -> None:
        self.repo.update(self.commit2)
        self._populate_kernel_caches()
        self.assertEqual({"file2"}, self._list_contents("dir"))

        # The checkout operation should invalidate the kernel's caches.
        self.repo.update(self.commit3)
        self.assertEqual({"file2", "file3"}, self._list_contents("dir"))

    def test_update_removing_file_invalidates_tree_inode_caches(self) -> None:
        self.repo.update(self.commit1)
        self._populate_kernel_caches()

        self.assertEqual({"file1", "file2"}, self._list_contents("dir"))

        # The checkout operation should invalidate the kernel's caches.
        self.repo.update(self.commit2)
        self.assertEqual({"file2"}, self._list_contents("dir"))

    def test_changing_file_contents_creates_new_inode_and_flushes_dcache(self) -> None:
        self.repo.update(self.commit1)
        self._populate_kernel_caches()

        before = self._scan_contents("dir")

        self.repo.update(self.commit4)

        after = self._scan_contents("dir")

        self.assertEqual(["file1", "file2"], [x.name for x in before])
        self.assertEqual(["file1", "file2"], [x.name for x in after])

        self.assertEqual(before[0].inode(), after[0].inode())
        self.assertNotEqual(before[1].inode(), after[1].inode())

    def test_clean_update_removes_added_file(self) -> None:
        self.repo.update(self.commit1)

        self.write_file("dir/new_file.txt", "new file")
        self.hg("add", "dir/new_file.txt")
        self.assertTrue(os.path.isfile(self.get_path("dir/new_file.txt")))
        self.assert_status({"dir/new_file.txt": "A"})

        self._populate_kernel_caches()
        self.repo.update(".", clean=True)
        self.assert_status({"dir/new_file.txt": "?"})
        self.assertTrue(os.path.isfile(self.get_path("dir/new_file.txt")))
        self.assert_dirstate_empty()

        self.assertEqual({"file1", "file2", "new_file.txt"}, self._list_contents("dir"))

    def test_clean_update_adds_removed_file(self) -> None:
        self.hg("remove", "dir/file1")
        self.assertFalse(os.path.isfile(self.get_path("dir/file1")))
        self.assert_status({"dir/file1": "R"})

        self._populate_kernel_caches()
        self.repo.update(".", clean=True)
        self.assert_status({})
        self.assertTrue(os.path.isfile(self.get_path("dir/file1")))
        self.assert_dirstate_empty()

        self.assertEqual({"file1", "file2"}, self._list_contents("dir"))

    def test_update_change_stat(self) -> None:
        self.repo.write_file("dir/file2", "foobar")
        self.repo.commit("Change file2")

        filepath = self.get_path("dir/file2")
        prestats = os.stat(filepath)
        self.assertEqual(prestats.st_size, 6)

        self.repo.update(self.commit4)

        poststats = os.stat(filepath)
        self.assertEqual(poststats.st_size, 7)

    if sys.platform == "win32":

        def _open_locked(self, path: str, directory: bool = False) -> Handle:
            import ctypes
            from ctypes.wintypes import (
                DWORD as _DWORD,
                HANDLE as _HANDLE,
                LPCWSTR as _LPCWSTR,
            )

            win32 = ctypes.windll.kernel32
            win32.CreateFileW.argtypes = [
                _LPCWSTR,
                _DWORD,
                _DWORD,
                ctypes.c_void_p,
                _DWORD,
                _DWORD,
                ctypes.c_void_p,
            ]
            win32.CreateFileW.restype = _HANDLE

            GENERIC_READ = 0x80000000
            OPEN_EXISTING = 3
            FILE_ATTRIBUTE_NORMAL = 0x80
            FILE_FLAG_BACKUP_SEMANTICS = 0x02000000
            INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

            path = self.get_path(path)

            flags = FILE_ATTRIBUTE_NORMAL
            if directory:
                flags |= FILE_FLAG_BACKUP_SEMANTICS
            fhandle = win32.CreateFileW(
                path, GENERIC_READ, 0, None, OPEN_EXISTING, flags, None
            )
            self.assertNotEqual(fhandle, INVALID_HANDLE_VALUE)
            return Handle(fhandle)

        def test_file_locked_change_content(self) -> None:
            self.repo.update(self.commit1)

            with self._open_locked("dir/file2"):
                with self.assertRaises(hgrepo.HgError):
                    self.repo.update(self.commit4)

            self.assertEqual(self.read_file("dir/file2"), "new")
            self.assert_status({"dir/file2": "M"})

        def test_file_locked_removal(self) -> None:
            self.repo.update(self.commit3)
            self.assertEqual(self.read_file("dir/file3"), "three")
            with self._open_locked("dir/file3"):
                with self.assertRaises(hgrepo.HgError):
                    self.repo.update(self.commit4)

            self.assertEqual(self.read_file("dir/file3"), "three")
            self.assert_status({"dir/file3": "?"})
