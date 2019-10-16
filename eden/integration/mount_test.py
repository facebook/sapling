#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import shutil
import subprocess
from pathlib import Path
from typing import Optional, Set

from eden.cli.util import poll_until
from eden.thrift import EdenClient, EdenNotRunningError
from facebook.eden.ttypes import (
    EdenError,
    FaultDefinition,
    MountState,
    UnblockFaultArg,
    WorkingDirectoryParents,
)
from fb303_core.ttypes import fb303_status
from thrift.Thrift import TException

from .lib import testcase


@testcase.eden_repo_test
# pyre-fixme[13]: Attribute `expected_mount_entries` is never initialized.
class MountTest(testcase.EdenRepoTest):
    expected_mount_entries: Set[str]
    enable_fault_injection: bool = True

    def populate_repo(self) -> None:
        self.repo.write_file("hello", "hola\n")
        self.repo.write_file("adir/file", "foo!\n")
        self.repo.write_file("bdir/test.sh", "#!/bin/bash\necho test\n", mode=0o755)
        self.repo.write_file("bdir/noexec.sh", "#!/bin/bash\necho test\n")
        self.repo.symlink("slink", "hello")
        self.repo.commit("Initial commit.")

        self.expected_mount_entries = {".eden", "adir", "bdir", "hello", "slink"}

    def test_remove_unmounted_checkout(self) -> None:
        # Clone a second checkout mount point
        mount2 = os.path.join(self.mounts_dir, "mount2")
        self.eden.clone(self.repo_name, mount2)
        self.assertEqual(
            {self.mount: "RUNNING", mount2: "RUNNING"}, self.eden.list_cmd_simple()
        )

        # Now unmount it
        self.eden.run_cmd("unmount", mount2)
        self.assertEqual(
            {self.mount: "RUNNING", mount2: "NOT_RUNNING"}, self.eden.list_cmd_simple()
        )
        # The Eden README telling users what to do if their mount point is not mounted
        # should be present in the original mount point directory.
        self.assertTrue(os.path.exists(os.path.join(mount2, "README_EDEN.txt")))

        # Now use "eden remove" to destroy mount2
        self.eden.remove(mount2)
        self.assertEqual({self.mount: "RUNNING"}, self.eden.list_cmd_simple())
        self.assertFalse(os.path.exists(mount2))

    def test_unmount_remount(self) -> None:
        # write a file into the overlay to test that it is still visible
        # when we remount.
        filename = os.path.join(self.mount, "overlayonly")
        with open(filename, "w") as f:
            f.write("foo!\n")

        self.assert_checkout_root_entries(self.expected_mount_entries | {"overlayonly"})
        self.assertTrue(self.eden.in_proc_mounts(self.mount))

        # do a normal user-facing unmount, preserving state
        self.eden.run_cmd("unmount", self.mount)

        self.assertFalse(self.eden.in_proc_mounts(self.mount))
        entries = set(os.listdir(self.mount))
        self.assertEqual({"README_EDEN.txt"}, entries)

        # Now remount it with the mount command
        self.eden.run_cmd("mount", self.mount)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))
        self.assert_checkout_root_entries(self.expected_mount_entries | {"overlayonly"})

        with open(filename, "r") as f:
            self.assertEqual("foo!\n", f.read(), msg="overlay file is correct")

    def test_double_unmount(self) -> None:
        # Test calling "unmount" twice.  The second should fail, but edenfs
        # should still work normally afterwards
        self.eden.run_cmd("unmount", self.mount)
        self.eden.run_unchecked("unmount", self.mount)

        # Now remount it with the mount command
        self.eden.run_cmd("mount", self.mount)

        self.assertTrue(self.eden.in_proc_mounts(self.mount))
        self.assert_checkout_root_entries({".eden", "adir", "bdir", "hello", "slink"})

    def test_unmount_succeeds_while_file_handle_is_open(self) -> None:
        fd = os.open(os.path.join(self.mount, "hello"), os.O_RDWR)
        # This test will fail or time out if unmounting times out.
        self.eden.run_cmd("unmount", self.mount)
        # Surprisingly, os.close does not return an error when the mount has
        # gone away.
        os.close(fd)

    def test_unmount_succeeds_while_dir_handle_is_open(self) -> None:
        fd = os.open(self.mount, 0)
        # This test will fail or time out if unmounting times out.
        self.eden.run_cmd("unmount", self.mount)
        # Surprisingly, os.close does not return an error when the mount has
        # gone away.
        os.close(fd)

    def test_mount_init_state(self) -> None:
        self.eden.run_cmd("unmount", self.mount)
        self.assertEqual({self.mount: "NOT_RUNNING"}, self.eden.list_cmd_simple())

        with self.eden.get_thrift_client() as client:
            fault = FaultDefinition(keyClass="mount", keyValueRegex=".*", block=True)
            client.injectFault(fault)

            # Run the "eden mount" CLI command.
            # This won't succeed until we unblock the mount.
            mount_cmd = self.eden.get_eden_cli_args("mount", self.mount)
            mount_proc = subprocess.Popen(mount_cmd)

            # Wait for the new mount to be reported by edenfs
            def mount_started() -> Optional[bool]:
                if self.eden.get_mount_state(Path(self.mount), client) is not None:
                    return True
                if mount_proc.poll() is not None:
                    raise Exception(
                        f"eden mount command finished (with status "
                        f"{mount_proc.returncode}) while mounting was "
                        f"still blocked"
                    )
                return None

            poll_until(mount_started, timeout=30)
            self.assertEqual({self.mount: "INITIALIZING"}, self.eden.list_cmd_simple())

            # Most thrift calls to access the mount should be disallowed while it is
            # still initializing.
            self._assert_thrift_calls_fail_during_mount_init(client)

            # Unblock mounting and wait for the mount to transition to running
            client.unblockFault(UnblockFaultArg(keyClass="mount", keyValueRegex=".*"))

            self._wait_for_mount_running(client)
            self.assertEqual({self.mount: "RUNNING"}, self.eden.list_cmd_simple())

    def _assert_thrift_calls_fail_during_mount_init(self, client: EdenClient) -> None:
        error_regex = "mount point .* is still initializing"
        mount_path = Path(self.mount)
        null_commit = b"\00" * 20

        with self.assertRaisesRegex(EdenError, error_regex):
            client.getFileInformation(mountPoint=bytes(mount_path), paths=[b""])

        with self.assertRaisesRegex(EdenError, error_regex):
            client.getScmStatus(
                mountPoint=bytes(mount_path), listIgnored=False, commit=null_commit
            )

        parents = WorkingDirectoryParents(parent1=null_commit)
        with self.assertRaisesRegex(EdenError, error_regex):
            client.resetParentCommits(mountPoint=bytes(mount_path), parents=parents)

    def test_start_blocked_mount_init(self) -> None:
        self.eden.shutdown()
        self.eden.spawn_nowait(
            extra_args=["--enable_fault_injection", "--fault_injection_block_mounts"]
        )

        # Wait for eden to report the mount point in the listMounts() output
        def is_initializing() -> Optional[bool]:
            try:
                with self.eden.get_thrift_client() as client:
                    if self.eden.get_mount_state(Path(self.mount), client) is not None:
                        return True
                assert self.eden._process is not None
                # pyre-fixme[16]: `Optional` has no attribute `poll`.
                if self.eden._process.poll():
                    self.fail("eden exited before becoming healthy")
                return None
            except (EdenNotRunningError, TException):
                return None

        poll_until(is_initializing, timeout=60)
        with self.eden.get_thrift_client() as client:
            # Since we blocked mount initialization the mount should still
            # report as INITIALIZING, and edenfs should report itself STARTING
            self.assertEqual({self.mount: "INITIALIZING"}, self.eden.list_cmd_simple())
            self.assertEqual(fb303_status.STARTING, client.getStatus())

            # Unblock mounting and wait for the mount to transition to running
            client.unblockFault(UnblockFaultArg(keyClass="mount", keyValueRegex=".*"))
            self._wait_for_mount_running(client)
            self._wait_until_alive(client)
            self.assertEqual(fb303_status.ALIVE, client.getStatus())

        self.assertEqual({self.mount: "RUNNING"}, self.eden.list_cmd_simple())

    def test_start_no_mount_wait(self) -> None:
        self.eden.shutdown()
        self.eden.start(
            extra_args=[
                "--noWaitForMounts",
                "--enable_fault_injection",
                "--fault_injection_block_mounts",
            ]
        )
        self.assertEqual({self.mount: "INITIALIZING"}, self.eden.list_cmd_simple())

        # Unblock mounting and wait for the mount to transition to running
        with self.eden.get_thrift_client() as client:
            self.assertEqual(fb303_status.ALIVE, client.getStatus())
            client.unblockFault(UnblockFaultArg(keyClass="mount", keyValueRegex=".*"))
            self._wait_for_mount_running(client)

        self.assertEqual({self.mount: "RUNNING"}, self.eden.list_cmd_simple())

    def _wait_for_mount_running(self, client: EdenClient) -> None:
        def mount_running() -> Optional[bool]:
            if (
                self.eden.get_mount_state(Path(self.mount), client)
                == MountState.RUNNING
            ):
                return True
            return None

        poll_until(mount_running, timeout=60)

    def _wait_until_alive(self, client: EdenClient) -> None:
        def is_alive() -> Optional[bool]:
            if client.getStatus() == fb303_status.ALIVE:
                return True
            return None

        poll_until(is_alive, timeout=60)

    def test_remount_creates_mount_point_dir(self) -> None:
        """Test that eden will automatically create the mount point directory if
        needed when it is setting up its mount points.
        """
        # Create a second checkout in a directory a couple levels deep so that
        # we can remove some of the parent directories of the checkout.
        checkout_path = Path(self.tmp_dir) / "checkouts" / "stuff" / "myproject"
        self.eden.run_cmd("clone", self.repo.path, str(checkout_path))
        self.assert_checkout_root_entries(self.expected_mount_entries, checkout_path)

        self.eden.run_cmd("unmount", str(checkout_path))
        self.assertEqual(
            {self.mount: "RUNNING", str(checkout_path): "NOT_RUNNING"},
            self.eden.list_cmd_simple(),
        )

        # Confirm that "eden mount" recreates the mount point directory
        shutil.rmtree(Path(self.tmp_dir) / "checkouts")
        self.eden.run_cmd("mount", str(checkout_path))
        self.assert_checkout_root_entries(self.expected_mount_entries, checkout_path)
        self.assertTrue(self.eden.in_proc_mounts(str(checkout_path)))
        self.assertEqual(
            {self.mount: "RUNNING", str(checkout_path): "RUNNING"},
            self.eden.list_cmd_simple(),
        )

        # Also confirm that Eden recreates the mount points on startup as well
        self.eden.shutdown()
        shutil.rmtree(Path(self.tmp_dir) / "checkouts")
        shutil.rmtree(self.mount_path)
        self.eden.start()
        self.assertEqual(
            {self.mount: "RUNNING", str(checkout_path): "RUNNING"},
            self.eden.list_cmd_simple(),
        )
        self.assert_checkout_root_entries(self.expected_mount_entries, checkout_path)

        # Check the behavior if Eden fails to create one of the mount point directories.
        # We just confirm that Eden still starts and mounts the other checkout normally
        # in this case.
        self.eden.shutdown()
        checkouts_dir = Path(self.tmp_dir) / "checkouts"
        shutil.rmtree(checkouts_dir)
        checkouts_dir.write_text("now a file\n")
        self.eden.start()
        self.assertEqual(
            {self.mount: "RUNNING", str(checkout_path): "NOT_RUNNING"},
            self.eden.list_cmd_simple(),
        )

    def test_remount_creates_bind_mounts_if_needed(self) -> None:
        # Add a repo definition to the config that includes some bind mounts.
        edenrc = os.path.join(self.home_dir, ".edenrc")
        project_id = "myproject"
        with open(edenrc, "w") as f:
            f.write(
                f"""\
["repository {project_id}"]
path = "{self.repo.get_canonical_root()}"
type = "{self.repo.get_type()}"

["bindmounts {project_id}"]
buck-out = "buck-out"
xplat-buck-out = "xplat/buck-out"
"""
            )

        # Clone the repository
        checkout_path = Path(self.tmp_dir) / "myproject_clone"
        self.eden.run_cmd("clone", project_id, str(checkout_path))
        checkout_state_dir = self.eden.client_dir_for_mount(checkout_path)
        bind_mounts_src_dir = checkout_state_dir / "bind-mounts"

        # Check that the bind mounts are set up correctly
        bind_src1 = bind_mounts_src_dir / "buck-out"
        bind_dest1 = checkout_path / "buck-out"
        bind_src2 = bind_mounts_src_dir / "xplat-buck-out"
        bind_dest2 = checkout_path / "xplat" / "buck-out"
        self._assert_bind_mounted(bind_src1, bind_dest1)
        self._assert_bind_mounted(bind_src2, bind_dest2)

        # Unmount this checkout
        self.eden.run_cmd("unmount", str(checkout_path))

        # Remove the bind-mount source directories.
        # This shouldn't really happen in normal practice, but in some cases users have
        # manually deleted these directories when trying to clean up disk space to
        # recover from disk full situations.
        shutil.rmtree(bind_mounts_src_dir)

        # Remount the checkout and make sure the bind mounts
        # still got recreated correctly
        self.eden.run_cmd("mount", str(checkout_path))
        self._assert_bind_mounted(bind_src1, bind_dest1)
        self._assert_bind_mounted(bind_src2, bind_dest2)

        # Also check that the bind mounts are recreated by "eden start"
        self.eden.shutdown()
        shutil.rmtree(bind_mounts_src_dir)
        self.eden.start()
        self._assert_bind_mounted(bind_src1, bind_dest1)
        self._assert_bind_mounted(bind_src2, bind_dest2)

    def _assert_bind_mounted(self, src: Path, dest: Path) -> None:
        # Both the source and destination paths should exist,
        # and should refer to the same directory
        src_stat = src.lstat()
        dest_stat = dest.lstat()
        self.assertEqual(
            (src_stat.st_dev, src_stat.st_ino), (dest_stat.st_dev, dest_stat.st_ino)
        )
