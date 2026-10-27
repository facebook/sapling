# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The attributes property: a getattr's type, executable bit, size, and mtime order
match the path's cells. Complements the corpus family `attributes`: these tests pin
when a comparison is an obligation at all (writes between mtimes, rebindings,
sizes racing writers leave open, fault targets, mkdir-made paths) and that the mode rule names no
producer.
"""

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge_isolated,
    status_of,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import getattr_facts
from eden.integration.correctness.exploratory.test.corpus.author import (
    NEW_DIR,
    R0,
    Run,
    thrift_checkout,
    WRITE_OK,
)


class AttributesTest(unittest.TestCase):
    def test_mtime_pair_with_a_write_between_is_no_obligation(self) -> None:
        run = Run("t", "mtime-write", "a write between two getattrs")
        run.act("a", "g", "getattr", path=R0)
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "h", "getattr", path=R0)
        run.call("g", "ok", **getattr_facts(mtime_ns=200))
        run.call("w", "ok", **WRITE_OK)
        run.call("h", "ok", **getattr_facts(mtime_ns=100))
        run.finish()
        verdict = judge_isolated(("attributes",), run)
        self.assertIsNone(status_of(verdict, "attributes.mtime-decreased", "h"))
        self.assertEqual("accept", str(verdict.kind))

    def test_types_overlapping_a_rebinding_are_not_compared(self) -> None:
        run = Run("t", "type-overlap", "a getattr inside an unlink")
        run.act("a", "u", "unlink", path=R0)
        run.act("b", "g", "getattr", path=R0)
        run.inv("u")
        run.call("g", "ok", **getattr_facts(thrift_type="directory"))
        run.done("u", "ok", path=R0)
        run.finish()
        verdict = judge_isolated(("attributes",), run)
        self.assertIsNone(status_of(verdict, "attributes.type-disagreement", "g"))

    def test_a_thrift_type_overlapping_a_write_is_an_engine_observation(
        self,
    ) -> None:
        """Not compared with FUSE while a write changes r0, the Thrift type is
        still a binding of the path at a point in the window: r0 stays a file."""
        found = {}
        for thrift_type in ("file", "directory"):
            run = Run("t", f"thrift-type-{thrift_type}", "a getattr inside a write")
            run.act("a", "w", "write", path=R0, value_id="w")
            run.act("b", "g", "getattr", path=R0)
            run.inv("w")
            run.call("g", "ok", **getattr_facts(thrift_type=thrift_type))
            run.done("w", "ok", **WRITE_OK)
            run.finish()
            verdict = judge_isolated(("attributes",), run)
            self.assertIsNone(status_of(verdict, "attributes.type-disagreement", "g"))
            found[thrift_type] = status_of(
                verdict, "attributes.type-mismatch", "g:thrift"
            )
        self.assertEqual({"file": "satisfied", "directory": "violated"}, found)

    def test_mode_identity_names_no_producer(self) -> None:
        run = Run("t", "mode-checkout", "a getattr after a checkout")
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "g", "getattr", path=R0)
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
        run.call("g", "ok", **getattr_facts(mode=0o755))
        run.finish()
        verdict = judge_isolated(("attributes",), run)
        assert verdict.report is not None
        (item,) = [
            item
            for result in verdict.report.results
            for item in result.obligations
            if item.rule_id == "attributes.mode-revert"
        ]
        self.assertEqual("violated", str(item.status))
        self.assertEqual([], [w.role for w in item.witnesses])

    def test_a_mkdir_made_path_has_no_type_mismatch_obligation(self) -> None:
        run = Run("t", "mkdir-type", "mkdir d, then a getattr of d")
        run.act("a", "m", "mkdir", path=NEW_DIR)
        run.act("a", "g", "getattr", path=NEW_DIR, requires=[("m", "finished")])
        run.call("m", "ok", path=NEW_DIR)
        run.call(
            "g", "ok", **getattr_facts(fuse_type="directory", thrift_type="directory")
        )
        run.finish()
        verdict = judge_isolated(("attributes", "existence"), run)
        self.assertIsNone(status_of(verdict, "attributes.type-mismatch", "g"))
        self.assertEqual("satisfied", status_of(verdict, "existence.mkdir-type", "g"))

    def test_types_of_a_fault_target_are_not_compared(self) -> None:
        """A fault targeting the getattr may make one channel's answer differ."""
        run = Run("t", "type-fault", "a getattr a fault targets")
        run.act("n", "u", "unload_inodes", path="tree", age_seconds=0)
        run.act(
            "n",
            "f",
            "fault_install",
            fault_id="F",
            site="inodemap-load",
            target_action="g",
            behavior="error",
        )
        run.act("a", "g", "getattr", path=R0, requires=[("f", "finished")])
        run.act("n", "x", "fault_remove", fault_id="F", requires=[("g", "finished")])
        run.call("u", "ok", path="tree", prepared=0, num_invalidated=1)
        run.call("f", "ok", fault_id="F")
        run.call("g", "ok", **getattr_facts(thrift_type="directory"))
        run.call("x", "ok", fault_id="F", removed=1)
        run.fault_removed("F")
        run.finish()
        verdict = judge_isolated(("attributes",), run)
        self.assertIsNone(status_of(verdict, "attributes.type-disagreement", "g"))
        twin = Run("t", "type-no-fault", "the same getattr without the fault")
        twin.act("a", "g", "getattr", path=R0)
        twin.call("g", "ok", **getattr_facts(thrift_type="directory"))
        twin.finish()
        self.assertEqual(
            "violated",
            status_of(
                judge_isolated(("attributes",), twin),
                "attributes.type-disagreement",
                "g",
            ),
        )
