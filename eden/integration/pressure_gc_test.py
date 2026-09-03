#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import asyncio
import contextlib
import os
import subprocess
import sys
import time
from typing import Callable, Dict, Generator, List, Optional

from eden.fs.service.eden.thrift_types import (
    DebugInvalidateRequest,
    GetStatInfoParams,
    MountId,
    STATS_MOUNTS_STATS,
    TimeSpec,
)

from .lib import testcase
from .lib.find_executables import FindExe


def privhelper_supports_scan_pins() -> bool:
    """Check whether the privhelper binary used by the daemon under test
    supports the --scan-pins mode. Older binaries reject the flag and exit
    nonzero."""
    privhelper = FindExe.EDEN_PRIVHELPER
    if privhelper is None:
        return False
    try:
        result = subprocess.run(
            [privhelper, "--scan-pins"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            encoding="utf-8",
            timeout=30,
        )
    except (subprocess.TimeoutExpired, OSError):
        return False
    lines = result.stdout.splitlines()
    return result.returncode == 0 and bool(lines) and lines[-1] == "done"


@contextlib.contextmanager
def pinned_cwd_child(cwd_path: str) -> Generator[Callable[[], str], None, None]:
    """Run a child process with its cwd pinned to a directory inside the
    mount. The yielded probe function makes the child call getcwd() and
    returns "cwd:<path>" on success or "errno:<errno>" on failure."""
    child_code = (
        "import os, sys\n"
        "print('ready', flush=True)\n"
        "for _ in sys.stdin:\n"
        "    try:\n"
        "        print(f'cwd:{os.getcwd()}', flush=True)\n"
        "    except OSError as e:\n"
        "        print(f'errno:{e.errno}', flush=True)\n"
    )
    with subprocess.Popen(
        [sys.executable, "-c", child_code],
        cwd=cwd_path,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        encoding="utf-8",
    ) as child:
        stdin = child.stdin
        stdout = child.stdout
        assert stdin is not None and stdout is not None

        def probe() -> str:
            stdin.write("\n")
            stdin.flush()
            return stdout.readline().strip()

        try:
            ready = stdout.readline().strip()
            if ready != "ready":
                raise RuntimeError(f"unexpected child output: {ready!r}")
            yield probe
        finally:
            stdin.close()
            child.wait(timeout=10)


@testcase.eden_repo_test(run_on_nfs=False)
class ActiveFuseInvalidationTest(testcase.EdenRepoTest):
    """Test that with pressure-based GC enabled, the active FUSE invalidation
    path in handleChildrenNotAccessedRecently sends FUSE_NOTIFY_INVAL_ENTRY
    for stale inodes, causing the kernel to FORGET them so they can be
    unloaded.

    Without pressure-based GC, the FUSE path doesn't invalidate anything
    (it relies on the kernel dropping references naturally). With it enabled,
    active invalidation means GC can actually reclaim inodes on Linux/FUSE.
    """

    directories: List[str] = ["a", "b", "c"]
    num_files: int = 10
    deep_file_count: int = 20

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("experimental", []).append("enable-pressure-based-gc = true")
        return result

    def edenfs_logging_settings(self) -> Dict[str, str]:
        return {
            "eden.fs.inodes.TreeInode": "DBG5",
        }

    def populate_repo(self) -> None:
        for directory in self.directories:
            for i in range(self.num_files):
                self.repo.write_file(f"{directory}/{i}", f"{i}\n")
        for i in range(self.deep_file_count):
            self.repo.write_file(f"deep/parent/child/{i}", f"{i}\n")
        self.repo.commit("Initial commit.")

    async def get_loaded_count(self) -> int:
        async with self.get_async_thrift_client() as client:
            stats = await client.getStatInfo(
                GetStatInfoParams(statsMask=STATS_MOUNTS_STATS)
            )
        mountPointInfo = stats.mountPointInfo
        if mountPointInfo is None:
            raise Exception("stats.mountPointInfo is not set")
        self.assertEqual(len(mountPointInfo), 1)
        for mountPath in mountPointInfo:
            info = mountPointInfo[mountPath]
            return info.loadedFileCount + info.loadedTreeCount
        return 0  # Appease pyre

    async def get_loaded_tree_count(self) -> int:
        async with self.get_async_thrift_client() as client:
            stats = await client.getStatInfo(
                GetStatInfoParams(statsMask=STATS_MOUNTS_STATS)
            )
        mountPointInfo = stats.mountPointInfo
        if mountPointInfo is None:
            raise Exception("stats.mountPointInfo is not set")
        self.assertEqual(len(mountPointInfo), 1)
        for mountPath in mountPointInfo:
            return mountPointInfo[mountPath].loadedTreeCount
        return 0  # Appease pyre

    def read_all(self) -> None:
        for directory in self.directories:
            for i in range(self.num_files):
                content = self.read_file(f"{directory}/{i}")
                self.assertEqual(content, f"{i}\n")

    async def invalidate(self, path: str, seconds: int = 0) -> int:
        async with self.get_async_thrift_client() as client:
            result = await client.debugInvalidateNonMaterialized(
                DebugInvalidateRequest(
                    mount=MountId(mountPoint=self.mount_path_bytes),
                    path=os.fsencode(path),
                    age=TimeSpec(seconds=seconds, nanoSeconds=0),
                )
            )
            return result.numInvalidated

    async def test_active_invalidation_unloads_inodes(self) -> None:
        """With pressure-based GC, debugInvalidateNonMaterialized triggers
        active FUSE invalidation which causes the kernel to FORGET inodes,
        allowing them to be unloaded."""
        self.read_all()
        loaded_after_read = await self.get_loaded_count()
        # 30 files + 3 directories + root = at least 34
        self.assertGreaterEqual(loaded_after_read, 34)

        # Wait so inodes are "old"
        time.sleep(3)

        # Trigger GC via debugInvalidateNonMaterialized.
        # With pressure-based GC enabled, this goes through
        # invalidateChildrenNotAccessedRecentlyFuse which sends
        # FUSE_NOTIFY_INVAL_ENTRY, then unloadChildrenUnreferencedByFs.
        invalidated = await self.invalidate("")

        loaded_after = await self.get_loaded_count()
        if sys.platform == "linux":
            # On Linux with active FUSE invalidation, inodes should
            # actually get unloaded (unlike the legacy path which can't
            # invalidate on FUSE).
            self.assertGreater(invalidated, 0)
            # Pressure GC should invalidate stale entries individually instead
            # of relying on one parent directory invalidation to reclaim an
            # entire subtree.
            self.assertGreaterEqual(invalidated, len(self.directories) * self.num_files)
            self.assertLess(loaded_after, loaded_after_read)
        elif sys.platform == "darwin":
            self.assertLess(loaded_after, loaded_after_read)

        # Files should still be readable
        self.read_all()

    async def test_active_invalidation_respects_age(self) -> None:
        """Active invalidation should only affect inodes older than the
        specified age."""
        # Read directory "a" first
        for i in range(self.num_files):
            self.read_file(f"a/{i}")

        time.sleep(3)

        # Read directory "b" now (so "a" is old, "b" is fresh)
        for i in range(self.num_files):
            self.read_file(f"b/{i}")

        loaded_before = await self.get_loaded_count()

        # Invalidate with 2s age: "a" is stale and "b" is fresh, so GC should
        # invalidate the stale entries under "a" individually.
        invalidated = await self.invalidate("", seconds=2)
        if sys.platform == "linux":
            self.assertGreaterEqual(invalidated, self.num_files)

        loaded_after = await self.get_loaded_count()
        # Some inodes from "a" should have been unloaded
        self.assertLess(loaded_after, loaded_before)

        # Everything should still be readable
        self.read_all()

    async def test_active_invalidation_reclaims_siblings_of_open_file(self) -> None:
        if sys.platform != "linux":
            self.skipTest("active FUSE invalidation is Linux-only")

        for i in range(self.deep_file_count):
            self.assertEqual(f"{i}\n", self.read_file(f"deep/parent/child/{i}"))

        open_path = os.path.join(self.mount, "deep/parent/child/0")
        with open(open_path) as open_file:
            self.assertEqual("0\n", open_file.readline())
            loaded_after_read = await self.get_loaded_count()
            self.assertGreaterEqual(
                loaded_after_read,
                self.deep_file_count + 4,
            )

            time.sleep(3)

            invalidated = await self.invalidate("")
            self.assertGreaterEqual(invalidated, self.deep_file_count)

            loaded_after_gc = await self.get_loaded_count()
            self.assertLess(loaded_after_gc, loaded_after_read)

        self.assertEqual("0\n", self.read_file("deep/parent/child/0"))

    async def test_active_invalidation_preserves_getcwd_of_running_process(
        self,
    ) -> None:
        if sys.platform != "linux":
            self.skipTest("active FUSE invalidation is Linux-only")

        for i in range(self.deep_file_count):
            self.assertEqual(f"{i}\n", self.read_file(f"deep/parent/child/{i}"))

        cwd_path = os.path.join(self.mount, "deep", "parent", "child")
        with pinned_cwd_child(cwd_path) as probe:
            self.assertEqual(f"cwd:{os.path.realpath(cwd_path)}", probe())

            invalidated = await self.invalidate("")
            self.assertGreater(invalidated, 0)

            # GC discovers pinned working directories via the privhelper
            # scan-pins mode and skips invalidating the pinned chain (or, if
            # pin information is unavailable, skips all directories), so
            # getcwd() keeps working.
            self.assertEqual(f"cwd:{os.path.realpath(cwd_path)}", probe())

    async def test_active_invalidation_reclaims_unpinned_directories(self) -> None:
        """With pin information available, GC invalidates directory entries
        outside the pinned chain, so unpinned directories are forgotten by the
        kernel and unloaded while the pinned cwd keeps working."""
        if sys.platform != "linux":
            self.skipTest("active FUSE invalidation is Linux-only")
        if not privhelper_supports_scan_pins():
            self.skipTest("privhelper does not support --scan-pins")

        self.read_all()
        for i in range(self.deep_file_count):
            self.assertEqual(f"{i}\n", self.read_file(f"deep/parent/child/{i}"))

        trees_before = await self.get_loaded_tree_count()
        # At least root, a, b, c, deep, deep/parent, deep/parent/child.
        self.assertGreaterEqual(trees_before, 7)

        cwd_path = os.path.join(self.mount, "deep", "parent", "child")
        with pinned_cwd_child(cwd_path) as probe:
            self.assertEqual(f"cwd:{os.path.realpath(cwd_path)}", probe())

            # The kernel sends FORGET replies to entry invalidations
            # asynchronously, so poll: each invalidate() also re-runs the
            # unload sweep that reaps newly-forgotten inodes.
            deadline = time.monotonic() + 10
            while True:
                await self.invalidate("")
                trees_after = await self.get_loaded_tree_count()
                if trees_after <= trees_before - len(self.directories):
                    break
                if time.monotonic() >= deadline:
                    self.fail(
                        "unpinned directories were not reclaimed: "
                        f"{trees_before} trees loaded before GC, "
                        f"{trees_after} after"
                    )
                await asyncio.sleep(0.1)

            self.assertEqual(f"cwd:{os.path.realpath(cwd_path)}", probe())

    async def test_active_invalidation_preserves_bind_redirection(self) -> None:
        if sys.platform != "linux":
            self.skipTest("active FUSE invalidation is Linux-only")

        repo_path = "a/generated-output"
        self.eden.run_cmd("redirect", "add", "--mount", self.mount, repo_path, "bind")

        redirection_path = os.path.join(self.mount, repo_path)
        mount_stat = os.stat(self.mount)

        def assert_bind_mounted() -> None:
            self.assertNotEqual(mount_stat.st_dev, os.stat(redirection_path).st_dev)

        def load_gc_candidate() -> None:
            self.assertEqual("0\n", self.read_file("a/0"))

        async def invalidate_until_gc_runs() -> None:
            deadline = time.monotonic() + 5
            while True:
                invalidated = await self.invalidate("a")
                if invalidated > 0:
                    return
                if time.monotonic() >= deadline:
                    self.fail("pressure GC did not invalidate the bind redirection")
                await asyncio.sleep(0.1)

        assert_bind_mounted()
        load_gc_candidate()
        await invalidate_until_gc_runs()
        # This is the test: redirection is still on a separate device.
        self.assertNotEqual(mount_stat.st_dev, os.stat(redirection_path).st_dev)

        self.eden.run_cmd("redirect", "fixup", "--mount", self.mount)
        assert_bind_mounted()

        self.eden.graceful_restart()
        assert_bind_mounted()

        load_gc_candidate()
        await invalidate_until_gc_runs()
        assert_bind_mounted()


@testcase.eden_repo_test(run_on_nfs=False)
class PressureGcWithoutPinScanTest(testcase.EdenRepoTest):
    """With mount:pressure-gc-scan-pins disabled, pressure GC has no pin
    information and must skip invalidating directory entries entirely, so
    pinned working directories keep functioning while file entries are still
    invalidated and reclaimed."""

    file_count: int = 20

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        result = super().edenfs_extra_config() or {}
        result.setdefault("experimental", []).append("enable-pressure-based-gc = true")
        result.setdefault("mount", []).append("pressure-gc-scan-pins = false")
        return result

    def populate_repo(self) -> None:
        for i in range(self.file_count):
            self.repo.write_file(f"deep/parent/child/{i}", f"{i}\n")
        self.repo.commit("Initial commit.")

    async def test_skips_directory_invalidation_without_pin_info(self) -> None:
        if sys.platform != "linux":
            self.skipTest("active FUSE invalidation is Linux-only")

        for i in range(self.file_count):
            self.assertEqual(f"{i}\n", self.read_file(f"deep/parent/child/{i}"))

        cwd_path = os.path.join(self.mount, "deep", "parent", "child")
        with pinned_cwd_child(cwd_path) as probe:
            self.assertEqual(f"cwd:{os.path.realpath(cwd_path)}", probe())

            async with self.get_async_thrift_client() as client:
                result = await client.debugInvalidateNonMaterialized(
                    DebugInvalidateRequest(
                        mount=MountId(mountPoint=self.mount_path_bytes),
                        path=b"",
                        age=TimeSpec(seconds=0, nanoSeconds=0),
                    )
                )
            # File entries are still invalidated even though directories are
            # skipped.
            self.assertGreaterEqual(result.numInvalidated, self.file_count)

            self.assertEqual(f"cwd:{os.path.realpath(cwd_path)}", probe())
