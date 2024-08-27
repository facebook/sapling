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

import thrift.transport

from eden.integration.lib.hgrepo import HgRepository
from facebook.eden.ttypes import (
    EdenError,
    EdenErrorType,
    FaultDefinition,
    GetBlockedFaultsRequest,
    GetScmStatusParams,
    ScmFileStatus,
    ScmStatus,
    SynchronizeWorkingCopyParams,
    UnblockFaultArg,
)

from .lib.hg_extension_test_base import EdenHgTestCase, hg_cached_status_test

THREAD_JOIN_TIMEOUT_SECONDS = 3

WINDOWS_RUNTIME_ERR_PREFIX = "class " if sys.platform == "win32" else ""


@hg_cached_status_test
# pyre-ignore[13]: T62487924
class StatusTest(EdenHgTestCase):
    enable_fault_injection: bool = True

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
            client.synchronizeWorkingCopy(
                self.mount.encode("utf-8"), SynchronizeWorkingCopyParams()
            )

            # `hg add` would trigger a call to getScmStatusV2
            if enable_status_cache:
                self.counter_check(client, miss_cnt=2, hit_cnt=1)
            else:
                self.counter_check(client, miss_cnt=0, hit_cnt=0)

            self.touch("untracked.txt")
            client.synchronizeWorkingCopy(
                self.mount.encode("utf-8"), SynchronizeWorkingCopyParams()
            )
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
            miss_cnt, hit_cnt = 0, 0
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            # the first call should miss the cache
            miss_cnt += 1
            self.assert_status_empty()
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            # a second call should hit the cache
            hit_cnt += 1
            self.assert_status_empty()
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            self.touch("world.txt")
            num_of_tries = self.assert_status({"world.txt": "?"})
            expected_num_of_miss = 1
            hit_cnt += num_of_tries - expected_num_of_miss
            miss_cnt += expected_num_of_miss
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            self.hg("add", "world.txt")
            # `hg add` would internally call getStatus with listIgnored=False.
            # But the cached key was from listIgnored=True, so the key mismatches
            # and the call misses the cache
            miss_cnt += 1

            second_commit = self.repo.commit("adding world")
            # `commit` method would internally call getStatus twice
            # against the old commit with listIgnoired=False.
            # but these two calls won't return new entries since there are
            # only changes under .hg folder
            hit_cnt += 2
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            def verify_status(commit, listIgnored, expect_status) -> None:
                res = client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=commit,
                        listIgnored=listIgnored,
                    )
                )
                self.assertEqual(expect_status, dict(res.status.entries))

            verify_status(second_commit, True, {})  # miss
            miss_cnt += 1
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            verify_status(second_commit, True, {})  # hit
            hit_cnt += 1
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            verify_status(second_commit, False, {})  # miss
            miss_cnt += 1
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            # cache miss because a commit will update the working directory
            # so the cached result from the previous assert_status call is lost
            # at this point
            verify_status(initial_commit, True, {b"world.txt": 0})  # '0' means ADDED
            miss_cnt += 1
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            # cache miss due to the same reason as above
            verify_status(initial_commit, False, {b"world.txt": 0})
            miss_cnt += 1
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

            verify_status(initial_commit, False, {b"world.txt": 0})
            hit_cnt += 1
            self.counter_check(client, miss_cnt=miss_cnt, hit_cnt=hit_cnt)

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
                t1.join(THREAD_JOIN_TIMEOUT_SECONDS)
                t2.join(THREAD_JOIN_TIMEOUT_SECONDS)

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

    def wait_for_status_cache_block_hit(self, client):
        poll_interval_seconds = 0.1
        deadline = time.monotonic() + 2
        while True:
            response = client.getBlockedFaults(
                GetBlockedFaultsRequest(keyclass="scmStatusCache")
            )
            if len(response.keyValues) == 1:
                break
            if time.monotonic() >= deadline:
                raise Exception("timeout waiting for the block hit")
            time.sleep(poll_interval_seconds)

    def test_status_shared_among_requests(self) -> None:
        """Test that status requests with the same parameters will
        wait for the first request to finish setting the value."""

        if not self.enable_status_cache:
            # no need to test the cache if it is not enabled
            return

        with self.get_thrift_client_legacy() as client:
            self.touch("world.txt")
            client.synchronizeWorkingCopy(
                self.mount.encode("utf-8"), SynchronizeWorkingCopyParams()
            )
            client.injectFault(
                FaultDefinition(
                    keyClass="scmStatusCache",
                    keyValueRegex="blocking setValue",
                    block=True,
                    count=1,
                )
            )
            num_requests = 10
            threads = []

            def thread_worker(cls, exceptions: List[Exception]) -> None:
                try:
                    cls.assert_status(
                        {"world.txt": "?"}, timeout_seconds=0
                    )  # retry can mess counters
                except Exception as e:
                    exceptions.append(e)

            exceptions = []
            t = Thread(target=thread_worker, args=(self, exceptions))
            t.start()
            threads.append(t)

            try:
                # wait for the block hit
                self.wait_for_status_cache_block_hit(client)

                for _ in range(num_requests - 1):
                    t = Thread(target=thread_worker, args=(self, exceptions))
                    t.start()
                    threads.append(t)

                # all threads should be blocking
                for t in threads:
                    assert (
                        t.is_alive()
                    ), f"thread should be blocking. dumping exceptions: {exceptions}"
            finally:
                client.unblockFault(
                    UnblockFaultArg(
                        keyClass="scmStatusCache", keyValueRegex="blocking setValue"
                    )
                )

            for t in threads:
                t.join(THREAD_JOIN_TIMEOUT_SECONDS)
            assert len(exceptions) == 0, f"no exception should be raised: {exceptions}"
            self.counter_check(client, miss_cnt=1, hit_cnt=num_requests - 1)

    def test_status_cache_expire_blocking_setValue(self) -> None:
        self.status_cache_expire_blocing_common("setValue")

    def test_status_cache_expire_blocking_insert(self) -> None:
        self.status_cache_expire_blocing_common("insert")

    def test_status_cache_expire_blocking_dropPromise(self) -> None:
        self.status_cache_expire_blocing_common("dropPromise")

    # not suing subTest because it's hard to get threading working correctly with a clean env
    def status_cache_expire_blocing_common(self, check_point) -> None:
        """Test that status requests with latest journal sequence number will
        invalidate the existing cache with old sequence number."""

        if not self.enable_status_cache:
            # no need to test the cache if it is not enabled
            return

        def thread_worker(cls, expect_status, exceptions: List[Exception]) -> None:
            try:
                cls.assert_status(
                    expect_status, timeout_seconds=0
                )  # retry can mess counters
            except Exception as e:
                exceptions.append(e)

        block_key_value = "blocking " + check_point

        with self.get_thrift_client_legacy() as client:
            self.touch("world.txt")
            client.injectFault(
                FaultDefinition(
                    keyClass="scmStatusCache",
                    keyValueRegex=block_key_value,
                    block=True,
                    count=1,  # so the second thread will not be blocked
                )
            )
            exceptions = []
            thread_expect_one_entry = Thread(
                target=thread_worker,
                args=(self, {"world.txt": "?"}, exceptions),
            )
            thread_expect_one_entry.start()

            try:
                # wait for the block hit
                self.wait_for_status_cache_block_hit(client)

                # touching a new file should advance the journal sequence number
                self.touch("peace.txt")
                thread_expect_two_entries = Thread(
                    target=thread_worker,
                    # no matter where is the previous thread bloked, this thread
                    # should always see the latest status
                    args=(self, {"world.txt": "?", "peace.txt": "?"}, exceptions),
                )
                thread_expect_two_entries.start()

                assert (
                    thread_expect_one_entry.is_alive()
                ), f"the first thread should be blocked. dumping exceptions: {exceptions}"
            finally:
                client.unblockFault(
                    UnblockFaultArg(
                        keyClass="scmStatusCache", keyValueRegex=block_key_value
                    )
                )

            for t in [thread_expect_one_entry, thread_expect_two_entries]:
                t.join(THREAD_JOIN_TIMEOUT_SECONDS)
            assert len(exceptions) == 0, f"unexpected exception raised: {exceptions}"

            # no cache should be hit since the sequence number is advanced
            self.counter_check(client, miss_cnt=2, hit_cnt=0)

    def test_status_cache_error_handlilng(self) -> None:
        """Test that when there is error computing the diff, we don't cache the error
        and the next call should succeed"""
        if not self.enable_status_cache:
            # no need to test the cache if it is not enabled
            return

        initial_commit_hex = self.repo.get_head_hash()
        initial_commit = binascii.unhexlify(initial_commit_hex)

        # prepare the folder structure
        self.repo.write_file("parent/file_1.txt", "what")
        self.repo.write_file("parent/file_2.txt", "what")

        self.repo.write_file("parent/child/file_1.txt", "what")
        self.repo.write_file("parent/child/file_2.txt", "what")

        with self.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass="TreeInode::computeDiff",
                    keyValueRegex="parent/child",
                    count=1,
                    errorType="runtime_error",
                )
            )
            initial_status_with_error = client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=bytes(self.mount, encoding="utf-8"),
                    commit=initial_commit,
                    listIgnored=False,
                    rootIdOptions=None,
                )
            ).status
            self.assertDictEqual(
                {
                    b"parent/child": f"{WINDOWS_RUNTIME_ERR_PREFIX}std::runtime_error: injected error"
                },
                initial_status_with_error.errors,
            )
            self.counter_check(client, miss_cnt=1, hit_cnt=0)

            # after the error is cleared, the next call should succeed without errors
            status_without_error = client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=bytes(self.mount, encoding="utf-8"),
                    commit=initial_commit,
                    listIgnored=False,
                    rootIdOptions=None,
                )
            ).status
            self.assertDictEqual(
                {},
                status_without_error.errors,
            )
            # the previous call should not be cached so we are expecting two misses
            self.counter_check(client, miss_cnt=2, hit_cnt=0)

            # writing more files to advance the journal sequence number
            self.repo.write_file("parent/file_3.txt", "what")
            self.repo.write_file("parent/child/file_3.txt", "what")

            # On windows platform, there is a chance that the changes are not
            # synced so this call might hit the cache instead of returning an error.
            client.synchronizeWorkingCopy(
                self.mount.encode("utf-8"), SynchronizeWorkingCopyParams()
            )

            client.injectFault(
                FaultDefinition(
                    keyClass="EdenMount::diff",
                    keyValueRegex=f".*{initial_commit_hex}.*",
                    count=1,
                    errorType="runtime_error",
                    errorMessage="intentional exception",
                )
            )

            try:
                client.getScmStatusV2(
                    GetScmStatusParams(
                        mountPoint=bytes(self.mount, encoding="utf-8"),
                        commit=initial_commit,
                        listIgnored=False,
                    )
                )
                self.fail("status cache should throw exception and fail this request!")
            except thrift.Thrift.TApplicationException as e:
                self.assertEqual(
                    f"{WINDOWS_RUNTIME_ERR_PREFIX}std::runtime_error: intentional exception",
                    e.message,
                )
            self.counter_check(client, miss_cnt=3, hit_cnt=0)

            status_without_error = client.getScmStatusV2(
                GetScmStatusParams(
                    mountPoint=bytes(self.mount, encoding="utf-8"),
                    commit=initial_commit,
                    listIgnored=False,
                )
            ).status

            self.assertDictEqual(
                {
                    b"parent/child/file_1.txt": 0,
                    b"parent/child/file_2.txt": 0,
                    b"parent/child/file_3.txt": 0,
                    b"parent/file_1.txt": 0,
                    b"parent/file_2.txt": 0,
                    b"parent/file_3.txt": 0,
                },
                status_without_error.entries,
            )
            self.counter_check(client, miss_cnt=4, hit_cnt=0)


@hg_cached_status_test
# pyre-ignore[13]: T62487924
class StatusEdgeCaseTest(EdenHgTestCase):
    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str
    # pyre-fixme[13]: Attribute `commit2` is never initialized.
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
    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str
    # pyre-fixme[13]: Attribute `commit2` is never initialized.
    commit2: str
    # pyre-fixme[13]: Attribute `commit3` is never initialized.
    commit3: str
    # pyre-fixme[13]: Attribute `commit4` is never initialized.
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
