#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import binascii
import os
import random
import socket
import stat
import sys
import time
from threading import Thread
from typing import Dict, List

from eden.integration.lib.hgrepo import HgRepository
from facebook.eden.ttypes import (
    EdenError,
    EdenErrorType,
    GetScmStatusParams,
    ScmFileStatus,
    ScmStatus,
)

from .lib.hg_extension_test_base import EdenHgTestCase, hg_cached_status_test


@hg_cached_status_test
# pyre-ignore[13]: T62487924
class StatusTest(EdenHgTestCase):
    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("hello.txt", "hola")
        repo.write_file("subdir/file.txt", "contents")
        repo.commit("Initial commit.")

    def test_status(self) -> None:
        """Test various `hg status` states in the root of an Eden mount."""
        self.assert_status_empty()

        self.touch("world.txt")
        self.assert_status({"world.txt": "?"})

        self.hg("add", "world.txt")
        self.assert_status({"world.txt": "A"})

        self.rm("hello.txt")
        self.assert_status({"hello.txt": "!", "world.txt": "A"})

        with open(self.get_path("hello.txt"), "w") as f:
            f.write("new contents")
        self.assert_status({"hello.txt": "M", "world.txt": "A"})

        self.hg("forget", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("rm", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        # If the file is already forgotten, `hg rm` does not remove it from
        # disk.
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("add", "hello.txt")
        self.assert_status({"hello.txt": "M", "world.txt": "A"})
        self.assertEqual("new contents", self.read_file("hello.txt"))

        self.hg("rm", "--force", "hello.txt")
        self.assert_status({"hello.txt": "R", "world.txt": "A"})
        self.assertFalse(os.path.exists(self.get_path("hello.txt")))

    def test_ignored(self) -> None:
        self.repo.write_file(".gitignore", "ignore_me\n")
        self.repo.commit("gitignore")

        self.touch("ignore_me")
        self.assert_status({"ignore_me": "I"})

    def thoroughly_get_scm_status(
        self, client, mountPoint, commit, listIgnored, expected_status
    ) -> None:
        status_from_get_scm_status = client.getScmStatus(
            mountPoint=bytes(mountPoint, encoding="utf-8"),
            commit=commit,
            listIgnored=False,
        )
        status_from_get_scm_status_v2 = client.getScmStatusV2(
            GetScmStatusParams(
                mountPoint=bytes(mountPoint, encoding="utf-8"),
                commit=commit,
                listIgnored=False,
                rootIdOptions=None,
            )
        ).status

        self.assertEqual(
            status_from_get_scm_status,
            status_from_get_scm_status_v2,
            "getScmStatus and getScmStatusV2 should agree",
        )

    def test_status_thrift_apis(self) -> None:
        """Test both the getScmStatusV2() and getScmStatus() thrift APIs."""
        # This confirms that both thrift APIs continue to work,
        # independently of the one currently used by hg.
        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)

        enable_status_cache = self.enable_status_cache

        with self.get_thrift_client_legacy() as client:

            # Test with a clean status.
            expected_status = ScmStatus(entries={}, errors={})
            self.thoroughly_get_scm_status(
                client, self.mount, initial_commit, False, expected_status
            )

            if enable_status_cache:
                self.counter_check(client, miss_cnt=1, hit_cnt=1)
            else:
                self.counter_check(client, miss_cnt=0, hit_cnt=0)

            # Modify the working directory and then test again
            self.repo.write_file("hello.txt", "saluton")
            self.touch("new_tracked.txt")

            self.hg("add", "new_tracked.txt")

            # `hg add` would trigger a call to getScmStatusV2
            if enable_status_cache:
                self.counter_check(client, miss_cnt=2, hit_cnt=1)
            else:
                self.counter_check(client, miss_cnt=0, hit_cnt=0)

            self.touch("untracked.txt")
            expected_entries = {
                b"hello.txt": ScmFileStatus.MODIFIED,
                b"new_tracked.txt": ScmFileStatus.ADDED,
                b"untracked.txt": ScmFileStatus.ADDED,
            }

            expected_status = ScmStatus(entries=expected_entries, errors={})
            self.thoroughly_get_scm_status(
                client, self.mount, initial_commit, False, expected_status
            )

            if enable_status_cache:
                self.counter_check(client, miss_cnt=3, hit_cnt=2)
            else:
                self.counter_check(client, miss_cnt=0, hit_cnt=0)

            # Commit the modifications
            self.repo.commit("committing changes")

    def test_status_with_non_parent(self) -> None:
        # This confirms that an error is thrown if getScmStatusV2 is called
        # with a commit that is not the parent commit
        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)

        with self.get_thrift_client_legacy() as client:
            # Add file to commit
            self.touch("new_tracked.txt")
            self.hg("add", "new_tracked.txt")

            # Commit the modifications
            self.repo.commit("committing changes")

            # Test calling getScmStatusV2() with a commit that is not the parent commit
            with self.assertRaises(EdenError) as context:
                client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=initial_commit,
                        listIgnored=False,
                    )
                )
            self.assertEqual(
                EdenErrorType.OUT_OF_DATE_PARENT, context.exception.errorType
            )

            self.use_customized_config(
                client,
                {"hg": ["enforce-parents = false"]},
            )

            try:
                client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=initial_commit,
                        listIgnored=False,
                    )
                )
            except EdenError as ex:
                self.fail(
                    "getScmStatusV2 threw after setting enforce-parents to false with {}".format(
                        ex
                    )
                )

    def test_manual_revert(self) -> None:
        self.assert_status_empty()
        self.write_file("dir1/a.txt", "original contents\n")
        self.hg("add", "dir1/a.txt")
        self.repo.commit("create a.txt")
        self.assert_status_empty()

        self.write_file("dir1/a.txt", "updated contents\n")
        self.repo.commit("modify a.txt")
        self.assert_status_empty()

        self.write_file("dir1/a.txt", "original contents\n")
        self.repo.commit("revert a.txt")
        self.assert_status_empty()

    def test_truncation_upon_open_modifies_file(self) -> None:
        fd = os.open(os.path.join(self.mount, "subdir/file.txt"), os.O_TRUNC)
        try:
            self.assert_status({"subdir/file.txt": "M"})
        finally:
            os.close(fd)

    def test_truncation_after_open_modifies_file(self) -> None:
        fd = os.open(os.path.join(self.mount, "subdir/file.txt"), os.O_WRONLY)
        try:
            os.ftruncate(fd, 0)
            self.assert_status({"subdir/file.txt": "M"})
        finally:
            os.close(fd)

    def test_partial_truncation_after_open_modifies_file(self) -> None:
        fd = os.open(os.path.join(self.mount, "subdir/file.txt"), os.O_WRONLY)
        try:
            os.ftruncate(fd, 1)
            self.assert_status({"subdir/file.txt": "M"})
        finally:
            os.close(fd)

    def test_irrelevant_chmod_is_ignored_by_status(self) -> None:
        path = os.path.join(self.mount, "hello.txt")
        mode = os.lstat(path).st_mode
        mode |= stat.S_IXGRP
        os.chmod(path, mode)
        self.assert_status_empty()

    def test_rename_materialized(self) -> None:
        self.write_file("subdir1/file.txt", "contents")
        self.assert_status({"subdir1/file.txt": "?"})

        subdir1 = os.path.join(self.mount, "subdir1")
        subdir2 = os.path.join(self.mount, "subdir2")
        os.rename(subdir1, subdir2)
        self.assert_status({"subdir2/file.txt": "?"})

    def test_status_socket(self) -> None:
        if sys.platform == "win32":
            from eden.thrift.windows_thrift import WindowsSocketHandle  # @manual

            uds = WindowsSocketHandle()
        else:
            uds = socket.socket(family=socket.AF_UNIX)

        uds.bind(os.path.join(self.mount, "socket"))
        uds.close()
        self.assert_status({"socket": "?"})

    def test_no_ignore_tracked(self) -> None:
        self.repo.write_file(".gitignore", "subdir/foo/file.txt")
        self.repo.write_file("subdir/foo/file.txt", "ignored but tracked file")
        commit_with_ignored = self.repo.commit("Commit with ignored file")

        self.repo.remove_file("subdir/foo/file.txt")
        commit_with_ignored_removed = self.repo.commit("Commit with removed file")
        self.repo.update(commit_with_ignored)

        with self.get_thrift_client_legacy() as client:
            status_from_get_scm_status = client.getScmStatus(
                mountPoint=self.mount_path_bytes,
                commit=commit_with_ignored_removed.encode(),
                listIgnored=False,
            )

        hg_status = self.repo.status(rev=commit_with_ignored_removed)

        # Check that both Mercurial and EdenFS agree when computing status.
        self.assertIn("subdir/foo/file.txt", hg_status)
        self.assertEqual(len(hg_status), 1)
        self.assertEqual(hg_status["subdir/foo/file.txt"], "A")

        self.assertIn(b"subdir/foo/file.txt", status_from_get_scm_status.entries)
        self.assertEqual(len(status_from_get_scm_status.entries), 1)
        self.assertEqual(
            status_from_get_scm_status.entries[b"subdir/foo/file.txt"],
            ScmFileStatus.ADDED,
        )

    def use_customized_config(self, client, config: Dict[str, List[str]]) -> None:
        edenrc = os.path.join(self.home_dir, ".edenrc")
        self.write_configs(config, edenrc)

        # Makes sure that EdenFS picks up our updated config,
        # since we wrote it out after EdenFS started.
        client.reloadConfig()

    def counter_check(self, client, miss_cnt, hit_cnt) -> None:
        if self.enable_status_cache and sys.platform == "win32":
            # currently we are not filtering out file changes under ".hg/"
            # somehow the cache counters can be impacted by file
            # changes under ".hg/"
            # before that, let's skip the counter check for now
            # TODO: remove this once we ignore the changes under ".hg/" reported by Journal
            return

        timeout_seconds = 2.0
        poll_interval_seconds = 0.1
        deadline = time.monotonic() + timeout_seconds
        while True:
            any_failure = False
            for name, expect_count in zip(["miss", "hit"], [miss_cnt, hit_cnt]):
                counter_name = f"journal.status_cache_{name}.count"
                actual_count = client.getCounters().get(counter_name)
                try:
                    self.assertEqual(
                        expect_count,
                        actual_count,
                        f"unexpected counter {counter_name}: {expect_count}(expected) vs {actual_count}(real)",
                    )
                except AssertionError as e:
                    any_failure = True
                    if time.monotonic() >= deadline:
                        raise e
                    time.sleep(poll_interval_seconds)
                    continue
            if not any_failure:
                break

    def test_scm_status_cache(self) -> None:
        """Test the SCM status cache"""
        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)

        if not self.enable_status_cache:
            # no need to test the cache if it is not enabled
            return

        with self.get_thrift_client_legacy() as client:
            # disable enforce parent check
            self.use_customized_config(
                client,
                {"hg": ["enforce-parents = false"]},
            )

            # at the beginning, all counters should be 0
            self.counter_check(client, miss_cnt=0, hit_cnt=0)

            self.assert_status_empty()
            self.counter_check(client, miss_cnt=1, hit_cnt=0)

            # a second call should hit the cache
            self.assert_status_empty()
            self.counter_check(client, miss_cnt=1, hit_cnt=1)

            self.touch("world.txt")
            self.assert_status({"world.txt": "?"})
            self.counter_check(client, miss_cnt=2, hit_cnt=1)

            self.hg("add", "world.txt")
            self.counter_check(client, miss_cnt=3, hit_cnt=1)

            second_commit = self.repo.commit("adding world")
            # looks like `commit` method would internally call it twice and miss twice
            self.counter_check(client, miss_cnt=5, hit_cnt=1)

            def verify_status(commit, listIgnored, expect_status) -> None:
                res = client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=commit,
                        listIgnored=listIgnored,
                    )
                )
                self.assertEqual(expect_status, dict(res.status.entries))

            verify_status(second_commit, True, {})
            self.counter_check(client, miss_cnt=6, hit_cnt=1)

            verify_status(second_commit, True, {})
            self.counter_check(client, miss_cnt=6, hit_cnt=2)

            verify_status(second_commit, False, {})
            self.counter_check(client, miss_cnt=7, hit_cnt=2)

            verify_status(initial_commit, True, {b"world.txt": 0})  # '0' means ADDED
            self.counter_check(client, miss_cnt=8, hit_cnt=2)

            verify_status(initial_commit, False, {b"world.txt": 0})
            self.counter_check(client, miss_cnt=9, hit_cnt=2)

            verify_status(initial_commit, False, {b"world.txt": 0})
            self.counter_check(client, miss_cnt=9, hit_cnt=3)

    def test_scm_status_cache_concurrent_calls(self) -> None:
        """Test the SCM status cache when there are concurrent calls to getScmStatusV2"""
        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)

        if not self.enable_status_cache:
            # no need to test the cache if it is not enabled
            return

        with self.get_thrift_client_legacy() as client:
            # disable enforce parent check
            self.use_customized_config(
                client,
                {"hg": ["enforce-parents = false"]},
            )

            # at the beginning, all counters should be 0
            self.counter_check(client, miss_cnt=0, hit_cnt=0)

            def two_threads_call_in_parallel(func, args_1=(), args_2=()):
                t1 = Thread(target=func, args=args_1)
                t2 = Thread(target=func, args=args_2)
                t1.start()
                t2.start()
                t1.join(30)
                t2.join(30)

            two_threads_call_in_parallel(
                self.assert_status_empty,
            )

            # we can't assert the exact number of hits and misses since
            # we don't know if both two threads miss or only one of them misses.

            self.touch("world.txt")
            two_threads_call_in_parallel(
                self.assert_status,
                (self, {"world.txt": "?"}),
                (self, {"world.txt": "?"}),
            )

            self.hg("add", "world.txt")
            second_commit = self.repo.commit("adding world")

            def verify_status(cls, commit, listIgnored, expect_status) -> None:
                with cls.get_thrift_client_legacy() as thread_client:
                    res = thread_client.getScmStatusV2(
                        GetScmStatusParams(
                            mountPoint=bytes(self.mount, encoding="utf-8"),
                            commit=commit,
                            listIgnored=listIgnored,
                        )
                    )
                    cls.assertEqual(expect_status, dict(res.status.entries))

            commit_list = [initial_commit, second_commit]
            listIgnoredFlags = [True, False]
            arg_pairs = [(x, y) for x in commit_list for y in listIgnoredFlags]
            # arg_pairs = list(zip(commit_list, listIgnoredFlags))
            random.shuffle(arg_pairs)

            # testing concurrent calls with same arguments
            print(f"arg_pairs: {arg_pairs}")
            for commit, flag in arg_pairs:
                arg_tuple = (
                    self,
                    commit,
                    flag,
                    {b"world.txt": 0} if commit == initial_commit else {},
                )
                two_threads_call_in_parallel(
                    verify_status, args_1=arg_tuple, args_2=arg_tuple
                )

            # "testing concurrent calls with different arguments"
            arg_pairs_1 = random.sample(arg_pairs, len(arg_pairs))
            arg_pairs_2 = random.sample(arg_pairs, len(arg_pairs))
            print(f"arg_pairs_1: {arg_pairs_1}")
            print(f"arg_pairs_2: {arg_pairs_2}")
            for i in range(len(arg_pairs)):
                arg_tuple_1 = (
                    self,
                    *arg_pairs_1[i],
                    {b"world.txt": 0} if arg_pairs_1[i][0] == initial_commit else {},
                )
                arg_tuple_2 = (
                    self,
                    *arg_pairs_2[i],
                    {b"world.txt": 0} if arg_pairs_2[i][0] == initial_commit else {},
                )
                two_threads_call_in_parallel(
                    verify_status, args_1=arg_tuple_1, args_2=arg_tuple_2
                )


@hg_cached_status_test
# pyre-ignore[13]: T62487924
class StatusEdgeCaseTest(EdenHgTestCase):
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("subdir/file.txt", "contents")
        self.commit1 = repo.commit("commit 1")
        repo.write_file("subdir/file.txt", "contents", mode=0o775)
        self.commit2 = repo.commit("commit 2")
        self.assertNotEqual(self.commit1, self.commit2)

    def select_storage_engine(self) -> str:
        """we need to persist data across restarts"""
        return "sqlite"

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.strace": "DBG7",
            "eden.fs.inodes.TreeInode": "DBG9",
        }

    @EdenHgTestCase.unix_only
    def test_file_created_with_relevant_mode_difference_and_then_fixed_is_ignored(
        self,
    ) -> None:
        self.repo.update(self.commit1)
        path = os.path.join(self.mount, "subdir", "file.txt")
        os.unlink(path)
        fd = os.open(path, os.O_CREAT | os.O_WRONLY, mode=0o775)
        try:
            os.write(fd, b"contents")
        finally:
            os.close(fd)

        self.assert_status({"subdir/file.txt": "M"})
        os.chmod(path, 0o664)
        self.assert_status_empty()
        self.repo.update(self.commit2)
        self.eden.restart()
        self.assert_status_empty()

    @EdenHgTestCase.unix_only
    def test_dematerialized_file_created_with_different_mode_is_unchanged(self) -> None:
        path = os.path.join(self.mount, "subdir", "file.txt")
        # save inode numbers and initial dtype
        os.lstat(path)
        # materialize and remove executable bit
        os.chmod(path, 0o664)
        self.assert_status({"subdir/file.txt": "M"})
        # make an untracked file so the checkout doesn't reallocate inodes
        os.close(os.open(os.path.join(self.mount, "subdir", "sibling"), os.O_CREAT))
        self.repo.update(self.commit1, merge=True)
        # put the old contents back
        os.unlink(os.path.join(self.mount, "subdir", "sibling"))
        self.assert_status_empty()
        self.eden.restart()
        os.chmod(os.path.join(self.mount, "subdir"), 0o664)
        self.assert_status_empty()


# Define a separate TestCase class purely to test with different initial
# repository contents.
@hg_cached_status_test
# pyre-ignore[13]: T62487924
class StatusRevertTest(EdenHgTestCase):
    commit1: str
    commit2: str
    commit3: str
    commit4: str

    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("dir1/a.txt", "original contents of a\n")
        repo.write_file("dir1/b.txt", "b.txt\n")
        repo.write_file("dir1/c.txt", "c.txt\n")
        repo.write_file("dir2/x.txt", "x.txt\n")
        repo.write_file("dir2/y.txt", "y.txt\n")
        self.commit1 = repo.commit("Initial commit.")

        repo.write_file("dir1/a.txt", "updated contents of a\n", add=False)
        self.commit2 = repo.commit("commit 2")

        repo.write_file("dir1/b.txt", "updated b\n", add=False)
        self.commit3 = repo.commit("commit 3")

        repo.write_file("dir1/a.txt", "original contents of a\n")
        self.commit4 = repo.commit("commit 4")

    def test_reverted_contents(self) -> None:
        self.assert_status_empty()
        # Read dir1/a.txt so it is loaded by edenfs
        self.read_file("dir1/a.txt")

        # Reset the state from commit4 to commit1 without actually doing a
        # checkout.  dir1/a.txt has the same contents in commit4 as in commit1,
        # but different blob hashes.
        self.hg("reset", "--keep", self.commit1)
        # Only dir1/b.txt should be reported as modified.
        # dir1/a.txt should not show up in the status output.
        self.assert_status({"dir1/b.txt": "M"})
