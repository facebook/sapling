# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The cross-channel property: a FUSE read and a Thrift read of one path over a span no
producer can touch return equal bytes. Complements the corpus family `cross-channel`:
these tests pin comparability from the shared effect windows, the spanning tree over
comparable reads (rooted at the most common value, equivalent to every pair), the
single violated scope of one odd read, and the bound linear in the reads.
"""

from __future__ import annotations

import itertools
import random
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.effects import EffectSet
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    MAX_RUN_OBLIGATIONS,
)
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.properties import cross_channel
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge_isolated,
    obligations_by_scope,
    REGISTERED_RULE_IDS,
    statuses_by_scope,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    R0,
    read_ok,
    Run,
    thrift_checkout,
    TIP,
    value,
    WRITE_OK,
)


CROSS = cross_channel.BYTE_DISAGREEMENT


ECONNABORTED = 103


def _cross_run(
    *,
    thrift: bytes | None,
    writer: tuple[str, str, str] | None = None,
    fuse: bytes | None = None,
) -> Run:
    """w1 acked, FUSE read f, Thrift read t, plus an optional writer `wx` of r0.

    `writer` = (when, outcome, completion): `when` is before-f (invoked
    before f, completing as `completion` says), between, or after.
    """
    run = Run("t", "cross", "a FUSE and a Thrift read of r0")
    run.act("a", "w1", "write", path=R0, value_id="v1")
    run.act("a", "f", "read", path=R0, channel="fuse")
    run.act("a", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0)
    if writer is not None:
        run.act("c", "wx", "write", path=R0, value_id="vx")
    when, outcome, completion = writer or ("", "", "")
    facts = (
        dict(WRITE_OK)
        if outcome == "ok"
        else (
            {"reason": "aborted", "errno": ECONNABORTED}
            if outcome == "fail"
            else {"reason": "operation-timeout"}
        )
    )
    run.call("w1", "ok", **WRITE_OK)
    if when == "before-f":
        run.inv("wx")
        if completion == "before-f":
            run.done("wx", outcome, **facts)
    run.call("f", "ok", **read_ok(fuse or value("a", "v1")))
    if when == "before-f" and completion == "after-f":
        run.done("wx", outcome, **facts)
    if when == "between":
        run.call("wx", outcome, **facts)
    if thrift is None:
        run.call("t", "fail", reason="error", source="thrift")
    else:
        run.call("t", "ok", **read_ok(thrift))
    if when == "after":
        run.call("wx", outcome, **facts)
    run.finish()
    return run


class CrossChannelTest(unittest.TestCase):
    def test_reads_have_stable_and_separated_twins(self) -> None:
        """The old twins: a writer that can run between the reads separates them."""
        v1, vx, other = value("a", "v1"), value("c", "vx"), committed(R0, TIP)
        cases = (
            ("different", _cross_run(thrift=other), "violated"),
            ("equal", _cross_run(thrift=v1), "satisfied"),
            (
                "fuse-overlap",
                _cross_run(thrift=vx, writer=("before-f", "ok", "after-f")),
                None,
            ),
            ("gap", _cross_run(thrift=vx, writer=("between", "ok", "")), None),
            ("boundary", _cross_run(thrift=vx, writer=("between", "fail", "")), None),
            (
                "earlier-info",
                _cross_run(thrift=vx, writer=("before-f", "info", "before-f")),
                None,
            ),
            ("failed-read", _cross_run(thrift=None), None),
            (
                "later-write",
                _cross_run(thrift=other, writer=("after", "ok", "")),
                "violated",
            ),
            (
                "earlier-finished",
                _cross_run(
                    thrift=other, fuse=vx, writer=("before-f", "ok", "before-f")
                ),
                "violated",
            ),
        )
        for name, run, expected in cases:
            with self.subTest(case=name):
                verdict = judge_isolated(("cross-channel",), run)
                found = statuses_by_scope(verdict, CROSS)
                self.assertEqual({} if expected is None else {"t": expected}, found)
                if expected == "violated":
                    item = obligations_by_scope(verdict, CROSS)["t"]
                    self.assertEqual(Attribution.PRODUCT, item.attribution)
                    self.assertTrue(
                        (item.reason or "").startswith("bytes/fuse≠thrift:")
                    )

    def test_an_overlapping_checkout_separates_the_reads(self) -> None:
        for finished, expected in ((False, {}), (True, {"t": "violated"})):
            with self.subTest(finished=finished):
                run = Run("t", "checkout", "a force checkout and two reads of r0")
                run.act("a", "f", "read", path=R0, channel="fuse")
                run.act(
                    "a", "t", "read", path=R0, channel="thrift", sync_timeout_seconds=0
                )
                run.act("c", "c", "checkout", revision=1, via="thrift", mode="force")
                done = thrift_checkout(1, 0)
                if finished:
                    run.call("c", "ok", **done)
                    run.call("f", "ok", **read_ok(committed(R0, 1)))
                    run.call("t", "ok", **read_ok(committed(R0, TIP)))
                else:
                    run.inv("c")
                    run.call("f", "ok", **read_ok(committed(R0, TIP)))
                    run.call("t", "ok", **read_ok(committed(R0, 1)))
                    run.done("c", "ok", **done)
                run.finish()
                self.assertEqual(
                    expected,
                    statuses_by_scope(judge_isolated(("cross-channel",), run), CROSS),
                )

    def test_many_comparable_reads_own_one_obligation_per_tree_edge(self) -> None:
        """150 x 150 pairs exceed the run bound; the tree has 299 edges."""
        run = Run("t", "many-reads", "one write, then 300 alternating reads")
        run.act("a", "w", "write", path=R0, value_id="w")
        ids = []
        for index in range(300):
            channel = "fuse" if index % 2 == 0 else "thrift"
            extra: dict[str, Any] = (
                {} if channel == "fuse" else {"sync_timeout_seconds": 0}
            )
            ids.append(
                run.act("a", f"r{index}", "read", path=R0, channel=channel, **extra)
            )
        run.call("w", "ok", **WRITE_OK)
        for read_id in ids:
            run.call(read_id, "ok", **read_ok(value("a", "w")))
        run.finish()
        self.assertGreater(150 * 150, MAX_RUN_OBLIGATIONS)
        verdict = judge_isolated(("cross-channel",), run)
        self.assertEqual("accept", verdict.kind, verdict.reason)
        found = statuses_by_scope(verdict, CROSS)
        self.assertEqual(299, len(found))
        self.assertEqual({"satisfied"}, set(found.values()))
        self.assertNotIn("r0", found)

    def test_one_odd_read_is_the_only_violated_scope(self) -> None:
        """FUSE f1 returns other bytes; three Thrift reads and FUSE f2 agree on the
        committed bytes. Only f1 is scoped violated, whether it comes first or last.
        """
        good, bad = committed(R0, TIP), value("z", "odd")
        for odd_first in (True, False):
            with self.subTest(odd_first=odd_first):
                run = Run("t", "odd-read", "five reads of r0, one odd")
                order = ["f1", "t0", "t1", "t2", "f2"]
                if not odd_first:
                    order = order[1:] + order[:1]
                for name in order:
                    if name[0] == "f":
                        run.act("a", name, "read", path=R0, channel="fuse")
                    else:
                        run.act(
                            "a",
                            name,
                            "read",
                            path=R0,
                            channel="thrift",
                            sync_timeout_seconds=0,
                        )
                for name in order:
                    run.call(name, "ok", **read_ok(bad if name == "f1" else good))
                run.finish()
                found = statuses_by_scope(
                    judge_isolated(("cross-channel",), run), CROSS
                )
                self.assertEqual(
                    {"f1"}, {s for s, v in found.items() if v == "violated"}
                )
                self.assertEqual(4, len(found))


def _read(name: str, channel: str, lo: float, hi: float, token: str) -> Any:
    return cross_channel._Read(name, R0, channel, lo, hi, token)


class SpanningForestTest(unittest.TestCase):
    def test_tree_spans_each_class_with_cross_channel_edges(self) -> None:
        members = [
            _read("f1", "fuse", 1, 2, "a"),
            _read("f2", "fuse", 3, 4, "a"),
            _read("t1", "thrift", 5, 6, "a"),
            _read("f3", "fuse", 7, 8, "a"),
            _read("t2", "thrift", 9, 10, "a"),
        ]
        edges = [(p.id, c.id) for p, c in cross_channel.spanning_edges(members)]
        self.assertEqual(
            [("t1", "f2"), ("f1", "t1"), ("t1", "f3"), ("f1", "t2")], edges
        )
        self.assertEqual([], cross_channel.spanning_edges(members[:2]))

    def test_the_root_holds_the_most_common_value(self) -> None:
        members = [
            _read("f1", "fuse", 1, 2, "bad"),
            _read("t0", "thrift", 3, 4, "a"),
            _read("t1", "thrift", 5, 6, "a"),
            _read("f2", "fuse", 7, 8, "a"),
        ]
        edges = [(p.id, c.id) for p, c in cross_channel.spanning_edges(members)]
        self.assertEqual([("t0", "f1"), ("f2", "t1"), ("t0", "f2")], edges)

    def test_tree_edges_agree_exactly_when_every_cross_pair_agrees(self) -> None:
        rng = random.Random(7)
        for _trial in range(2000):
            size = rng.randint(1, 7)
            members = [
                _read(
                    f"r{index}",
                    rng.choice(("fuse", "thrift")),
                    index,
                    index + 0.5,
                    rng.choice("ab"),
                )
                for index in range(size)
            ]
            tree = all(
                p.token == c.token for p, c in cross_channel.spanning_edges(members)
            )
            pairs = all(
                x.token == y.token
                for x, y in itertools.combinations(members, 2)
                if x.channel != y.channel
            )
            self.assertEqual(pairs, tree, members)

    def test_classes_are_the_writer_free_gaps(self) -> None:
        run = Run("t", "gaps", "writes of r0 at known windows")
        run.act("a", "w1", "write", path=R0, value_id="w1")
        run.act("a", "w2", "write", path=R0, value_id="w2")
        run.call("w1", "ok", **WRITE_OK)
        run.call("w2", "ok", **WRITE_OK)
        run.finish()
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        facts = checker.HistoryFacts.build(
            spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
        )
        derived = checker.contract_effects(
            EDENFS_TARGET, spec, facts, REGISTERED_RULE_IDS
        )
        assert not isinstance(derived, checker.PropertyError)
        effects: EffectSet = derived[1]
        windows = sorted(
            (g.window.lo, g.window.hi) for g in effects.groups if g.action[0] == "w"
        )
        (a_lo, a_hi), (b_lo, b_hi) = windows
        reads = [
            _read("before", "fuse", a_lo - 2, a_lo - 1, "x"),
            _read("touch", "thrift", a_hi, a_hi + 0.01, "x"),
            _read("mid1", "fuse", a_hi + 0.01, a_hi + 0.02, "x"),
            _read("mid2", "thrift", b_lo - 0.02, b_lo - 0.01, "x"),
            _read("touch2", "fuse", b_lo - 0.01, b_lo, "y"),
            _read("after", "fuse", b_hi + 1, b_hi + 2, "x"),
        ]
        classes = cross_channel.comparable_classes(effects, reads)
        self.assertEqual(
            [["before"], ["mid1", "mid2"], ["after"]],
            [[read.id for read in members] for members in classes],
        )
