# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The held-descriptor property: a read through a held descriptor returns the content of
the object its open bound. Complements the corpus family `fd`: these tests pin how
the contracts carries the bound object (bindings, renames, unlinks, checkout swaps and
detaches), nemesis-caused descriptor failures, EBADF after a failed open, the gap a
NORMAL checkout opens, and the footprint a moved object can reach.
"""

from __future__ import annotations

import errno
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.effects import EffectKind
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts import (
    model as contracts_model,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
    descriptor_plans,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    DESCRIPTOR_RULE,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    DESCRIPTOR_BINDING,
    DESCRIPTOR_OBJECT,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    contract_run,
    judge,
    judge_isolated,
    obligations_with_prefix,
    status_of,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import footprint_of
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    R0,
    R1,
    read_ok,
    Run,
    thrift_checkout,
    thrift_rejected,
    TIP,
    value,
    WRITE_OK,
)


def _held(*, between: list[tuple[str, str, dict[str, Any]]], data: bytes) -> Run:
    """r0 is opened; actions run; read_fd returns `data`."""
    run = Run("t", "held", "a held descriptor")
    run.act("a", "o", "open", path=R0)
    for action_id, kind, payload in between:
        run.act("a", action_id, kind, **payload)
    run.act("a", "d", "read_fd", fd="o")
    run.call("o", "ok", fd=3)
    for action_id, kind, payload in between:
        facts = WRITE_OK if kind == "write" else payload
        run.call(action_id, "ok", **facts)
    run.call("d", "ok", **read_ok(data))
    run.finish()
    return run


class DescriptorContractsTest(unittest.TestCase):
    def test_open_binds_and_rename_moves_the_binding(self) -> None:
        run = _held(
            between=[("m", "rename", {"source": R0, "destination": R1})],
            data=committed(R0, TIP),
        )
        sink = contract_run(run)
        spec = sink.spec
        opened = next(group for group in sink.groups if group.action == "o")
        self.assertEqual(
            {(effect.cell.space, effect.cell.key) for effect in opened.effects},
            {("fd", "d0"), (DESCRIPTOR_BINDING, f"d0/{R0}")},
        )
        moved = next(group for group in sink.groups if group.action == "m")
        transfers = {
            (effect.cell.space, effect.source.key, effect.cell.key)
            for effect in moved.effects
            if effect.kind == EffectKind.TRANSFER and effect.source is not None
        }
        self.assertIn((DESCRIPTOR_BINDING, f"d0/{R0}", f"d0/{R1}"), transfers)
        self.assertIn((DESCRIPTOR_OBJECT, f"d0/{R0}", f"d0/{R1}"), transfers)
        facts = checker.HistoryFacts.build(
            spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
        )
        plan = descriptor_plans(spec, facts, None)["o"]
        self.assertEqual(frozenset({R0, R1}), plan.paths)
        self.assertEqual(("d0", ()), (plan.key, plan.gaps))

    def _after_checkout(self, data: bytes) -> checker.Verdict:
        """r0 is opened, a checkout replaces it, and a write goes to the new inode."""
        run = Run("t", "checkout-then-write", "a write after the swap")
        run.act("a", "o", "open", path=R0)
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "d", "read_fd", fd="o")
        run.call("o", "ok", fd=3)
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
        run.call("w", "ok", **WRITE_OK)
        run.call("d", "ok", **read_ok(data))
        run.finish()
        return judge(run)

    def test_a_checkout_swap_unbinds_the_object(self) -> None:
        self.assertEqual(
            "satisfied",
            status_of(self._after_checkout(committed(R0, TIP)), DESCRIPTOR_RULE, "d"),
        )
        self.assertEqual(
            "violated",
            status_of(self._after_checkout(value("a", "w")), DESCRIPTOR_RULE, "d"),
        )

    def test_no_descriptor_cells_without_the_rule(self) -> None:
        run = _held(
            between=[("w", "write", {"path": R0, "value_id": "w"})],
            data=value("a", "w"),
        )
        sink = contract_run(run, CONTRACT_RULES - {DESCRIPTOR_RULE})
        spaces = {
            cell.space
            for group in sink.groups
            for effect in group.effects
            for cell in effect.writes()
        }
        self.assertFalse(spaces & {"fd", DESCRIPTOR_BINDING, DESCRIPTOR_OBJECT})


class DescriptorFailureTest(unittest.TestCase):
    def _failing(self, code: int) -> checker.Verdict:
        run = Run("t", "fails", "read_fd fails")
        run.act("a", "o", "open", path=R0)
        run.act("a", "d", "read_fd", fd="o")
        run.call("o", "ok", fd=3)
        run.call("d", "fail", **errno_fail(code, "pread"))
        run.finish()
        return judge(run)

    def test_eio_is_a_finding_with_its_errno(self) -> None:
        verdict = self._failing(errno.EIO)
        assert verdict.report is not None
        (item,) = [
            item
            for result in verdict.report.results
            for item in result.obligations
            if item.rule_id == "fd.read-failed"
        ]
        self.assertEqual("violated", str(item.status))
        self.assertEqual("undetermined", str(item.attribution))
        self.assertIn(("errno", "EIO"), [(w.role, w.value) for w in item.witnesses])

    def test_another_failure_is_not_this_rule(self) -> None:
        verdict = self._failing(errno.ENOTCONN)
        self.assertIsNone(status_of(verdict, "fd.read-failed", "d"))


def _removed_then_held(remover: str, *, open_outcome: str = "fail") -> Run:
    """A remover of r0, then open r0, read_fd, and close: the serial twin."""
    run = Run("t", f"{remover}-then-held", "r0 is gone before the open")
    if remover == "rename":
        run.act("a", "u", "rename", source=R0, destination=R1)
    else:
        run.act("a", "u", "unlink", path=R0)
    run.act("a", "o", "open", path=R0)
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    if remover == "rename":
        run.call("u", "ok", source=R0, destination=R1)
    else:
        run.call("u", "ok", path=R0)
    if open_outcome == "fail":
        run.call("o", "fail", **errno_fail(errno.ENOENT, "open"))
    else:
        run.call("o", "ok", fd=3)
    run.call("d", "fail", **errno_fail(errno.EBADF, "read: no descriptor"))
    run.call("e", "fail", **errno_fail(errno.EBADF, "close: no descriptor"))
    run.finish()
    return run


class UnopenedDescriptorTest(unittest.TestCase):
    """A `read_fd` or `close` whose open failed fails EBADF."""

    def test_ebadf_after_a_failed_open_is_accepted(self) -> None:
        for remover in ("unlink", "rename"):
            with self.subTest(remover=remover):
                verdict = judge(_removed_then_held(remover))
                self.assertEqual("accept", str(verdict.kind), verdict.reason)
                for scope in ("o", "d"):
                    self.assertEqual(
                        "satisfied", status_of(verdict, "core.input-legality", scope)
                    )
                # `close` is not a modeled input.
                self.assertIsNone(status_of(verdict, "core.input-legality", "e"))
                self.assertEqual(set(), obligations_with_prefix(verdict, "fd."))

    def test_ebadf_after_an_acked_open_is_never_a_product_finding(self) -> None:
        """The backend holds a descriptor for every acked open; this is no state."""
        verdict = judge_isolated(
            ("core.input-legality", "fd"),
            _removed_then_held("unlink", open_outcome="ok"),
        )
        self.assertNotEqual("reject", str(verdict.kind), verdict.reason)
        self.assertEqual(
            "not-evaluated", status_of(verdict, "core.input-legality", "d")
        )


def _held_across(
    mode: str, revision: int, data: bytes, *, dirty: bool = False, conflicts: int = 0
) -> Run:
    """r0 is opened (after a write `v` when dirty), a Thrift checkout runs, a write
    `w` goes through r0, and `read_fd` returns `data`."""
    run = Run("t", f"held-{mode}-{revision}", "a held descriptor across a checkout")
    if dirty:
        run.act("a", "v", "write", path=R0, value_id="v")
    run.act("a", "o", "open", path=R0)
    run.act("a", "c", "checkout", revision=revision, via="thrift", mode=mode)
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    if dirty:
        run.call("v", "ok", **WRITE_OK)
    run.call("o", "ok", fd=3)
    run.call("c", "ok", **thrift_checkout(revision, conflicts=conflicts))
    run.call("w", "ok", **WRITE_OK)
    run.call("d", "ok", **read_ok(data))
    run.call("e", "ok", fd=3)
    run.finish()
    return run


class CheckoutDetachTest(unittest.TestCase):
    """A checkout surely detaches a held object only where it changes the path's value.

    EdenFS skips a clean subtree already at the destination, FORCE included
    (`TreeInode::canShortCircuitCheckout`), and replaces a loaded inode its
    walk reaches; a FORCE checkout to the current revision keeping the held
    inode was observed live.
    """

    def test_force_to_the_current_revision_may_keep_the_object(self) -> None:
        """The live shape: the write after the checkout reaches the object."""
        for data in (value("a", "w"), committed(R0, TIP)):
            with self.subTest(data=data[:8]):
                verdict = judge_isolated(("fd",), _held_across("force", TIP, data))
                self.assertEqual("satisfied", status_of(verdict, DESCRIPTOR_RULE, "d"))
                self.assertEqual("accept", str(verdict.kind), verdict.reason)

    def test_an_unchanged_value_detaches_only_in_its_own_indeterminate_group(
        self,
    ) -> None:
        sink = contract_run(_held_across("force", TIP, value("a", "w")))
        binding = (DESCRIPTOR_BINDING, f"d0/{R0}")
        writers = [
            group
            for group in sink.groups
            if group.action == "c"
            for effect in group.effects
            if (effect.cell.space, effect.cell.key) == binding
        ]
        optional = [g for g in writers if str(g.status) == "indeterminate"]
        self.assertEqual(1, len(optional))
        self.assertEqual(1, len(optional[0].effects))
        for group in writers:
            for effect in group.effects:
                if (effect.cell.space, effect.cell.key) != binding:
                    continue
                assert effect.guard is not None
                index = effect.guard.cells.index(contracts_model.path_cell(R0))
                atoms = {str(row[index].op) for row in effect.guard.expect.rows}
                expected = {"eq"} if group in optional else {"not-in"}
                self.assertEqual(expected, atoms, group.id)

    def test_a_rejected_checkout_never_detaches_the_object(self) -> None:
        """The optional unbind keeps the rejected group's requirement."""
        for mode in ("force", "normal"):
            for data, kind in (
                (committed(R0, TIP), "reject"),
                (value("a", "w"), "accept"),
            ):
                with self.subTest(mode=mode, stale=kind == "reject"):
                    verdict = judge_isolated(("fd",), _rejected_across(mode, data))
                    self.assertEqual(kind, str(verdict.kind), verdict.reason)
                    expected = "violated" if kind == "reject" else "satisfied"
                    self.assertEqual(expected, status_of(verdict, DESCRIPTOR_RULE, "q"))

    def test_the_optional_unbind_carries_the_rejected_groups_requirements(
        self,
    ) -> None:
        sink = contract_run(_rejected_across("force", value("a", "w")))
        optional = [
            group
            for group in sink.groups
            if group.action == "c2" and str(group.status) == "indeterminate"
            for effect in group.effects
            if effect.cell.space == DESCRIPTOR_BINDING
        ]
        self.assertTrue(optional)
        for group in optional:
            self.assertIn(
                "checkout.in-progress-rejection",
                {item.obligation.rule_id for item in group.requires},
                group.id,
            )


def _rejected_across(mode: str, data: bytes) -> Run:
    """r0 is opened; c1 (NORMAL to the current revision) holds the guard while c2
    (`mode`, same revision) is rejected; a write `w` goes through r0, and
    `read_fd` returns `data`."""
    run = Run("t", f"rejected-{mode}", "a rejected checkout across a descriptor")
    run.act("a", "o", "open", path=R0)
    for actor, name, how in (("b", "c1", "normal"), ("c", "c2", mode)):
        run.act(
            actor,
            name,
            "checkout",
            revision=TIP,
            via="thrift",
            mode=how,
            requires=[("o", "finished")],
        )
    run.act(
        "a",
        "w",
        "write",
        path=R0,
        value_id="w",
        requires=[("c1", "finished"), ("c2", "finished")],
    )
    run.act("a", "q", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    run.call("o", "ok", fd=3)
    run.inv("c1")
    run.inv("c2")
    run.done("c2", "fail", **thrift_rejected())
    run.done("c1", "ok", **thrift_checkout(TIP, conflicts=0))
    run.call("w", "ok", **WRITE_OK)
    run.call("q", "ok", **read_ok(data))
    run.call("e", "ok", fd=3)
    run.finish()
    return run


def _gapped(
    *,
    write_after: str | None,
    revision: int = 2,
    mode: str = "normal",
    written: bool = False,
) -> Run:
    """open, read_fd, close of r0; a write to r0 overlaps a Thrift checkout.

    `write_after` names the action the overlapping pair waits for: "e" puts it
    after the close, "o" between the open and the read (the read waits for it).
    The read returns the write's bytes with `written`, else the source's.
    """
    run = Run("t", "gapped", f"a write overlaps a {mode} checkout")
    run.act("a", "o", "open", path=R0)
    run.act(
        "b",
        "c",
        "checkout",
        revision=revision,
        via="thrift",
        mode=mode,
        requires=[(write_after, "finished")] if write_after else [],
    )
    run.act(
        "c",
        "w",
        "write",
        path=R0,
        value_id="w",
        requires=[(write_after, "finished")] if write_after else [],
    )
    if write_after == "o":
        run.act(
            "a", "d", "read_fd", fd="o", requires=[("c", "finished"), ("w", "finished")]
        )
    else:
        run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    data = value("c", "w") if written else committed(R0, TIP)
    run.call("o", "ok", fd=3)
    if write_after == "o":
        run.inv("c", "w")
        run.done("w", "ok", **WRITE_OK)
        run.done("c", "ok", **thrift_checkout(revision, conflicts=0))
        run.call("d", "ok", **read_ok(data))
        run.call("e", "ok", fd=3)
    else:
        run.call("d", "ok", **read_ok(data))
        run.call("e", "ok", fd=3)
        run.inv("c", "w")
        run.done("w", "ok", **WRITE_OK)
        run.done("c", "ok", **thrift_checkout(revision, conflicts=0))
    run.finish()
    return run


class DescriptorGapTest(unittest.TestCase):
    """`fd.unmodeled-object` covers only reads an overlap can reach."""

    def test_an_overlap_after_the_close_leaves_the_read_judged(self) -> None:
        verdict = judge_isolated(("fd",), _gapped(write_after="e"))
        self.assertEqual("satisfied", status_of(verdict, DESCRIPTOR_RULE, "d"))
        self.assertEqual("accept", str(verdict.kind), verdict.reason)

    def test_a_force_overlap_leaves_the_read_unevaluated(self) -> None:
        """FORCE swaps the inode as NORMAL does, so the write
        may land in the inode the descriptor holds, with no conflict."""
        for written in (False, True):
            with self.subTest(written=written):
                verdict = judge_isolated(
                    ("fd",), _gapped(write_after="o", mode="force", written=written)
                )
                self.assertEqual(
                    "not-evaluated", status_of(verdict, DESCRIPTOR_RULE, "d")
                )
                self.assertNotEqual("reject", str(verdict.kind), verdict.reason)


def _moved_read() -> Run:
    """open r0; rename r0->r1; a write to r1 beside the read through the open."""
    run = Run("t", "diff-moved-fd", "a write through the moved object", reference=True)
    run.act("a", "o", "open", path=R0)
    run.act("a", "m", "rename", source=R0, destination=R1)
    run.act("a", "d", "read_fd", fd="o")
    run.act("a", "e", "close", fd="o")
    requires = [("m", "finished"), ("e", "finished")]
    run.act("b", "w", "write", path=R1, value_id="w", requires=requires)
    run.paired("o", "ok", fd=3)
    run.paired("m", "ok", source=R0, destination=R1)
    run.paired("d", "ok", **read_ok(committed(R0, TIP)))
    run.paired("e", "ok", fd=3)
    run.paired("w", "ok", **WRITE_OK)
    run.finish()
    return run


class DescriptorFootprintTest(unittest.TestCase):
    """A descriptor's footprint follows its object through moves."""

    def test_the_footprint_reaches_every_path_a_move_can_carry_the_object_to(
        self,
    ) -> None:
        spec = from_json(_moved_read().spec_text(), EDENFS_TARGET)
        for action_id in ("d", "e"):
            self.assertEqual(
                frozenset({R0, R1}), footprint_of(spec, spec.action(action_id)).paths()
            )
