# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The bytes property: a successful read returns a whole value some producer of the run
can write, visible at some point inside the read. Complements the corpus family
`direct`: these tests pin the splice witness naming both candidate values, moved
values as candidates of their destination, the recorder finding for an inconsistent
digest, junk of another width, and FUSE and Thrift reads that agree against the
effects sharing one serving group.
"""

from __future__ import annotations

import hashlib
import unittest

from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts.model import EdenPinned
from eden.integration.correctness.exploratory.edenfs.properties import (
    bytes as bytes_property,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    obligations_of,
    violations_of,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    MOVED,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    TIP,
    value,
    WRITE_OK,
)


class SpliceTest(unittest.TestCase):
    def test_splice_needs_two_candidates_covering_every_byte(self) -> None:
        a, b = b"AAAAAAAA", b"BBBBBBBB"
        candidates = {"a": a, "b": b, "c": b"CCCC"}
        self.assertEqual(("a", "b"), bytes_property.splice(b"AAAABBBB", candidates))
        self.assertEqual(("a", "b"), bytes_property.splice(b"BABABABA", candidates))
        self.assertIsNone(bytes_property.splice(b"AAAABBBC", candidates))
        self.assertIsNone(bytes_property.splice(b"AAAA", candidates))
        self.assertIsNone(bytes_property.splice(b"AAAABBBB", {"a": a}))

    def test_moved_values_are_candidates_of_the_destination(self) -> None:
        run = Run("t", "moved", "a write then a rename")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "m", "rename", source=R0, destination=R1)
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        candidates = bytes_property.Candidates(spec, EdenPinned.for_spec(spec))
        written = value("a", "w")
        self.assertIn(written, candidates.of_path(R1).values())
        self.assertIn(committed(R0, TIP), candidates.of_path(R1).values())
        self.assertNotIn(committed(R1, TIP), candidates.of_path(R0).values())

    def test_a_moved_directory_carries_its_files_candidates(self) -> None:
        """r2 is a directory at the tip; renaming it moves its child's values."""
        run = Run(
            "t",
            "moved-dir",
            "rename a type-changed directory",
            type_changes=[{"path": R2, "at_revision": TIP}],
        )
        child = run.layout().type_change_child(R2).path
        run.act("a", "m", "rename", source=R2, destination=MOVED)
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        pinned = EdenPinned.for_spec(spec)
        candidates = bytes_property.Candidates(spec, pinned)
        moved_child = MOVED + child[len(R2) :]
        data = pinned.fixture.committed(child, TIP)
        assert data is not None
        self.assertIn(data.data, candidates.of_path(moved_child).values())
        self.assertNotIn(data.data, candidates.of_path(R0).values())


class BytesTest(unittest.TestCase):
    def test_an_inconsistent_digest_is_a_recorder_finding(self) -> None:
        """A recorded sha256 that is not its own bytes' digest is
        the recorder's (`core.result-schema`), never `bytes.torn-read`."""
        run = Run("t", "bad-digest", "a read whose sha256 is not its bytes'")
        run.act("a", "r", "read", path=R0, channel="fuse")
        run.call("r", "ok", **{**read_ok(committed(R0, TIP)), "sha256": "ab" * 32})
        run.finish()
        self.assertEqual(
            {("core.result-schema", "r"): "harness"},
            violations_of(judge_isolated(("bytes", "core.result-schema"), run)),
        )

    def test_a_splice_names_both_candidates(self) -> None:
        run = Run("t", "splice", "a write, then half of it over rev3's bytes")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "r", "read", path=R0, channel="fuse", requires=[("w", "finished")])
        run.call("w", "ok", **WRITE_OK)
        run.call("r", "ok", **read_ok(value("a", "w")[:32] + committed(R0, TIP)[32:]))
        run.finish()
        torn = obligations_of(judge_isolated(("bytes",), run))[("bytes.torn-read", "r")]
        self.assertEqual(ObligationStatus.VIOLATED, torn.status)
        spliced = {w.value for w in torn.witnesses if w.role == "spliced"}
        self.assertEqual(
            {
                hashlib.sha256(value("a", "w")).hexdigest(),
                hashlib.sha256(committed(R0, TIP)).hexdigest(),
            },
            spliced,
        )

    def test_fuse_and_thrift_agreeing_against_effects_share_one_serving_group(
        self,
    ) -> None:
        """FUSE and Thrift read one inode and overlay layer: both returning the
        overwritten bytes corroborate a lost write, not a model error."""
        run = Run("t", "two-paths", "FUSE and Thrift both return the old bytes")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "f", "read", path=R0, channel="fuse", requires=[("w", "finished")])
        run.act(
            "a",
            "t",
            "read",
            path=R0,
            channel="thrift",
            sync_timeout_seconds=60,
            requires=[("f", "finished")],
        )
        run.call("w", "ok", **WRITE_OK)
        run.call("f", "ok", **read_ok(committed(R0, TIP)))
        run.call("t", "ok", **read_ok(committed(R0, TIP)))
        run.finish()
        verdict = judge(run)
        self.assertEqual(
            {
                ("bytes.stale-read", "f"): "undetermined",
                ("bytes.stale-read", "t"): "undetermined",
            },
            violations_of(verdict),
        )
        self.assertEqual("reject", str(verdict.kind))
        assert verdict.report is not None
        self.assertGreater(
            verdict.report.work.phases["explanation"]["attribution_trials"], 0
        )
