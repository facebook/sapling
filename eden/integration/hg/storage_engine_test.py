#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import os
import subprocess

from eden.integration.lib import hgrepo, testcase

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test
from .lib.histedit_command import HisteditCommand


class _Hidden:
    # This _Hidden class exists soley to hide the abstract StorageEngineTest class from
    # the unittest framework, so it does not find it during test discovery.  The
    # unittest code is unfortunately not smart enough to skip abstract classes.

    class StorageEngineTest(EdenHgTestCase, metaclass=abc.ABCMeta):
        _commit1: str
        _commit2: str
        _commit3: str

        # These tests were originally copied from histedit_test.py. It doesn't
        # matter which tests are used as long as commits are created and checked out
        # and a realistic workflow is verified against each storage engine.
        def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
            repo.write_file("first", "")
            self._commit1 = repo.commit("first commit")

            repo.write_file("second", "")
            self._commit2 = repo.commit("second commit")

            repo.write_file("third", "")
            self._commit3 = repo.commit("third commit")

        @abc.abstractmethod
        def select_storage_engine(self) -> str:
            raise NotImplementedError()

        def test_stop_at_earlier_commit_in_the_stack_without_reordering(self) -> None:
            commits = self.repo.log()
            self.assertEqual([self._commit1, self._commit2, self._commit3], commits)

            # histedit, stopping in the middle of the stack.
            histedit = HisteditCommand()
            histedit.pick(self._commit1)
            histedit.stop(self._commit2)
            histedit.pick(self._commit3)

            # We expect histedit to terminate with a nonzero exit code in this case.
            with self.assertRaises(hgrepo.HgError) as context:
                histedit.run(self)
            head = self.repo.log(revset=".")[0]
            expected_msg = (
                "Changes committed as %s. "
                "You may amend the changeset now." % head[:12]
            )
            self.assertIn(expected_msg, str(context.exception))

            # Verify the new commit stack and the histedit termination state.
            # Note that the hash of commit[0] is unpredictable because Hg gives it a
            # new hash in anticipation of the user amending it.
            parent = self.repo.log(revset=".^")[0]
            self.assertEqual(self._commit1, parent)
            self.assertEqual(["first commit", "second commit"], self.repo.log("{desc}"))

            # Make sure the working copy is in the expected state.
            self.assert_status_empty(op="histedit")
            self.assertSetEqual(
                {".eden", ".hg", "first", "second"},
                set(os.listdir(self.repo.get_canonical_root())),
            )

            self.hg("histedit", "--continue")
            self.assertEqual(
                ["first commit", "second commit", "third commit"],
                self.repo.log("{desc}"),
            )
            self.assert_status_empty()
            self.assertSetEqual(
                {".eden", ".hg", "first", "second", "third"},
                set(os.listdir(self.repo.get_canonical_root())),
            )


# Each LocalStore implementation may complete its futures from different
# threads. Verify that Eden works the same with all of them.


@hg_test
# pyre-ignore[13]: T62487924
class HisteditMemoryStorageEngineTest(_Hidden.StorageEngineTest):
    def select_storage_engine(self) -> str:
        return "memory"


@hg_test
# pyre-ignore[13]: T62487924
class HisteditSQLiteStorageEngineTest(_Hidden.StorageEngineTest):
    def select_storage_engine(self) -> str:
        return "sqlite"


@hg_test
# pyre-ignore[13]: T62487924
class HisteditRocksDBStorageEngineTest(_Hidden.StorageEngineTest):
    def select_storage_engine(self) -> str:
        return "rocksdb"


@testcase.eden_test
class FailsToOpenLocalStoreTest(testcase.EdenTestCase):
    enable_fault_injection = True

    def test_start_eden_with_local_store_that_fails_to_open(self) -> None:
        self.eden.shutdown()
        self.eden.start(
            extra_args=["--fault_injection_fail_opening_local_store"],
            should_wait_for_daemon_healthy=False,
        )
        self.assertNotEqual(self.eden._process, None)
        # pyre-ignore[16]: I checked it's not None above :|
        return_code = self.eden._process.wait(timeout=120)
        self.assertNotEqual(return_code, 0)

    def test_restart_eden_with_local_store_that_fails_to_open(self) -> None:
        self.eden.graceful_restart(
            extra_args=["--fault_injection_fail_opening_local_store"],
            should_wait_for_old=True,
            should_wait_for_new=False,
        )
        self.assertNotEqual(self.eden._process, None)
        # pyre-ignore[16]: I checked it's not None above :|
        return_code = self.eden._process.wait(timeout=120)
        self.assertNotEqual(return_code, 0)


@testcase.eden_test
# pyre-ignore[13]: T62487924
class FailsToOpenLocalStoreTestWithMounts(EdenHgTestCase):
    enable_fault_injection = True

    def populate_backing_repo(self, repo) -> None:
        repo.write_file("afile", "blah")

    def test_start_eden_with_local_store_that_fails_to_open(self) -> None:

        self.eden.shutdown()

        self.eden.start(
            extra_args=["--fault_injection_fail_opening_local_store"],
            should_wait_for_daemon_healthy=False,
        )
        self.assertNotEqual(self.eden._process, None)
        # pyre-ignore[16]: I checked it's not None above :|
        return_code = self.eden._process.wait(timeout=120)
        self.assertNotEqual(return_code, 0)

    def cleanup_mount(self) -> None:
        cmd = ["sudo", "/bin/umount", "-lf", self.mount]
        subprocess.call(cmd)

    def test_restart_eden_with_local_store_that_fails_to_open(self) -> None:
        self.eden.graceful_restart(
            extra_args=["--fault_injection_fail_opening_local_store"],
            should_wait_for_old=True,
            should_wait_for_new=False,
        )
        self.assertNotEqual(self.eden._process, None)
        # pyre-ignore[16]: I checked it's not None above :|
        return_code = self.eden._process.wait(timeout=120)
        self.assertNotEqual(return_code, 0)

        self.cleanup_mount()
