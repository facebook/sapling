#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import signal
import sys
import threading
from multiprocessing import Process
from pathlib import Path
from typing import Dict, List, Optional

import pexpect
from eden.fs.cli.util import EdenStartError, get_pid_using_lockfile, poll_until
from eden.thrift.legacy import EdenClient
from facebook.eden.ttypes import FaultDefinition, MountState, UnblockFaultArg
from fb303_core.ttypes import fb303_status

from .lib import testcase
from .lib.find_executables import FindExe


# pyre-ignore[13]: T62487924
class TakeoverTestBase(testcase.EdenRepoTest):
    pagesize: int
    page1: str
    page2: str
    commit1: str
    commit2: str
    enable_fault_injection: bool = True

    def populate_repo(self) -> None:
        import resource

        self.pagesize = resource.getpagesize()
        self.page1 = "1" * self.pagesize
        self.page2 = "2" * self.pagesize
        self.repo.write_file("tree/hello", self.page1 + self.page2)
        self.repo.write_file("tree/deleted", self.page1 + self.page2)
        self.repo.write_file("src/main.c", "hello world")
        self.commit1 = self.repo.commit("Initial commit.")

        self.repo.write_file("src/main.c", "hello world v2")
        self.repo.write_file("src/test/test1.py", "test1")
        self.repo.write_file("src/test/test2.py", "test2")
        self.commit2 = self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        """we need to persist data across restarts"""
        return "sqlite"

    def edenfs_logging_settings(self) -> Dict[str, str]:
        if self._testMethodName == "test_takeover_with_io":
            # test_takeover_with_io causes lots of I/O, so do not enable
            # verbose logging of I/O operations in this test.
            return {}
        return {
            "eden.strace": "DBG7",
            "eden.fs.fuse": "DBG7",
            "eden.fs.inodes.InodeMap": "DBG6",
            "eden.fs.takeover": "DBG7",
            "eden.fs.service": "DBG4",
        }

    def do_takeover_test(self) -> None:
        hello = os.path.join(self.mount, "tree/hello")
        deleted = os.path.join(self.mount, "tree/deleted")
        deleted_local = os.path.join(self.mount, "deleted-local")

        # To test our handling of unlinked inodes, in addition
        # to unlinking something that is in the manifest we
        # need to check that we handle the case of a local
        # file being deleted to make sure that we cover both
        # code paths for FileInode.
        with open(deleted_local, "w") as dl:
            dl.write(self.page1)
            dl.write(self.page2)

        # We'd like to make sure that we do something reasonable
        # for directories that have been unlinked and that are
        # still referenced via a file descriptor.  Ideally we'd call
        # opendir() here and then readdir() it after we've performed
        # the graceful restart, but we can't directly call those
        # functions from python.  The approach used here is to
        # open a file descriptor to the directory and then try
        # to stat() it after the restart.  Since the directory
        # has to be empty in order to be unlinked, a readdir
        # from it wouldn't return any interesting results anyway.
        deleted_dir = os.path.join(self.mount, "deleted-dir")
        os.mkdir(deleted_dir)
        deleted_dir_fd = os.open(deleted_dir, 0)
        os.rmdir(deleted_dir)

        with open(hello, "r") as f, open(deleted, "r") as d, open(
            deleted_local, "r"
        ) as dl:
            # Read the first page only (rather than the whole file)
            # before we restart the process.
            # This is so that we can check that the kernel really
            # does call in to us for the second page and that we're
            # really servicing the read for the second page and that
            # it isn't just getting served from the kernel buffer cache
            self.assertEqual(self.page1, f.read(self.pagesize))

            # Let's make sure that unlinked inodes continue to
            # work appropriately too.  We've opened the file
            # handles and are holding them alive in `d` and `dl`,
            # so now let's unlink it from the filesystem
            os.unlink(deleted)
            os.unlink(deleted_local)

            print("=== beginning restart ===", file=sys.stderr)
            self.eden.graceful_restart()
            print("=== restart complete ===", file=sys.stderr)

            # Ensure that our file handle is still live across
            # the restart boundary
            f.seek(0)
            self.assertEqual(self.page1, f.read(self.pagesize))
            self.assertEqual(self.page2, f.read(self.pagesize))

            # We should be able to read from the `d` file handle
            # even though we deleted the file from the tree
            self.assertEqual(self.page1, d.read(self.pagesize))
            self.assertEqual(self.page2, d.read(self.pagesize))
            # Likewise for the `dl` file handle
            self.assertEqual(self.page1, dl.read(self.pagesize))
            self.assertEqual(self.page2, dl.read(self.pagesize))

        # Now check that the unlinked directory handle still seems
        # connected.  This is difficult to do directly in python;
        # the directory had to be empty in order to be removed
        # so even if we could read its directory entries there
        # wouldn't be anything to read.
        # Note that os.stat() will throw if the fd is deemed
        # bad either by the kernel or the eden instance,
        # so we're just calling it and discarding the return
        # value.
        os.stat(deleted_dir_fd)
        os.close(deleted_dir_fd)

        # Let's also test opening the same file up again,
        # just to make sure that that is still working after
        # the graceful restart.
        with open(hello, "r") as f:
            self.assertEqual(self.page1, f.read(self.pagesize))
            self.assertEqual(self.page2, f.read(self.pagesize))


@testcase.eden_repo_test
class TakeoverTest(TakeoverTestBase):
    def test_takeover(self) -> None:
        return self.do_takeover_test()

    def test_takeover_after_diff_revisions(self) -> None:
        # Make a getScmStatusBetweenRevisions() call to Eden.
        # Previously this thrift call caused Eden to create temporary inode
        # objects outside of the normal root inode tree, and this would cause
        # Eden to crash when shutting down afterwards.
        with self.get_thrift_client_legacy() as client:
            client.getScmStatusBetweenRevisions(
                os.fsencode(self.mount),
                self.commit1.encode("utf-8"),
                self.commit2.encode("utf-8"),
            )

        return self.do_takeover_test()

    def test_takeover_with_io(self) -> None:
        num_threads = 4
        write_chunk_size = 1024 * 1024
        max_file_length = write_chunk_size * 100

        # TODO: Setting this higher than 1 currently makes it likely that
        # edenfs will crash during restart.
        # There are still some other bugs we need to track down in the restart
        # ordering.
        num_restarts = 1

        stop = threading.Event()
        bufs = [b"x" * write_chunk_size, b"y" * write_chunk_size]

        def do_io(thread_id: int, running_event: threading.Event) -> None:
            path = os.path.join(self.mount, "src", "test", "data%d.log" % thread_id)
            with open(path, "wb") as f:
                # Use raw file descriptors to avoid going through python's I/O
                # buffering code.
                fd = f.fileno()

                buf_idx = 0
                buf = bufs[buf_idx]
                offset = 0

                # Repeatedly write and rewrite the same file,
                # alternating between two different data buffers.
                running_event.set()
                while True:
                    os.pwrite(fd, buf, offset)
                    if stop.is_set():
                        return
                    offset += len(buf)
                    if offset >= max_file_length:
                        buf_idx += 1
                        buf = bufs[buf_idx % len(bufs)]
                        offset = 0

        # Log the mount points device ID at the start of the test
        # (Just in case anything hangs and we need to abort the mount
        # using /sys/fs/fuse/connections/<dev>/)
        st = os.lstat(self.mount)
        print("=== eden mount device=%d ===" % st.st_dev, file=sys.stderr)

        # Start several threads doing I/O while we we perform a takeover
        threads = []
        try:
            running_events = []
            for n in range(num_threads):
                running = threading.Event()
                thread = threading.Thread(target=do_io, args=(n, running))
                thread.start()
                threads.append(thread)
                running_events.append(running)

            # Wait until all threads have started and are doing I/O
            for event in running_events:
                event.wait()

            # Restart edenfs
            for n in range(num_restarts):
                print("=== beginning restart %d ===" % n, file=sys.stderr)
                self.eden.graceful_restart()
                print("=== restart %d complete ===" % n, file=sys.stderr)
        finally:
            stop.set()
            for thread in threads:
                thread.join()

    def test_takeover_updates_process_id_in_lock_file(self) -> None:
        self.assertEqual(
            self.eden.get_pid_via_thrift(),
            get_pid_using_lockfile(Path(self.eden.eden_dir)),
        )
        self.eden.graceful_restart()
        self.assertEqual(
            self.eden.get_pid_via_thrift(),
            get_pid_using_lockfile(Path(self.eden.eden_dir)),
        )

    def test_takeover_preserves_inode_numbers_for_open_nonmaterialized_files(
        self,
    ) -> None:
        hello = os.path.join(self.mount, "tree/hello")

        fd = os.open(hello, os.O_RDONLY)
        try:
            inode_number = os.fstat(fd).st_ino

            self.eden.graceful_restart()

            self.assertEqual(inode_number, os.fstat(fd).st_ino)
        finally:
            os.close(fd)

        fd = os.open(hello, os.O_RDONLY)
        try:
            self.assertEqual(inode_number, os.fstat(fd).st_ino)
        finally:
            os.close(fd)

    def test_contents_are_the_same_if_handle_is_held_open(self) -> None:
        with open(os.path.join(self.mount, "tree", "hello")) as c2_hello_file, open(
            os.path.join(self.mount, "src", "main.c")
        ) as c2_mainc_file:

            self.eden.graceful_restart()
            self.eden.run_cmd(
                "debug", "flush_cache", os.path.join("tree", "hello"), cwd=self.mount
            )
            self.eden.run_cmd(
                "debug", "flush_cache", os.path.join("src", "main.c"), cwd=self.mount
            )

            self.assertEqual(self.page1 + self.page2, c2_hello_file.read())
            self.assertEqual("hello world v2", c2_mainc_file.read())

    def test_readdir_after_graceful_restart(self) -> None:
        # Ensure capability flags (e.g. FUSE_NO_OPENDIR_SUPPORT) survive
        # graceful restart
        self.eden.graceful_restart()
        self.assertEqual(
            ["test1.py", "test2.py"],
            sorted(os.listdir(os.path.join(self.mount, "src", "test"))),
        )

    def test_readdir_before_and_after_graceful_restart(self) -> None:
        self.assertEqual(
            ["test1.py", "test2.py"],
            sorted(os.listdir(os.path.join(self.mount, "src", "test"))),
        )
        self.eden.graceful_restart()
        self.assertEqual(
            ["test1.py", "test2.py"],
            sorted(os.listdir(os.path.join(self.mount, "src", "test"))),
        )

    def test_takeover_doesnt_send_ping(self) -> None:
        """
        tests that if we try a takeover with a version that doesn't know
        how to accept a ping, we don't send one. This test should not fail
        in either case since it is running against a client that knows how
        to listen for a ping. It just is used to look at logs to make sure
        the correct code path is entered.
        """
        self.eden.fake_takeover_with_version(3)

    def test_takeover_failure(self) -> None:
        print("=== beginning restart ===", file=sys.stderr)
        self.eden.takeover_without_ping_response()
        print("=== restart complete ===", file=sys.stderr)
        self.assertTrue(self.eden.wait_for_is_healthy())

    def run_restart(self) -> "pexpect.spawn[bytes]":
        edenfsctl, env = FindExe.get_edenfsctl_env()
        restart_cmd = [
            edenfsctl,
            "--config-dir",
            str(self.eden_dir),
            "--etc-eden-dir",
            str(self.etc_eden_dir),
            "--home-dir",
            str(self.home_dir),
            "restart",
            "--daemon-binary",
            FindExe.FAKE_EDENFS,
        ]

        print("Restarting eden: %r" % (restart_cmd,))
        return pexpect.spawn(
            restart_cmd[0],
            restart_cmd[1:],
            logfile=sys.stdout.buffer,
            timeout=120,
            env=env,
        )

    def assert_restart_fails_with_in_progress_graceful_restart(
        self, client: EdenClient
    ) -> None:
        pid = self.eden.get_pid_via_thrift()
        p = self.run_restart()
        p.expect_exact(
            f"The current edenfs daemon (pid {pid}) is in the middle of stopping."
            f"\r\nUse `eden restart --force` if you want to forcibly restart the current daemon\r\n"
        )
        p.wait()
        self.assertEqual(p.exitstatus, 4)

        self.assertEqual(client.getDaemonInfo().status, fb303_status.STOPPING)

    def assert_shutdown_fails_with_in_progress_graceful_restart(
        self, client: EdenClient
    ) -> None:
        # call initiateShutdown. This should not throw.
        try:
            client.initiateShutdown("shutdown requested during graceful restart")
        except Exception:
            self.fail(
                "initiateShutdown should not throw when graceful restart is in progress"
            )

        self.assertEqual(client.getDaemonInfo().status, fb303_status.STOPPING)

    def assert_sigkill_fails_with_in_progress_graceful_restart(
        self, client: EdenClient
    ) -> None:
        # send SIGTERM to process. This should not throw.
        pid = self.eden.get_pid_via_thrift()
        try:
            os.kill(pid, signal.SIGTERM)
        except Exception:
            self.fail(
                "sending SIGTERM should not throw when graceful restart is in progress"
            )

        self.assertEqual(client.getDaemonInfo().status, fb303_status.STOPPING)

    def test_stop_during_takeover(self) -> None:
        # block graceful restart
        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(
                    keyClass="takeover", keyValueRegex="server_shutdown", block=True
                )
            )

            self.eden.wait_for_is_healthy()

            # Run a graceful restart
            # This won't succeed until we unblock the shutdown.
            p = Process(target=self.eden.graceful_restart)
            p.start()

            # Wait for the state to be shutting down
            def state_shutting_down() -> Optional[bool]:
                if not p.is_alive():
                    raise Exception(
                        "eden restart --graceful command finished while "
                        "graceful restart was still blocked"
                    )
                if client.getDaemonInfo().status is fb303_status.STOPPING:
                    return True
                return None

            poll_until(state_shutting_down, timeout=60)

            # Normal restart should be rejected while a graceful restart
            # is in progress
            self.assert_restart_fails_with_in_progress_graceful_restart(client)

            # Normal shutdown should be rejected while a graceful restart
            # is in progress
            self.assert_shutdown_fails_with_in_progress_graceful_restart(client)

            # Getting SIGTERM should not kill process while a graceful restart is in
            # progress
            self.assert_sigkill_fails_with_in_progress_graceful_restart(client)

            # Unblock the server shutdown and wait for the graceful restart to complete.
            client.unblockFault(
                UnblockFaultArg(keyClass="takeover", keyValueRegex="server_shutdown")
            )

            p.join()

    def test_takeover_during_mount(self) -> None:
        self.eden.unmount(self.mount_path)

        with self.eden.get_thrift_client_legacy() as client:
            client.injectFault(
                FaultDefinition(keyClass="mount", keyValueRegex=".*", block=True)
            )

        try:
            mountProcess = Process(target=self.eden.mount, args=(self.mount_path,))
            mountProcess.start()

            def mount_initializing() -> Optional[bool]:
                with self.eden.get_thrift_client_legacy() as client:
                    for mount_info in client.listMounts():
                        if mount_info.mountPoint == self.mount_path_bytes:
                            if mount_info.state == MountState.INITIALIZING:
                                return True
                            return False
                return None

            poll_until(mount_initializing, timeout=60)

            with self.assertRaisesRegex(
                EdenStartError, "edenfs exited before becoming healthy"
            ):
                self.eden.graceful_restart()
        finally:
            with self.eden.get_thrift_client_legacy() as client:
                client.unblockFault(
                    UnblockFaultArg(keyClass="mount", keyValueRegex=".*")
                )
            mountProcess.join()


@testcase.eden_repo_test(run_on_nfs=False)
class TakeoverTestNoNFSServer(TakeoverTestBase):
    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        return {}

    def test_takeover(self) -> None:
        return self.do_takeover_test()


@testcase.eden_repo_test
class TakeoverRocksDBStressTest(testcase.EdenRepoTest):
    enable_fault_injection: bool = True

    def populate_repo(self) -> None:
        self.repo.write_file("test-directory/file", "")
        # pyre-fixme[16]: `TakeoverRocksDBStressTest` has no attribute `commit1`.
        self.commit1 = self.repo.commit("Initial commit.")

    def select_storage_engine(self) -> str:
        return "rocksdb"

    def test_takeover_with_tree_inode_loading_from_local_store(self) -> None:
        """
        Restart edenfs while a tree inode is being loaded asynchronously. Ensure
        restarting does not deadlock.
        """

        def load_test_directory_inode_from_local_store_asynchronously() -> None:
            """
            Make edenfs start loading "/test-directory" from the local store.

            To ensure that the local store is in use during takeover, load the tree
            inode using a prefetch.

            At the time of writing, os.listdir("foo") causes edenfs to prefetch
            the tree inodes of foo/*. Exploit this to load the tree inode for
            "/directory".

            Other options considered:

            * At the time of writing, if we load the tree inode using a FUSE
              request (e.g. os.stat), edenfs would wait for the FUSE request to
              finish before starting the inode shutdown procedure.

            * At the time of writing, 'edenfsctl prefetch' does not prefetch
              tree inodes asynchronously.
            """
            os.listdir(self.mount)

        graceful_restart_startup_time = 5.0

        with self.eden.get_thrift_client_legacy() as client:
            for key_class in ["local store get single", "local store get batch"]:
                client.injectFault(
                    FaultDefinition(
                        keyClass=key_class,
                        keyValueRegex=".*",
                        delayMilliseconds=int(graceful_restart_startup_time * 1000),
                        count=100,
                    )
                )

        load_test_directory_inode_from_local_store_asynchronously()
        self.eden.graceful_restart()
