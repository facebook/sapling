# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The effect groups a checkout's contracts record: per-path apply points ordered after the
checkout's own parent move, the NORMAL and FORCE tables, `sl goto`'s DRY_RUN visits
and abort requirement, typed rejections, supersede groups for overlapping writes,
checkouts to the revision they start from, pinned initial values, and the contracts cost
that does not grow with the repository.
"""

from __future__ import annotations

import unittest
from typing import Any

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Cell,
    EffectGroup,
    Expect,
    none_of,
    ObligationRef,
    Observation,
    Status,
    UnknownInitial,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test.testing import ev
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    ABORT_BLOCKERS,
    abort_scope,
    CHECKOUT_ABORT_RULE,
    CHECKOUT_ORDER_RULE,
    conflict_paths,
    CONTRACT_RULES,
    dryrun_cell,
    IN_PROGRESS_RULE,
    updatestate_cell,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    CLEAN,
    DIR,
    EdenPinned,
    file_value,
    INPUT_LEGALITY_RULE,
    mode_cell,
    mode_value,
    NEVER,
    parent_cell,
    path_cell,
    rev_value,
    SAPLING_PARENT,
    source_cell,
    track_cell,
    visit_cell,
    YES,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.fixture import GeneratedFixture
from eden.integration.correctness.exploratory.edenfs.test.contracts_fixtures import (
    action_payload,
    checkout_op,
    checkout_spec,
    committed_value,
    contract_sink,
    DIRTY,
    guard_applies,
    guard_holds,
    is_explained,
    local_state,
    PLAIN,
    R0,
    R0_PART,
    R1,
    read_obs,
    run_spec,
    SEED,
    sl_abort_facts,
    stat_obs,
    THRIFT_OK,
    TIP,
    WALK_OVERLAP,
    WRITTEN,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    CountingRepo,
    RecordingSink,
    VOCAB_TARGET,
)
from eden.integration.correctness.repo import GeneratedRepo


class CheckoutTest(unittest.TestCase):
    def _thrift(
        self, mode: str = "normal", outcome: tuple[str, dict[str, Any]] | None = None
    ) -> tuple[RunSpec, RecordingSink]:
        spec = checkout_spec(("c", checkout_op(1, mode=mode)))
        kind, facts = outcome or (
            "ok",
            {"revision": 1, "node": "n", "via": "thrift", "conflicts": 0},
        )
        return spec, contract_sink(
            spec, [ev("invoke", "c", 1.0), ev(kind, "c", 2.0, **facts)]
        )

    def test_a_settled_checkout_reads_no_mark_and_needs_no_order(self) -> None:
        """Nothing reads the parent inside its window and its source is the start."""
        _spec_, sink = self._thrift()
        parent = sink.by_id("g:c:parent")
        ((cell, value),) = [(e.cell, e.value) for e in parent.effects]
        self.assertEqual((parent_cell(), rev_value(1)), (cell, value))
        paths = [group for group in sink.groups if group.id != "g:c:parent"]
        self.assertTrue(paths)
        for group in paths:
            self.assertEqual((), group.requires)
            for effect in group.effects:
                assert effect.guard is not None
                self.assertFalse(
                    {"source", "parent"} & {c.space for c in effect.guard.cells}
                )
        self.assertNotIn("g:c:sapling", {group.id for group in sink.groups})

    def test_a_watched_checkout_marks_its_source_and_orders_paths_by_it(self) -> None:
        """A parent reader overlapping the checkout: marks carry the order.

        A status, `sl rm`, or `sl mv` may read the parent. No other parent
        move overlaps it, so no path reads the parent.
        """
        moved = "tree/registers/m.reg"
        for payload, done in (
            (action_payload("status", channel="sl"), {"status": [], "diff_paths": []}),
            (action_payload("sl_rm", path=R1), {"path": R1}),
            (
                action_payload("sl_mv", source=R1, destination=moved),
                {"source": R1, "destination": moved},
            ),
        ):
            with self.subTest(kind=payload["kind"]):
                self._watched(payload, done)

    def _watched(self, payload: dict[str, Any], done: dict[str, Any]) -> None:
        spec = checkout_spec(
            ("c", checkout_op(1)),
            extra={"s": payload},
            actors=[{"id": "a", "actions": ["c"]}, {"id": "b", "actions": ["s"]}],
            metadata=WALK_OVERLAP,
        )
        sink = contract_sink(
            spec,
            [
                ev("invoke", "c", 1.0),
                ev("invoke", "s", 1.5),
                ev("ok", "s", 1.6, **done),
                ev("ok", "c", 2.0, revision=1, **THRIFT_OK),
            ],
        )
        parent = sink.by_id("g:c:parent")
        values = {effect.cell: effect for effect in parent.effects}
        self.assertEqual(rev_value(1), values[parent_cell()].value)
        mark = values[source_cell("k0", TIP)]
        self.assertEqual(YES, mark.value)
        assert mark.guard is not None
        self.assertTrue(guard_holds(mark.guard, {parent_cell(): rev_value(TIP)}))
        self.assertFalse(guard_holds(mark.guard, {parent_cell(): rev_value(1)}))
        paths = [group for group in sink.groups if group.id.startswith("g:c:p")]
        paths = [group for group in paths if group.id != "g:c:parent"]
        self.assertTrue(paths)
        for group in paths:
            (order,) = group.requires
            self.assertEqual(ObligationRef(CHECKOUT_ORDER_RULE, "c"), order.obligation)
            self.assertNotIn(parent_cell(), order.cells)
            self.assertTrue(guard_holds(order, {source_cell("k0", TIP): YES}))
            self.assertFalse(guard_holds(order, {}))

    def test_overlapping_checkouts_also_order_paths_by_the_parent(self) -> None:
        """Once another checkout may move the parent inside the window, paths read it."""
        spec = checkout_spec(
            ("c", checkout_op(1)),
            ("d", checkout_op(2)),
            actors=[{"id": "a", "actions": ["c"]}, {"id": "b", "actions": ["d"]}],
        )
        sink = contract_sink(
            spec,
            [
                ev("invoke", "c", 1.0),
                ev("invoke", "d", 1.5),
                ev("ok", "c", 2.0, revision=1, **THRIFT_OK),
                ev("ok", "d", 3.0, revision=2, **THRIFT_OK),
            ],
        )
        group = sink.by_id(f"g:c:{R0_PART}")
        (order,) = group.requires
        marked = {source_cell("k0", TIP): YES}
        self.assertTrue(guard_holds(order, {parent_cell(): rev_value(1), **marked}))
        self.assertFalse(guard_holds(order, {parent_cell(): rev_value(2), **marked}))
        self.assertFalse(guard_holds(order, {parent_cell(): rev_value(1)}))

    def _path_group(self, sink: RecordingSink, path: str) -> EffectGroup:
        cell = path_cell(path)
        return next(
            group
            for group in sink.groups
            if any(effect.cell == cell for effect in group.effects)
            and not group.id.endswith(":track")
        )

    def _applies(
        self, group: EffectGroup, local: Value, source: int = TIP, **kwargs: Any
    ) -> bool:
        return guard_applies(group, local, source, **kwargs)

    def test_thrift_normal_follows_the_per_path_table(self) -> None:
        _spec_, sink = self._thrift()
        group = self._path_group(sink, R0)
        self.assertEqual(committed_value(R0, 1), group.effects[0].value)
        self.assertTrue(self._applies(group, committed_value(R0, TIP)))
        self.assertTrue(self._applies(group, committed_value(R0, 1)))
        self.assertFalse(self._applies(group, DIRTY))
        self.assertFalse(self._applies(group, ABSENT))
        self.assertEqual(
            {path_cell(R0), mode_cell(R0), visit_cell("k0", R0)},
            {effect.cell for effect in group.effects},
        )
        (visit,) = [e for e in group.effects if e.cell == visit_cell("k0", R0)]
        self.assertEqual((YES, group.effects[0].guard), (visit.value, visit.guard))

    def test_paths_the_destination_leaves_unchanged_get_no_group(self) -> None:
        _spec_, sink = self._thrift()
        churn = path_cell("tree/churn/c0")
        self.assertFalse(
            any(
                effect.cell == churn
                for group in sink.groups
                for effect in group.effects
            )
        )

    def test_force_resets_every_path_either_revision_holds(self) -> None:
        spec = checkout_spec(("c", checkout_op(1, mode="force")))
        sink = contract_sink(
            spec,
            [
                ev("invoke", "c", 1.0),
                ev("ok", "c", 2.0, revision=1, node="n", via="thrift", conflicts=1),
            ],
        )
        for path in (R0, "tree/churn/c0"):
            group = self._path_group(sink, path)
            for local in (ABSENT, Value("file", ("dirty",)), DIR):
                with self.subTest(path=path, local=local):
                    self.assertTrue(self._applies(group, local))

    def test_an_acked_sl_goto_blocks_on_unlinked_paths_and_moves_both_parents(
        self,
    ) -> None:
        """A DRY_RUN visit per path, then a NORMAL apply point per path.

        The visit marks whether the path blocks (dirty or unlinked) and
        whether Sapling applies it itself (never a path the source holds);
        the apply point follows EdenFS's NORMAL table and requires its visit
        to have passed.
        """
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        sink = contract_sink(
            spec,
            [
                ev("invoke", "c", 1.0),
                ev("ok", "c", 2.0, revision=1, node="n", via="goto"),
            ],
        )
        dryrun = sink.by_id(f"g:c:{R0_PART}:dryrun")
        marks = {effect.cell.space: effect for effect in dryrun.effects}
        self.assertEqual({"dryrun", "blocked", "planned"}, set(marks))
        for space, local, holds in (
            ("blocked", DIRTY, True),
            ("blocked", ABSENT, True),
            ("blocked", committed_value(R0, TIP), False),
            ("planned", ABSENT, False),
            ("planned", committed_value(R0, TIP), False),
        ):
            guard = marks[space].guard
            assert guard is not None
            with self.subTest(space=space, local=local):
                self.assertEqual(holds, guard_holds(guard, local_state(R0, local)))
        group = sink.by_id(f"g:c:{R0_PART}")
        self.assertFalse(self._applies(group, ABSENT))
        self.assertFalse(self._applies(group, DIRTY))
        self.assertTrue(self._applies(group, committed_value(R0, TIP)))
        guard = group.effects[0].guard
        assert guard is not None
        planned = {cell: YES for cell in guard.cells if cell.space == "planned"}
        self.assertTrue(guard_holds(guard, {**local_state(R0, ABSENT), **planned}))
        self.assertFalse(self._applies(group, ABSENT, visited=False))
        self.assertFalse(self._applies(group, DIRTY, visited=False))
        self.assertIn(track_cell(R0), {effect.cell for effect in group.effects})
        # Settled: no order requirement, only the abort requirement.
        (abort,) = group.requires
        self.assertEqual(CHECKOUT_ABORT_RULE, abort.obligation.rule_id)
        visited = {dryrun_cell("k0", R0): YES}
        self.assertTrue(guard_holds(abort, visited))
        self.assertFalse(guard_holds(abort, {}))
        self.assertFalse(
            guard_holds(
                abort,
                {
                    **visited,
                    **{cell: YES for cell in abort.cells if cell.space == "blocked"},
                },
            )
        )
        sapling = sink.by_id("g:c:sapling")
        self.assertEqual(parent_cell(SAPLING_PARENT), sapling.effects[0].cell)

    def test_every_sl_abort_scope_part_has_a_blockers_identity(self) -> None:
        """The checkout property reads `ABORT_BLOCKERS[part]` for every derived abort scope."""
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        parts: set[str] = set()
        for completion in (
            ev("ok", "c", 2.0, revision=1, node="n", via="goto"),
            ev("fail", "c", 2.0, **sl_abort_facts(1, R0)),
        ):
            sink = contract_sink(spec, [ev("invoke", "c", 1.0), completion])
            refs = [ref for o in sink.observations for ref in o.obligations]
            refs += [r.obligation for g in sink.groups for r in g.requires]
            parts.update(
                ref.scope.removeprefix("c").removeprefix(":")
                for ref in refs
                if ref.rule_id == CHECKOUT_ABORT_RULE
            )
        self.assertEqual(set(ABORT_BLOCKERS), parts)
        with self.assertRaises(ValueError):
            abort_scope("c", "interrupted")

    def test_a_lock_contended_sl_goto_may_have_run_its_checkout(self) -> None:
        """A lock-contended `sl goto` is judged indeterminate like any other failure."""
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        stderr = "abort: lock 'wlock' contended\n"
        events = [
            ev("invoke", "c", 1.0),
            ev(
                "fail",
                "c",
                2.0,
                reason=f"goto: {stderr.strip()}",
                returncode=255,
                stdout="",
                stderr=stderr,
            ),
        ]
        parent = contract_sink(spec, events).by_id("g:c:parent")
        self.assertEqual(Status.INDETERMINATE, parent.status)

    def test_an_aborting_sl_goto_has_no_effect_and_names_its_blockers(self) -> None:
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        stderr = f"abort: 1 conflicting file changes:\n {R0}\n(commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)\n"
        events = [
            ev("invoke", "c", 1.0),
            ev(
                "fail",
                "c",
                2.0,
                reason="goto",
                returncode=255,
                stdout="",
                stderr=stderr,
            ),
        ]
        sink = contract_sink(spec, events)
        self.assertEqual(
            [(updatestate_cell(), ABSENT)],
            [(e.cell, e.value) for group in sink.groups for e in group.effects],
        )
        facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
        self.assertEqual((R0,), conflict_paths(facts.action("c")))
        (observation,) = sink.observations
        self.assertEqual(
            (ObligationRef(CHECKOUT_ABORT_RULE, "c:listed"),), observation.obligations
        )
        start = rev_value(TIP)
        self.assertEqual(
            (parent_cell(), path_cell(R0), mode_cell(R0), track_cell(R0)),
            observation.cells,
        )
        self.assertTrue(observation.expect.holds((start, DIRTY, PLAIN, CLEAN)))
        self.assertFalse(
            observation.expect.holds((start, committed_value(R0, TIP), PLAIN, CLEAN))
        )
        self.assertTrue(observation.expect.holds((start, ABSENT, ABSENT, CLEAN)))

    def test_an_abort_listing_an_unchanged_path_has_no_explanation(self) -> None:
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        stderr = "abort: 1 conflicting file changes:\n tree/churn/c0\n"
        events = [
            ev("invoke", "c", 1.0),
            ev(
                "fail",
                "c",
                2.0,
                reason="goto",
                returncode=255,
                stdout="",
                stderr=stderr,
            ),
        ]
        (observation,) = contract_sink(spec, events).observations
        for local in (ABSENT, DIRTY):
            for mode in (ABSENT, PLAIN, mode_value(True)):
                self.assertFalse(
                    observation.expect.holds((rev_value(TIP), local, mode, CLEAN))
                )

    def test_an_interrupted_sl_goto_keeps_sapling_and_tracking_optional(self) -> None:
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        stderr = "abort: EdenError: std::runtime_error: fault\n"
        sink = contract_sink(
            spec,
            [
                ev("invoke", "c", 1.0),
                ev(
                    "fail",
                    "c",
                    2.0,
                    reason="goto",
                    returncode=255,
                    stdout="",
                    stderr=stderr,
                ),
                Event("target-closed", 5.0, {}),
            ],
        )
        self.assertTrue(
            all(group.status == Status.INDETERMINATE for group in sink.groups)
        )
        # EdenFS reported the failure, so every effect precedes the reply.
        self.assertTrue(all(group.window == Window(1.0, 2.0) for group in sink.groups))
        ids = {group.id for group in sink.groups}
        self.assertNotIn("g:c:sapling", ids)
        self.assertTrue(any(group_id.endswith(":track") for group_id in ids))
        group = self._path_group(sink, R0)
        self.assertNotIn(track_cell(R0), {effect.cell for effect in group.effects})

    def test_typed_rejections_have_no_effect(self) -> None:
        for facts in (
            {
                "reason": "another checkout operation is still in progress",
                "source": "thrift",
                "error_type": "CHECKOUT_IN_PROGRESS",
            },
        ):
            _spec_, sink = self._thrift(outcome=("fail", facts))
            self.assertTrue(sink.groups)
            for group in sink.groups:
                self.assertEqual(Status.INDETERMINATE, group.status)
                (requirement,) = group.requires
                self.assertEqual(
                    ObligationRef(IN_PROGRESS_RULE, "c"), requirement.obligation
                )
                self.assertEqual(((NEVER,),), requirement.expect.rows)
        _spec_, sink = self._thrift(
            outcome=("fail", {"reason": "rpc: timed out", "source": "thrift"})
        )
        self.assertTrue(sink.groups)
        self.assertTrue(
            all(group.status == Status.INDETERMINATE for group in sink.groups)
        )

    def test_an_overlapping_write_gets_an_optional_supersede_group(self) -> None:
        """A write overlapping a checkout may be superseded.

        The main guard stays deterministic on the local value; the supersede
        outcome is a separate indeterminate group inside both windows. A write
        whose window misses the checkout's on either side gets none.
        """
        spec = checkout_spec(
            ("c", checkout_op(1)),
            extra={
                "w": action_payload("write", path=R0, value_id="w"),
                "e": action_payload("write", path=R0, value_id="e"),
                "l": action_payload("write", path=R0, value_id="l"),
            },
            actors=[
                {"id": "a", "actions": ["c"]},
                {"id": "b", "actions": ["e", "w", "l"]},
            ],
        )
        events = [
            ev("invoke", "e", 0.1),
            ev("ok", "e", 0.2, bytes_written=64),
            ev("invoke", "c", 1.0),
            ev("invoke", "w", 1.5),
            ev("ok", "w", 1.6, bytes_written=64),
            ev("ok", "c", 2.0, revision=1, node="n", via="thrift", conflicts=0),
            ev("invoke", "l", 3.0),
            ev("ok", "l", 3.1, bytes_written=64),
        ]
        sink = contract_sink(spec, events)
        written = file_value(encode_value(SEED, "b", "w"))
        main = sink.by_id(f"g:c:{R0_PART}")
        self.assertFalse(self._applies(main, written))
        (supersede,) = [
            g
            for g in sink.groups
            if ":supersede:" in g.id and not g.id.endswith(":resolve")
        ]
        self.assertEqual(f"g:c:{R0_PART}:supersede:w", supersede.id)
        self.assertEqual(Status.INDETERMINATE, supersede.status)
        self.assertEqual(Window(1.5, 1.6), supersede.window)
        self.assertTrue(self._applies(supersede, written))
        self.assertFalse(self._applies(supersede, written, visited=False))
        self.assertFalse(self._applies(supersede, committed_value(R0, TIP)))
        self.assertEqual(committed_value(R0, 1), supersede.effects[0].value)

    def test_each_overlapping_op_gets_its_own_supersede_group(self) -> None:
        """A write and a chmod are superseded independently.

        Each group resets only the cell its op set (bytes or mode) and
        marks no conflict. It exists wherever the checkout swaps the inode:
        FORCE on a changed or a kept path, NORMAL on a changed path only.
        """
        written = file_value(encode_value(SEED, "b", "w"))
        for mode, revision, superseded in (
            ("normal", 1, True),
            ("force", 1, True),
            ("normal", TIP, False),
            ("force", TIP, True),
        ):
            with self.subTest(mode=mode, revision=revision):
                spec = checkout_spec(
                    ("c", checkout_op(revision, mode=mode)),
                    extra={
                        "w": action_payload("write", path=R0, value_id="w"),
                        "m": action_payload("chmod", path=R0, mode=0o755),
                    },
                    actors=[
                        {"id": "a", "actions": ["c"]},
                        {"id": "b", "actions": ["w", "m"]},
                    ],
                )
                sink = contract_sink(
                    spec,
                    [
                        ev("invoke", "c", 1.0),
                        ev("invoke", "w", 1.2),
                        ev("ok", "w", 1.4, bytes_written=64),
                        ev("invoke", "m", 1.5),
                        ev("ok", "m", 1.6, path=R0, mode=0o755),
                        ev(
                            "ok",
                            "c",
                            2.0,
                            revision=revision,
                            node="n",
                            via="thrift",
                            conflicts=0,
                        ),
                    ],
                )
                groups = {
                    group.id.rsplit(":", 1)[1]: group
                    for group in sink.groups
                    if ":supersede:" in group.id and not group.id.endswith(":resolve")
                }
                if not superseded:
                    self.assertEqual({}, groups)
                    continue
                self.assertEqual({"w", "m"}, set(groups))
                write, chmod = groups["w"], groups["m"]
                destination = committed_value(R0, revision)
                self.assertEqual(
                    [(path_cell(R0), destination)],
                    [(effect.cell, effect.value) for effect in write.effects],
                )
                self.assertEqual(
                    [(mode_cell(R0), PLAIN)],
                    [(effect.cell, effect.value) for effect in chmod.effects],
                )
                self.assertEqual(Window(1.2, 1.4), write.window)
                self.assertEqual(Window(1.5, 1.6), chmod.window)
                self.assertTrue(self._applies(write, written))
                self.assertFalse(self._applies(write, written, visited=False))
                self.assertFalse(self._applies(write, destination))
                executable = mode_value(True)
                self.assertTrue(self._applies(chmod, destination, mode=executable))
                self.assertTrue(self._applies(chmod, written, mode=executable))
                self.assertFalse(self._applies(chmod, destination))
                self.assertFalse(
                    self._applies(chmod, destination, mode=executable, visited=False)
                )

    def test_an_op_invoked_after_the_swap_is_never_superseded(self) -> None:
        """An op invoked after a read inside the checkout saw the destination is kept.

        A read inside the checkout returns the destination, so the path
        was swapped before the op was invoked: the op reached the new
        inode and is kept. Without that read the op may have resolved the
        old inode first, so losing it stays legal.
        """
        destination = committed_value(R0, 1)
        for mode, kind in (
            ("normal", "write"),
            ("normal", "chmod"),
            ("force", "write"),
            ("force", "chmod"),
        ):
            with self.subTest(mode=mode, kind=kind):
                payload = (
                    action_payload("write", path=R0, value_id="w")
                    if kind == "write"
                    else action_payload("chmod", path=R0, mode=0o755)
                )
                spec = checkout_spec(
                    ("c", checkout_op(1, mode=mode)),
                    extra={"w": payload},
                    actors=[
                        {"id": "a", "actions": ["c"]},
                        {"id": "b", "actions": ["w"]},
                    ],
                )
                result = (
                    {"bytes_written": 64}
                    if kind == "write"
                    else {"path": R0, "mode": 0o755}
                )
                events = [
                    ev("invoke", "c", 1.0),
                    ev("invoke", "w", 1.5),
                    ev("ok", "w", 1.6, **result),
                    ev("ok", "c", 2.0, revision=1, node="n", via="thrift", conflicts=0),
                    Event("target-closed", 9.0, {}),
                ]
                swapped = read_obs("e", R0, destination, 1.1, 1.2)
                lost, kept = (
                    (
                        read_obs("f", R0, destination, 5, 6),
                        read_obs(
                            "f", R0, file_value(encode_value(SEED, "b", "w")), 5, 6
                        ),
                    )
                    if kind == "write"
                    else (stat_obs("f", R0, False, 5, 6), stat_obs("f", R0, True, 5, 6))
                )
                self.assertFalse(is_explained(spec, events, (swapped, lost)))
                self.assertTrue(is_explained(spec, events, (swapped, kept)))
                self.assertTrue(is_explained(spec, events, (lost,)))
                early = [ev("invoke", "w", 0.5), *events[:1], *events[2:]]
                self.assertTrue(is_explained(spec, early, (lost,)))


class PinnedTest(unittest.TestCase):
    def test_initial_values_come_from_the_fixture(self) -> None:
        spec = run_spec(
            {"x": action_payload("write", path=R0, value_id="w")},
            [{"id": "a", "actions": ["x"]}],
            scatter_dirty=2,
        )
        pinned = EdenPinned.for_spec(spec)
        layout = pinned.fixture.layout
        dirty = layout.file_at(0, TIP).path
        self.assertEqual(committed_value(R0, TIP), pinned.initial(path_cell(R0)))
        self.assertEqual(
            file_value(b"q" * 64 + b"\n"), pinned.initial(path_cell(dirty))
        )
        self.assertEqual(DIR, pinned.initial(path_cell("tree/registers")))
        self.assertEqual(ABSENT, pinned.initial(path_cell("tree/registers/new")))
        self.assertEqual(mode_value(False), pinned.initial(Cell("primary", "mode", R0)))
        self.assertEqual(CLEAN, pinned.initial(track_cell(R0)))
        self.assertEqual(rev_value(TIP), pinned.initial(parent_cell(SAPLING_PARENT)))
        self.assertEqual(ABSENT, pinned.initial(source_cell("k0", 1)))
        self.assertEqual(ABSENT, pinned.initial(Cell("primary", "fd", "o")))
        self.assertEqual(
            committed_value(R0, TIP),
            pinned.initial(Cell("primary", "fd-object", f"o/{R0}")),
        )
        with self.assertRaises(UnknownInitial):
            pinned.initial(Cell("primary", "fd-object", "o"))
        with self.assertRaises(UnknownInitial):
            pinned.initial(Cell("primary", "journal", "o"))


class ScaleTest(unittest.TestCase):
    def _queries(self, file_count: int, aggregate: bool) -> tuple[int, int]:
        tip_path = (
            GeneratedRepo(SEED, file_count, TIP + 1).file_at(file_count - 1, TIP).path
        )
        actions = {
            "w": action_payload("write", path=R0, value_id="w"),
            "g": action_payload("getattr", path=tip_path),
            "c": checkout_op(1),
            "m": action_payload("mkdir", path="tree/registers/d"),
        }
        if aggregate:
            actions["s"] = action_payload("status", channel="sl")
        spec = run_spec(
            actions, [{"id": "a", "actions": list(actions)}], file_count=file_count
        )
        events: list[Event] = []
        results = {
            "w": {"bytes_written": 64},
            "g": {
                "size": 1,
                "mtime": 0.0,
                "mtime_history": 0.0,
                "mode": 420,
                "fuse_type": "file",
            },
            "c": {"revision": 1, "node": "n", "via": "thrift", "conflicts": 0},
            "m": {"path": "tree/registers/d"},
            "s": {"status": [], "diff_paths": []},
        }
        for index, action in enumerate(actions):
            events += [
                ev("invoke", action, 2.0 * index),
                ev("ok", action, 2.0 * index + 1, **results[action]),
            ]
        layout = CountingRepo(SEED, file_count, TIP + 1)
        pinned = EdenPinned(GeneratedFixture.for_spec(spec, layout))
        sink = contract_sink(spec, events, pinned)
        return sum(layout.calls.values()), len(sink.groups)

    def test_contract_queries_do_not_grow_with_one_million_files(self) -> None:
        small = self._queries(10_000, aggregate=False)
        large = self._queries(1_000_000, aggregate=False)
        self.assertEqual(small, large)
        queries, groups = large
        self.assertLessEqual(queries, 40 * groups)

    def test_aggregate_representatives_stay_bounded(self) -> None:
        """Representatives saturate: 100k and 1M files cost the same queries and groups."""
        small = self._queries(10_000, aggregate=True)
        medium = self._queries(100_000, aggregate=True)
        large = self._queries(1_000_000, aggregate=True)
        self.assertEqual(medium, large)
        self.assertLessEqual(small, large)
        queries, groups = large
        self.assertLessEqual(groups, 200)
        self.assertLessEqual(queries, 20_000)


class SameRevisionCheckoutTest(unittest.TestCase):
    """A checkout to the revision it starts from still decides its paths.

    The parent value already equals the destination before the parent
    group runs, so only the checkout's own source mark can order its
    paths; a path group that ran first must not become a no-op.
    """

    _RULE_SETS = (CONTRACT_RULES, frozenset({INPUT_LEGALITY_RULE}))

    def _dirty_then_checkout(
        self, mode: str = "force", outcome: str = "ok", via: str = "thrift"
    ) -> tuple[RunSpec, list[Event]]:
        spec = run_spec(
            {
                "w": action_payload("write", path=R0, value_id="w"),
                "c": checkout_op(TIP, via=via, mode=mode),
            },
            [{"id": "a", "actions": ["w", "c"]}],
        )
        conflicts = 1 if mode == "force" else 0
        done = (
            ev("ok", "c", 4.0, revision=TIP, **{**THRIFT_OK, "conflicts": conflicts})
            if outcome == "ok"
            else ev("info", "c", 4.0, reason="deadline")
        )
        events = [
            ev("invoke", "w", 1.0),
            ev("ok", "w", 2.0, bytes_written=64),
            ev("invoke", "c", 3.0),
            done,
            Event("target-closed", 9.0, {}),
        ]
        return spec, events

    def test_force_to_the_current_revision_resets_a_dirty_write(self) -> None:
        spec, events = self._dirty_then_checkout()
        dirty = read_obs("r", R0, WRITTEN, 5, 6)
        reset = read_obs("r", R0, committed_value(R0, TIP), 5, 6)
        for rules in self._RULE_SETS:
            with self.subTest(rules=sorted(rules)):
                self.assertFalse(is_explained(spec, events, (dirty,), rules))
                self.assertTrue(is_explained(spec, events, (reset,), rules))

    def test_force_to_the_current_revision_clears_the_dirt_status_reports(
        self,
    ) -> None:
        """The old `DirtinessResetTest` shape: the path still differs from `.` afterwards."""
        spec, events = self._dirty_then_checkout()
        still_modified = Observation(
            id="s",
            action="s",
            cells=(path_cell(R0),),
            expect=Expect(((none_of({committed_value(R0, TIP)}),),)),
            window=Window(5, 6),
            channel="sl-status",
            obligations=(ObligationRef("status.path", "s"),),
        )
        for rules in self._RULE_SETS:
            with self.subTest(rules=sorted(rules)):
                self.assertFalse(is_explained(spec, events, (still_modified,), rules))

    def test_force_to_the_current_revision_resets_mode_dirt(self) -> None:
        spec = run_spec(
            {
                "m": action_payload("chmod", path=R0, mode=0o755),
                "c": checkout_op(TIP, mode="force"),
            },
            [{"id": "a", "actions": ["m", "c"]}],
        )
        events = [
            ev("invoke", "m", 1.0),
            ev("ok", "m", 2.0, path=R0, mode=0o755),
            ev("invoke", "c", 3.0),
            ev("ok", "c", 4.0, revision=TIP, **{**THRIFT_OK, "conflicts": 1}),
        ]
        for rules in self._RULE_SETS:
            with self.subTest(rules=sorted(rules)):
                self.assertFalse(
                    is_explained(spec, events, (stat_obs("s", R0, True, 5, 6),), rules)
                )
                self.assertTrue(
                    is_explained(spec, events, (stat_obs("s", R0, False, 5, 6),), rules)
                )

    def test_an_unfinished_force_to_the_current_revision_may_keep_the_dirt(
        self,
    ) -> None:
        spec, events = self._dirty_then_checkout(outcome="info")
        for value in (WRITTEN, committed_value(R0, TIP)):
            with self.subTest(value=value):
                self.assertTrue(
                    is_explained(spec, events, (read_obs("r", R0, value, 5, 6),))
                )

    def test_normal_to_the_current_revision_keeps_the_dirt(self) -> None:
        """NORMAL changes no path the destination leaves unchanged ."""
        spec, events = self._dirty_then_checkout(mode="normal")
        self.assertTrue(is_explained(spec, events, (read_obs("r", R0, WRITTEN, 5, 6),)))
        self.assertFalse(
            is_explained(
                spec, events, (read_obs("r", R0, committed_value(R0, TIP), 5, 6),)
            )
        )

    def test_paths_need_their_own_mark_even_when_the_parent_already_matches(
        self,
    ) -> None:
        """When a status may observe the checkout, its own mark orders the paths."""
        spec, events = self._dirty_then_checkout()
        watched = run_spec(
            {
                **{
                    a.id: {"kind": a.kind, "deadline": "operation", **a.payload}
                    for a in spec.actions
                },
                "s": action_payload("status", channel="sl"),
            },
            [{"id": "a", "actions": ["w", "c"]}, {"id": "b", "actions": ["s"]}],
        )
        status = [ev("invoke", "s", 3.5), ev("ok", "s", 3.6, status=[], diff_paths=[])]
        group = contract_sink(watched, [*events[:3], *status, *events[3:]]).by_id(
            f"g:c:{R0_PART}"
        )
        (order,) = [
            r for r in group.requires if r.obligation.rule_id == CHECKOUT_ORDER_RULE
        ]
        self.assertFalse(guard_holds(order, {}))
        self.assertTrue(guard_holds(order, {source_cell("k0", TIP): YES}))
