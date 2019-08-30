#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import resource
import sys
import threading
from pathlib import Path
from typing import Dict

from eden.cli.util import get_pid_using_lockfile
from facebook.eden.ttypes import FaultDefinition

from .lib import testcase


@testcase.eden_repo_test
class TakeoverTest(testcase.EdenRepoTest):
    # pyre-fixme[13]: Attribute `pagesize` is never initialized.
    pagesize: int
    # pyre-fixme[13]: Attribute `page1` is never initialized.
    page1: str
    # pyre-fixme[13]: Attribute `page2` is never initialized.
    page2: str
    # pyre-fixme[13]: Attribute `commit1` is never initialized.
    commit1: str
    # pyre-fixme[13]: Attribute `commit2` is never initialized.
    commit2: str

    def populate_repo(self) -> None:
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
        """ we need to persist data across restarts """
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

    def test_takeover(self) -> None:
        return self.do_takeover_test()

    def test_takeover_after_diff_revisions(self) -> None:
        # Make a getScmStatusBetweenRevisions() call to Eden.
        # Previously this thrift call caused Eden to create temporary inode
        # objects outside of the normal root inode tree, and this would cause
        # Eden to crash when shutting down afterwards.
        with self.get_thrift_client() as client:
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
                # jalternating between two different data buffers.
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
        self
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

        with self.eden.get_thrift_client() as client:
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
