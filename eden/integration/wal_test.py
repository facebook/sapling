#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

"""Integration tests for the EdenFS overlay Write-Ahead Log (WAL).

These tests exercise crash-recovery, fsck, and concurrency scenarios that
unit tests cannot cover end-to-end. WAL is enabled per-class via the
edenfs_extra_config override below — not as a global default — so other
integration tests keep their non-WAL coverage. The tests depend on the
Legacy / LegacyDev inode catalog; on platforms that don't use that
catalog the tests skip.
"""

from __future__ import annotations

import logging
import os
import pathlib
import struct
import subprocess
import sys
import threading
import time
import unittest
from typing import Dict, List, Optional, Tuple

from eden.fs.service.eden.thrift_types import MountState
from eden.integration.lib import overlay as overlay_mod, testcase

logger: logging.Logger = logging.getLogger(__name__)

# Mirror enum class WalOpType in eden/fs/inodes/fscatalog/FsInodeCatalog.h.
_WAL_ADD: int = 1


def _wal_path_for(overlay_dir: pathlib.Path, inode: int) -> pathlib.Path:
    """Return the WAL file path for a directory inode.

    Mirrors FsFileContentStore::getWalPath: the WAL lives next to the
    overlay file in the same shard directory, with a `.wal` suffix.
    """
    shard = f"{inode % 256:02x}"
    return overlay_dir / shard / f"{inode}.wal"


def _encode_wal_add(name: str, inode: int, mode: int = 0o100644) -> bytes:
    """Encode a single ADD WAL entry with no source-control hash.

    Wire format (little-endian native, see appendWalEntry in
    FsInodeCatalog.cpp):
        [uint32 entryLen]
        [uint8  op=ADD]
        [uint16 nameLen]
        [bytes  name]
        [int32  mode]
        [int64  inodeNumber]
        [uint8  hashLen=0]
    entryLen covers everything after the entryLen field itself.
    """
    name_bytes = name.encode("utf-8")
    payload = struct.pack(
        f"<BH{len(name_bytes)}siqB",
        _WAL_ADD,
        len(name_bytes),
        name_bytes,
        mode,
        inode,
        0,  # hashLen
    )
    return struct.pack("<I", len(payload)) + payload


# Decorator order matters: @testcase.eden_nfs_repo_test replaces WalTest
# with subclasses registered directly into module scope, so any decorator
# above it is applied to a class that no longer exists. Apply
# @unittest.skipIf BELOW @testcase.eden_nfs_repo_test so WalTest itself
# carries the skip attribute and the generated subclasses (WalTestDefault,
# WalTestNFS, ...) inherit it via MRO.
@testcase.eden_nfs_repo_test
@unittest.skipIf(
    sys.platform == "win32",
    "WAL is only implemented for the Legacy/LegacyDev FsInodeCatalog "
    "(Linux/macOS). Windows uses the Sqlite catalog which has its own "
    "semantic addChild/removeChild path and does not produce .wal files.",
)
class WalTest(testcase.HgRepoTestMixin, testcase.EdenRepoTest):
    """End-to-end WAL recovery tests.

    Each test materializes a directory in the overlay, exercises a
    restart or fsck scenario, and asserts that the user-visible state
    converges as documented in the WAL design.

    Tests use clean restarts, not SIGKILL: shutdown drains in-flight
    writes, so every returned write must survive replay. Mid-write
    crash semantics are covered by test_wal_torn_write_recovery
    (synthetic torn WAL) and by C++ fault-injection unit tests.

    Rename crash semantics are covered by WalRenameTest in
    eden/fs/inodes/test/OverlayTest.cpp (the cross-dir both-visible
    state is not expressible through FUSE/NFS).
    """

    # pyre-fixme[13]: Attribute `overlay` is never initialized.
    overlay: overlay_mod.OverlayStore

    def edenfs_extra_config(self) -> Optional[Dict[str, List[str]]]:
        # WAL is opt-in per test class: enabling it here (rather than in
        # the base testcase.py) keeps non-WAL coverage intact for every
        # other integration test.
        configs = super().edenfs_extra_config() or {}
        configs.setdefault("overlay", []).append("use-wal = true")
        return configs

    def populate_repo(self) -> None:
        self.repo.write_file("README.md", "wal-test\n")
        self.repo.commit("Initial commit.")

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        self.overlay = overlay_mod.OverlayStore(self.eden, self.mount_path)

    def _get_inode(self, path: pathlib.Path) -> int:
        return os.lstat(path).st_ino

    def _wait_for_mount_running(self, timeout_seconds: float = 30.0) -> None:
        """Block until self.mount_path is in MountState.RUNNING.

        Polling EdenFS's authoritative state machine via thrift —
        rather than probing the kernel filesystem with iterdir — gives
        us a deterministic readiness signal. After eden.restart() the
        daemon's thrift socket is up immediately but the kernel mount
        (FUSE on Linux, NFS on macOS) takes a few hundred milliseconds
        more to be re-established. Waiting on MountState.RUNNING
        eliminates that race without filesystem retries.
        """
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            state = self.eden.get_mount_state(self.mount_path)
            if state == MountState.RUNNING:
                return
            time.sleep(0.02)
        raise TimeoutError(
            f"mount {self.mount_path!s} did not reach RUNNING within "
            f"{timeout_seconds}s (last state: "
            f"{self.eden.get_mount_state(self.mount_path)})"
        )

    def _materialize_subdir(self, name: str) -> Tuple[pathlib.Path, int]:
        """Create a directory under the mount and force it into the
        overlay. Returns (path, inode_number).

        Calling materialize_dir engages the first-materialization branch
        in TreeInode::childMaterialized which uses saveOverlayDir to
        create the base file. Subsequent mutations go through the WAL
        fast path.
        """
        path = self.mount_path / name
        path.mkdir()
        self.overlay.materialize_dir(path)
        return (path, self._get_inode(path))

    def test_wal_replay_after_restart(self) -> None:
        """Files added after materialization survive a clean restart.

        Each addChild appends a WAL entry instead of rewriting the
        whole directory. We do not compact on shutdown (lazy replay
        is faster), so the WAL stays on disk across the restart and
        loadOverlayDir on remount replays it into the base file.
        """
        test_dir, _ = self._materialize_subdir("restart_test")
        # Spread enough writes to keep us below the inline-compaction
        # threshold so the WAL still has unmerged entries at restart
        # time (otherwise we test only the no-WAL fast path).
        # Threshold for an empty dir is 30 (3 * max(0, 10)), so 25
        # keeps everything in the WAL.
        file_count = 25
        for i in range(file_count):
            (test_dir / f"file_{i:03d}").write_text(f"content_{i}")

        self.eden.restart()
        self._wait_for_mount_running()

        listed = sorted(p.name for p in test_dir.iterdir())
        expected = sorted(f"file_{i:03d}" for i in range(file_count))
        self.assertEqual(expected, listed)
        self.assertEqual("content_0", (test_dir / "file_000").read_text())
        self.assertEqual(
            f"content_{file_count - 1}",
            (test_dir / f"file_{file_count - 1:03d}").read_text(),
        )

    def test_wal_torn_write_recovery(self) -> None:
        """A torn entry at the end of the WAL is silently discarded;
        well-formed entries before it are merged on next mount.

        Pattern: shutdown leaves the WAL on disk by design (no compact-
        on-shutdown). We append junk bytes that look like the start of
        an oversized entry; replayWal stops at the torn tail but keeps
        the good prefix.
        """
        test_dir, dir_inode = self._materialize_subdir("torn_test")
        for i in range(5):
            (test_dir / f"good_{i}").write_text("ok")

        # Shutdown gracefully so the daemon's in-memory state is flushed
        # to disk but the WAL is left intact (no compact-on-shutdown).
        self.eden.shutdown()

        wal_path = _wal_path_for(self.overlay.overlay_dir, dir_inode)
        self.assertTrue(wal_path.exists(), f"expected WAL on disk at {wal_path}")
        # Append an entryLen of 1MB with no payload — a clear torn write.
        with wal_path.open("ab") as f:
            f.write(struct.pack("<I", 1_000_000))

        self.eden.start()
        self._wait_for_mount_running()

        listed = sorted(p.name for p in test_dir.iterdir())
        expected = sorted(f"good_{i}" for i in range(5))
        self.assertEqual(expected, listed, "torn tail must not drop good prefix")

    def test_fsck_replays_orphan_wal(self) -> None:
        """`eden fsck` replays WAL entries before the orphan scan so
        file inodes referenced only by the WAL are not falsely deleted.
        """
        test_dir, dir_inode = self._materialize_subdir("fsck_test")

        # Shutdown so we can author the WAL by hand.
        self.eden.shutdown()

        # Pick an inode number outside any range edenfs has handed out
        # by inflating it well above next-inode-number. fsck only
        # cares that the WAL deserializes; the inode number need not
        # back a real file for the merge to apply.
        synthetic_inode = 1 << 40
        wal_path = _wal_path_for(self.overlay.overlay_dir, dir_inode)
        wal_path.parent.mkdir(parents=True, exist_ok=True)
        with wal_path.open("ab") as f:
            f.write(_encode_wal_add("ghost.txt", synthetic_inode))

        # Force fsck (mount is not currently mounted, so no --force).
        result = self.eden.run_unchecked(
            "fsck",
            self.mount,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        fsck_out = result.stdout.decode("utf-8", errors="replace")
        logger.info("fsck output:\n%s", fsck_out)

        # The pre-pass must have either replayed the WAL into the base
        # dir or removed the WAL after merge — what it must NOT do is
        # leave a dangling .wal file behind for fsck to flag.
        self.assertFalse(
            wal_path.exists(),
            f"orphan WAL still on disk after fsck: {wal_path}",
        )

    def test_wal_concurrent_writers(self) -> None:
        """Multiple threads mutate the same directory concurrently.
        After a clean restart, every successfully-returned write
        survives WAL replay. Validates the WAL replay merge under
        concurrent producer load.
        """
        test_dir, _ = self._materialize_subdir("concurrent_test")

        thread_count = 4
        files_per_thread = 10
        succeeded: list[str] = []
        succeeded_lock = threading.Lock()

        def writer(thread_id: int) -> None:
            for i in range(files_per_thread):
                name = f"t{thread_id}_f{i:02d}"
                try:
                    (test_dir / name).write_text(f"{thread_id}-{i}")
                except OSError as e:
                    logger.warning("write %s failed: %s", name, e)
                    continue
                with succeeded_lock:
                    succeeded.append(name)

        threads = [
            threading.Thread(target=writer, args=(t,)) for t in range(thread_count)
        ]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        self.eden.restart()
        self._wait_for_mount_running()

        listed = {p.name for p in test_dir.iterdir()}
        missing = set(succeeded) - listed
        self.assertEqual(
            set(),
            missing,
            f"WAL replay lost {len(missing)} successfully-written entries: "
            f"{sorted(missing)[:10]}{'...' if len(missing) > 10 else ''}",
        )
