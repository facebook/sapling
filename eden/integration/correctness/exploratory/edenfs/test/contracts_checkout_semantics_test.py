# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Checkout outcomes the engine decides over contract inputs, as legal and
illegal twins: which dirt a NORMAL checkout keeps, when `sl goto` aborts and which
paths an abort may list, type-changed directories, FORCE resets, untracked copies of
the destination, a killed `sl goto` followed by another checkout, and operations
between `sl goto`'s DRY_RUN and NORMAL phases.
"""

from __future__ import annotations

import errno
import hashlib
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    Cell,
    Exclusive,
    ObligationRef,
    Status,
    Value,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test.testing import ev
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    conflict_paths,
    CONTRACT_RULES,
    count_judgment,
    CountJudgment,
    IN_PROGRESS_RULE,
    resolution,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import REMOVED
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    CLEAN,
    DIR,
    EdenPinned,
    file_value,
    mode_cell,
    mode_value,
    path_cell,
    Resolution,
    rev_value,
    source_cell,
    YES,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    encode_value,
)
from eden.integration.correctness.exploratory.edenfs.test.contracts_fixtures import (
    action_payload,
    checkout_op,
    checkout_spec,
    committed_value,
    contract_sink,
    digest_facts,
    DIRTY,
    engine_explains,
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
    SL_OK,
    stat_obs,
    THRIFT_OK,
    TIP,
    WALK_OVERLAP,
    WRITTEN,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import VOCAB_TARGET


GETATTR_OK = {
    "size": 64,
    "mtime": 1.5,
    "mtime_history": 1.5,
    "mode": 0o100644,
    "fuse_type": "file",
}


class CheckoutSemanticsTest(unittest.TestCase):
    """Contract inputs decided by the engine, legal and illegal twins."""

    def test_mode_only_dirt_is_kept_by_thrift_normal(self) -> None:
        """Chmod-only dirt on a changed path is kept."""
        spec = run_spec(
            {
                "m": action_payload("chmod", path=R0, mode=0o755),
                "c": checkout_op(1),
                "r": action_payload("read", path=R0, channel="fuse"),
            },
            [{"id": "a", "actions": ["m", "c", "r"]}],
        )
        events = [
            ev("invoke", "m", 1.0),
            ev("ok", "m", 2.0, path=R0, mode=0o755),
            ev("invoke", "c", 3.0),
            ev("ok", "c", 4.0, revision=1, **{**THRIFT_OK, "conflicts": 1}),
            ev("invoke", "r", 5.0),
            ev("ok", "r", 6.0, **digest_facts(encode_committed(SEED, R0, TIP))),
        ]
        kept = (
            read_obs("o", R0, committed_value(R0, TIP), 5, 6),
            stat_obs("s", R0, True, 5, 6),
        )
        replaced = (read_obs("o", R0, committed_value(R0, 1), 5, 6),)
        self.assertTrue(is_explained(spec, events, kept))
        self.assertFalse(is_explained(spec, events, replaced))

    def _write_then_checkout(
        self, write: Event, via: str = "thrift", conflicts: int = 1
    ) -> tuple[RunSpec, list[Event]]:
        spec = run_spec(
            {
                "w": action_payload("write", path=R0, value_id="w"),
                "r": action_payload("read", path=R0, channel="fuse"),
                "c": checkout_op(1, via=via),
                "q": action_payload("read", path=R0, channel="fuse"),
            },
            [{"id": "a", "actions": ["w", "r", "c", "q"]}],
        )
        done = (
            ev("ok", "c", 6.0, revision=1, **{**THRIFT_OK, "conflicts": conflicts})
            if via == "thrift"
            else ev("ok", "c", 6.0, revision=1, **SL_OK)
        )
        return spec, [
            ev("invoke", "w", 1.0),
            write,
            ev("invoke", "r", 3.0),
            ev("ok", "r", 4.0, **digest_facts(encode_value(SEED, "a", "w"))),
            ev("invoke", "c", 5.0),
            done,
            ev("invoke", "q", 7.0),
            ev("ok", "q", 8.0, **digest_facts(b"x")),
            Event("run-closed", 9.0, {}),
        ]

    def test_an_overlapping_write_may_be_kept_or_superseded(self) -> None:
        """A timed-out write may be kept or superseded; neither is forced.

        The timed-out write is not visible before the checkout (`r` sees
        the source bytes), so it may land after the checkout applied the
        clean path (kept) or into the replaced inode (superseded). Landing
        before the visit instead is a conflict and keeps the write, so a
        superseded write is never counted.
        """
        timed_out = ev("fail", "w", 2.0, reason="pwrite", errno=errno.ETIMEDOUT)
        clean = read_obs("r", R0, committed_value(R0, TIP), 3, 4)
        for later, conflicts, explained in (
            (WRITTEN, 0, True),
            (WRITTEN, 1, True),
            (committed_value(R0, 1), 0, True),
            (committed_value(R0, 1), 1, False),
        ):
            with self.subTest(later=later, conflicts=conflicts):
                spec, events = self._write_then_checkout(timed_out, conflicts=conflicts)
                self.assertEqual(
                    explained,
                    is_explained(spec, events, (clean, read_obs("q", R0, later, 7, 8))),
                )

    def test_a_path_dirty_before_a_normal_checkout_is_never_superseded(self) -> None:
        """A path dirty when the checkout reaches it is kept.

        Only a path the checkout applied can be superseded, so a write
        visible before the checkout (timed out, or acked with a second
        write overlapping the checkout) survives it. A clean path with an
        overlapping write may still end at the destination.
        """
        destination = committed_value(R0, 1)
        timed_out = ev("fail", "w", 2.0, reason="pwrite", errno=errno.ETIMEDOUT)
        acked = ev("ok", "w", 2.0, bytes_written=64)
        for name, write in (("timed out", timed_out), ("acked", acked)):
            with self.subTest(write=name):
                spec, events = self._write_then_checkout(write)
                seen = read_obs("r", R0, WRITTEN, 3, 4)
                kept = (seen, read_obs("q", R0, WRITTEN, 7, 8))
                replaced = (seen, read_obs("q", R0, destination, 7, 8))
                self.assertTrue(is_explained(spec, events, kept))
                self.assertFalse(is_explained(spec, events, replaced))
        spec = run_spec(
            {
                "w0": action_payload("write", path=R0, value_id="w0"),
                "r": action_payload("read", path=R0, channel="fuse"),
                "c": checkout_op(1),
                "w1": action_payload("write", path=R0, value_id="w1"),
                "q": action_payload("read", path=R0, channel="fuse"),
            },
            [
                {"id": "a", "actions": ["w0", "r", "c", "q"]},
                {"id": "b", "actions": ["w1"]},
            ],
        )

        def overlapping(conflicts: int) -> list[Event]:
            return [
                ev("invoke", "c", 5.0),
                ev("invoke", "w1", 5.2),
                ev("ok", "w1", 5.4, bytes_written=64),
                ev("ok", "c", 6.0, revision=1, **{**THRIFT_OK, "conflicts": conflicts}),
                ev("invoke", "q", 7.0),
                ev("ok", "q", 8.0, **digest_facts(b"x")),
            ]

        events = [
            ev("invoke", "w0", 1.0),
            ev("ok", "w0", 2.0, bytes_written=64),
            ev("invoke", "r", 3.0),
            ev("ok", "r", 4.0, **digest_facts(b"x")),
            *overlapping(1),
        ]
        seen = read_obs("r", R0, file_value(encode_value(SEED, "a", "w0")), 3, 4)
        w1 = file_value(encode_value(SEED, "b", "w1"))
        self.assertTrue(is_explained(spec, events, (seen, read_obs("q", R0, w1, 7, 8))))
        self.assertFalse(
            is_explained(spec, events, (seen, read_obs("q", R0, destination, 7, 8)))
        )
        clean = run_spec(
            {
                "c": checkout_op(1),
                "w1": action_payload("write", path=R0, value_id="w1"),
                "q": action_payload("read", path=R0, channel="fuse"),
            },
            [{"id": "a", "actions": ["c", "q"]}, {"id": "b", "actions": ["w1"]}],
        )
        self.assertTrue(
            is_explained(clean, overlapping(0), (read_obs("q", R0, destination, 7, 8),))
        )

    def test_a_truncated_conflict_list_ends_at_the_more_line(self) -> None:
        """Sapling lists five conflicts, then `...and N more`."""
        registers = tuple(f"tree/registers/r{index}.reg" for index in range(7))
        actions = {
            f"w{index}": action_payload("write", path=path, value_id=f"w{index}")
            for index, path in enumerate(registers)
        }
        actions["c"] = checkout_op(1, via="goto")
        spec = run_spec(
            actions, [{"id": "a", "actions": list(actions)}], registers=registers
        )
        events = []
        for index in range(7):
            events += [
                ev("invoke", f"w{index}", 2.0 * index),
                ev("ok", f"w{index}", 2.0 * index + 1, bytes_written=64),
            ]
        events += [
            ev("invoke", "c", 20.0),
            ev("fail", "c", 21.0, **sl_abort_facts(1, *registers[:5], more=2)),
        ]
        facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
        self.assertEqual(registers[:5], conflict_paths(facts.action("c")))
        self.assertTrue(is_explained(spec, events))

    def test_abort_paths_are_observed_one_at_a_time(self) -> None:
        """The DRY_RUN walk is not a snapshot.

        r0 blocks (mode dirt) only until a concurrent chmod restores it, and
        r1 only after a concurrent write that starts after that chmod, so no
        single point sees both blocking; an abort listing r1 still needs
        r1 to block at some point.
        """
        actions = {
            "m": action_payload("chmod", path=R0, mode=0o755),
            "c": checkout_op(1, via="goto"),
            "n": action_payload("chmod", path=R0, mode=0o644),
            "v": action_payload("write", path=R1, value_id="v"),
        }
        events = [
            ev("invoke", "m", 1.0),
            ev("ok", "m", 2.0, path=R0, mode=0o755),
            ev("invoke", "c", 3.0),
            ev("invoke", "n", 4.0),
            ev("ok", "n", 5.0, path=R0, mode=0o644),
            ev("invoke", "v", 6.0),
            ev("ok", "v", 7.0, bytes_written=64),
            ev("fail", "c", 10.0, **sl_abort_facts(1, R0, R1)),
        ]
        spec = run_spec(
            actions,
            [{"id": "a", "actions": ["m", "c"]}, {"id": "b", "actions": ["n", "v"]}],
        )
        sink = contract_sink(spec, events)
        aborts = [
            o
            for o in sink.observations
            if ":abort:" in o.id and not o.id.endswith(":blocked")
        ]
        self.assertEqual([path_cell(R0), path_cell(R1)], [o.cells[1] for o in aborts])
        self.assertTrue(is_explained(spec, events))
        del actions["v"]
        spec = run_spec(
            actions,
            [{"id": "a", "actions": ["m", "c"]}, {"id": "b", "actions": ["n"]}],
        )
        self.assertFalse(is_explained(spec, events[:5] + events[7:]))

    def test_a_killed_sl_goto_and_a_later_checkout_may_both_apply(self) -> None:
        """Two checkout attempts may apply serially.

        The killed `sl goto`'s daemon checkout completes; a later Thrift
        checkout then applies after it. Parent-before-paths still orders
        them: the later checkout's paths read the parent it moved.
        """
        spec = checkout_spec(
            ("c", checkout_op(1, via="goto")),
            ("d", checkout_op(2)),
            extra={"r": action_payload("read", path=R0, channel="fuse")},
            actors=[{"id": "a", "actions": ["c", "r", "d"]}],
        )
        events = [
            ev("invoke", "c", 1.0),
            ev("fail", "c", 2.0, reason="goto", returncode=-9, stdout="", stderr=""),
            ev("invoke", "r", 3.0),
            ev("ok", "r", 4.0, **digest_facts(encode_committed(SEED, R0, 1))),
            ev("invoke", "d", 5.0),
            ev("ok", "d", 6.0, revision=2, **THRIFT_OK),
            Event("run-closed", 9.0, {}),
        ]
        self.assertTrue(
            is_explained(
                spec, events, (read_obs("r", R0, committed_value(R0, 1), 3, 4),)
            )
        )
        sink = contract_sink(spec, events)
        exclusive = Exclusive(
            ("g:c:parent", "g:d:parent"), ObligationRef("checkout.exclusive", "c,d")
        )
        self.assertFalse(
            engine_explains(
                EdenPinned.for_spec(spec),
                sink.groups,
                [*sink.observations, read_obs("r", R0, committed_value(R0, 1), 3, 4)],
                [*sink.constraints, exclusive],
            )
        )

    def _type_change(self) -> RunSpec:
        return run_spec(
            {
                "c": checkout_op(2),
                "w": action_payload("write", path=R0, value_id="w"),
                "d": checkout_op(3),
            },
            [{"id": "a", "actions": ["c", "w", "d"]}],
            type_changes=((R0, 3),),
        )

    def test_a_kept_dirty_file_keeps_its_type_changed_child_absent(self) -> None:
        """A NORMAL checkout keeps a dirty file, so it creates nothing under it."""
        spec = self._type_change()
        child = EdenPinned.for_spec(spec).fixture.layout.type_change_child(R0).path
        start = [
            ev("invoke", "c", 1.0),
            ev("ok", "c", 2.0, revision=2, **THRIFT_OK),
        ]
        written = [ev("invoke", "w", 3.0), ev("ok", "w", 4.0, bytes_written=64)]
        retype = [ev("invoke", "d", 5.0), ev("ok", "d", 6.0, revision=3, **THRIFT_OK)]
        absent = read_obs("e", child, ABSENT, 7, 8)
        present = read_obs("e", child, committed_child(spec, child), 7, 8)
        dirty = [*start, *written, *retype]
        self.assertTrue(
            is_explained(spec, dirty, (read_obs("r", R0, WRITTEN, 7, 8), absent))
        )
        self.assertFalse(
            is_explained(spec, dirty, (read_obs("r", R0, WRITTEN, 7, 8), present))
        )
        clean = run_spec(
            {"c": checkout_op(2), "d": checkout_op(3)},
            [{"id": "a", "actions": ["c", "d"]}],
            type_changes=((R0, 3),),
        )
        self.assertFalse(is_explained(clean, [*start, *retype], (absent,)))
        self.assertTrue(is_explained(clean, [*start, *retype], (present,)))

    def test_sl_rm_of_a_type_changed_directory_removes_its_children(self) -> None:
        spec = run_spec(
            {"h": action_payload("sl_rm", path=R0)},
            [{"id": "a", "actions": ["h"]}],
            type_changes=((R0, 3),),
        )
        child = EdenPinned.for_spec(spec).fixture.layout.type_change_child(R0).path
        events = [ev("invoke", "h", 1.0), ev("ok", "h", 2.0, path=R0)]
        self.assertTrue(
            is_explained(spec, events, (read_obs("b", child, ABSENT, 3, 4),))
        )
        self.assertFalse(
            is_explained(
                spec,
                events,
                (read_obs("b", child, committed_child(spec, child), 3, 4),),
            )
        )

    def test_a_directory_with_a_dirty_child_is_kept(self) -> None:
        """A checkout replaces a directory with a file only once its children are clean or gone."""
        spec = run_spec(
            {"c": checkout_op(2)},
            [{"id": "a", "actions": ["c"]}],
            type_changes=((R0, 3),),
        )
        child = EdenPinned.for_spec(spec).fixture.layout.type_change_child(R0).path
        sink = contract_sink(
            spec, [ev("invoke", "c", 1.0), ev("ok", "c", 2.0, revision=2, **THRIFT_OK)]
        )
        group = sink.by_id(f"g:c:{R0_PART}")
        guard = group.effects[0].guard
        assert guard is not None
        clean = committed_child(spec, child)
        for local, mode, replaced in (
            (clean, PLAIN, True),
            (ABSENT, ABSENT, True),
            (DIRTY, PLAIN, False),
            (clean, mode_value(True), False),
        ):
            values = {cell: ABSENT for cell in guard.cells}
            values.update(local_state(R0, DIR))
            values.update(local_state(child, local, mode))
            values[source_cell("k0", TIP)] = YES
            with self.subTest(local=local, mode=mode):
                self.assertEqual(replaced, guard_holds(guard, values))
        self.assertIn(path_cell(child), {effect.cell for effect in group.effects})

    def test_a_move_carries_a_type_changed_directory(self) -> None:
        """A renamed directory takes its relevant children with it."""
        spec = run_spec(
            {"m": action_payload("rename", source=R0, destination="tree/registers/m")},
            [{"id": "a", "actions": ["m"]}],
            type_changes=((R0, 3),),
        )
        child = EdenPinned.for_spec(spec).fixture.layout.type_change_child(R0).path
        moved = "tree/registers/m/" + child.rsplit("/", 1)[1]
        value = committed_child(spec, child)
        events = [
            ev("invoke", "m", 1.0),
            ev("ok", "m", 2.0, source=R0, destination="tree/registers/m"),
        ]
        self.assertTrue(
            is_explained(
                spec,
                events,
                (read_obs("a", moved, value, 3, 4), read_obs("b", child, ABSENT, 3, 4)),
            )
        )
        self.assertFalse(
            is_explained(spec, events, (read_obs("b", child, value, 3, 4),))
        )

    def test_missing_removed_and_mode_dirt_block_sl(self) -> None:
        """Sapling lists a path absent locally that the destination removes.

        That path (MISSING_REMOVED) blocks, and so does mode-only dirt; the
        committed entry does not.
        """
        spec = self._type_change()
        child = EdenPinned.for_spec(spec).fixture.layout.type_change_child(R0).path
        start = rev_value(TIP)
        events = [
            ev("invoke", "c", 1.0),
            ev("fail", "c", 2.0, **sl_abort_facts(2, child)),
        ]
        retarget = run_spec(
            {"c": checkout_op(2, via="goto")},
            [{"id": "a", "actions": ["c"]}],
            type_changes=((R0, 3),),
        )
        (listed,) = contract_sink(retarget, events).observations
        self.assertTrue(listed.expect.holds((start, ABSENT, ABSENT, CLEAN)))
        self.assertTrue(listed.expect.holds((start, ABSENT, ABSENT, REMOVED)))
        self.assertFalse(
            listed.expect.holds((start, committed_child(spec, child), PLAIN, CLEAN))
        )
        self.assertTrue(
            listed.expect.holds(
                (start, committed_child(spec, child), mode_value(True), CLEAN)
            )
        )

    def test_a_mode_only_change_in_the_destination_changes_the_path(self) -> None:
        """EdenFS compares SCM entries by blob and type: an exec-bit change is a change."""

        class ModeOnly(EdenPinned):
            def committed(self, path: str, revision: int) -> Value:
                return super().committed(path, TIP)

            def committed_mode(self, path: str, revision: int) -> Value:
                return mode_value(revision == 1)

        spec = checkout_spec(("c", checkout_op(1)))
        pinned = ModeOnly(EdenPinned.for_spec(spec).fixture)
        sink = contract_sink(
            spec,
            [ev("invoke", "c", 1.0), ev("ok", "c", 2.0, revision=1, **THRIFT_OK)],
            pinned,
        )
        group = sink.by_id(f"g:c:{R0_PART}")
        self.assertEqual(mode_value(True), group.effects[1].value)
        self.assertTrue(guard_applies(group, committed_value(R0, TIP)))
        self.assertFalse(guard_applies(group, DIRTY))

    def test_force_resets_the_mode_of_a_kept_path(self) -> None:
        """FORCE resets chmod-only dirt to the destination mode."""
        spec = run_spec(
            {
                "m": action_payload("chmod", path=R0, mode=0o755),
                "c": checkout_op(1, mode="force"),
            },
            [{"id": "a", "actions": ["m", "c"]}],
        )
        events = [
            ev("invoke", "m", 1.0),
            ev("ok", "m", 2.0, path=R0, mode=0o755),
            ev("invoke", "c", 3.0),
            ev("ok", "c", 4.0, revision=1, **{**THRIFT_OK, "conflicts": 1}),
        ]
        reset = (
            read_obs("o", R0, committed_value(R0, 1), 5, 6),
            stat_obs("s", R0, False, 5, 6),
        )
        self.assertTrue(is_explained(spec, events, reset))
        self.assertFalse(is_explained(spec, events, (stat_obs("s", R0, True, 5, 6),)))

    def test_thrift_conflict_counts_follow_the_mode_and_the_dirt(self) -> None:
        """NORMAL counts a dirty changed path, FORCE also a dirty kept one, and a
        clean path counts in neither mode; a lower count has no explanation."""
        for mode, revision, dirty, count in (
            ("normal", 1, True, 1),
            ("force", TIP, True, 1),
            ("force", 1, False, 0),
        ):
            actions = {"c": checkout_op(revision, mode=mode)}
            events = [ev("invoke", "c", 3.0)]
            if dirty:
                actions["w"] = action_payload("write", path=R0, value_id="w")
                events = [
                    ev("invoke", "w", 1.0),
                    ev("ok", "w", 2.0, bytes_written=64),
                    *events,
                ]
            spec = run_spec(actions, [{"id": "a", "actions": sorted(actions)[::-1]}])
            for recorded in range(count + 1):
                done = ev(
                    "ok",
                    "c",
                    4.0,
                    revision=revision,
                    **{**THRIFT_OK, "conflicts": recorded},
                )
                with self.subTest(mode=mode, dirty=dirty, recorded=recorded):
                    self.assertEqual(
                        recorded == count, is_explained(spec, [*events, done])
                    )

    def test_abort_rows_compare_against_the_source_revision(self) -> None:
        """A path blocks `sl goto` only when it differs from the revision it starts at.

        After `sl goto` to 2, r0 holds revision 2's bytes: clean, so an
        abort listing it is illegal. Chmod-only dirt on it makes the same
        abort legal.
        """
        actions = {
            "c0": checkout_op(2, via="goto"),
            "c": checkout_op(1, via="goto"),
        }
        clean = run_spec(actions, [{"id": "a", "actions": ["c0", "c"]}])
        dirty = run_spec(
            {**actions, "m": action_payload("chmod", path=R0, mode=0o755)},
            [{"id": "a", "actions": ["c0", "m", "c"]}],
        )
        moved = [ev("invoke", "c0", 1.0), ev("ok", "c0", 2.0, revision=2, **SL_OK)]
        chmod = [ev("invoke", "m", 3.0), ev("ok", "m", 4.0, path=R0, mode=0o755)]
        abort = [ev("invoke", "c", 5.0), ev("fail", "c", 6.0, **sl_abort_facts(1, R0))]
        self.assertFalse(is_explained(clean, [*moved, *abort]))
        self.assertTrue(is_explained(dirty, [*moved, *chmod, *abort]))

    def test_force_leaves_paths_neither_revision_holds(self) -> None:
        spec = checkout_spec(
            ("c", checkout_op(1, mode="force")),
            extra={"k": action_payload("mkdir", path="tree/registers/d")},
        )
        sink = contract_sink(
            spec,
            [
                ev("invoke", "c", 1.0),
                ev("ok", "c", 2.0, revision=1, **THRIFT_OK),
                ev("invoke", "k", 3.0),
                ev("ok", "k", 4.0, path="tree/registers/d"),
            ],
        )
        written = {
            effect.cell
            for group in sink.groups
            if group.action == "c"
            for effect in group.effects
        }
        self.assertIn(path_cell(R0), written)
        self.assertNotIn(path_cell("tree/registers/d"), written)

    def test_thrift_leaves_an_untracked_copy_of_the_destination(self) -> None:
        """An untracked file equal to the destination: Thrift keeps it; `sl goto` tracks it.

        Checking out back to the tip from revision 1 adds a generated path
        that revision 1 lacks.
        """
        probe = run_spec(
            {"g": action_payload("getattr", path=R0)}, [{"id": "a", "actions": ["g"]}]
        )
        layout = EdenPinned.for_spec(probe).fixture.layout
        added = layout.file_at(min(layout.changed_indices(TIP)), TIP).path
        for via, applies in (("thrift", False), ("goto", True)):
            spec = run_spec(
                {
                    "g": action_payload("getattr", path=added),
                    "c": checkout_op(1),
                    "d": checkout_op(TIP, via=via),
                },
                [{"id": "a", "actions": ["g", "c", "d"]}],
            )
            pinned = EdenPinned.for_spec(spec)
            self.assertEqual(ABSENT, pinned.committed(added, 1))
            copy = pinned.committed(added, TIP)
            done = THRIFT_OK if via == "thrift" else SL_OK
            sink = contract_sink(
                spec,
                [
                    ev("invoke", "g", 0.1),
                    ev("ok", "g", 0.2, **GETATTR_OK),
                    ev("invoke", "c", 1.0),
                    ev("ok", "c", 2.0, revision=1, **THRIFT_OK),
                    ev("invoke", "d", 3.0),
                    ev("ok", "d", 4.0, revision=TIP, **done),
                ],
            )
            part = "p" + hashlib.sha256(added.encode()).hexdigest()[:16]
            group = sink.by_id(f"g:d:{part}")
            with self.subTest(via=via):
                self.assertFalse(guard_applies(group, copy, 1))
                self.assertTrue(guard_applies(group, ABSENT, 1))
                if via == "goto":
                    planned = next(
                        effect
                        for effect in sink.by_id(f"g:d:{part}:dryrun").effects
                        if effect.cell.space == "planned"
                    )
                    assert planned.guard is not None
                    self.assertEqual(
                        applies, guard_holds(planned.guard, local_state(added, copy))
                    )

    def test_the_interrupted_advice_is_a_typed_rejection(self) -> None:
        spec = checkout_spec(("c", checkout_op(1, via="goto")))
        stderr = (
            "abort: EdenError: a previous checkout was interrupted - please run "
            "'sl go 1' to resume it\n"
        )
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
        facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
        self.assertEqual(Resolution.NONE, resolution(facts.action("c")))
        rules = CONTRACT_RULES - {IN_PROGRESS_RULE}
        # The refusal may come from NORMAL, after DRY_RUN wrote `.hg/updatestate`.
        self.assertEqual(
            {
                ("g:c:updatestate", Status.ACKED),
                ("g:c:updatestate:written", Status.INDETERMINATE),
            },
            {
                (group.id, group.status)
                for group in contract_sink(spec, events, rules=rules).groups
            },
        )
        counterfactual = [
            group
            for group in contract_sink(spec, events).groups
            if not group.id.startswith("g:c:updatestate")
        ]
        self.assertTrue(counterfactual)
        self.assertTrue(
            all(
                group.requires[0].obligation.rule_id == IN_PROGRESS_RULE
                for group in counterfactual
            )
        )


def committed_child(spec: RunSpec, child: str) -> Value:
    return EdenPinned.for_spec(spec).committed(child, TIP)


def sl_gap(op: dict[str, Any]) -> RunSpec:
    return run_spec(
        {"c": checkout_op(2, via="goto"), "x": op},
        [{"id": "a", "actions": ["c"]}, {"id": "b", "actions": ["x"]}],
        metadata=WALK_OVERLAP,
    )


class SlGotoTwoPhaseTest(unittest.TestCase):
    """An operation between `sl goto`'s DRY_RUN and NORMAL phases is kept."""

    def _race(
        self, op: dict[str, Any], done: dict[str, Any]
    ) -> tuple[RunSpec, list[Event]]:
        events = [
            ev("invoke", "c", 1.0),
            ev("invoke", "x", 2.0),
            ev("ok", "x", 3.0, **done),
            ev("ok", "c", 4.0, revision=2, **SL_OK),
        ]
        return sl_gap(op), events

    def test_an_unlink_in_the_gap_stays_absent(self) -> None:
        """Kept: the path's visit saw it present, so it neither blocks nor is
        re-created (an unlink before the visit blocks instead)."""
        spec, events = self._race(action_payload("unlink", path=R0), {"path": R0})
        self.assertTrue(is_explained(spec, events, (read_obs("g", R0, ABSENT, 5, 6),)))


class ExtraEntries(EdenPinned):
    """The fixture plus `extra`: each path's value at the revisions that hold it,
    absent at the others; the run starts at each path's TIP value."""

    def __init__(self, spec: RunSpec, extra: dict[str, dict[int, Value]]) -> None:
        super().__init__(EdenPinned.for_spec(spec).fixture)
        self.extra = extra

    def committed(self, path: str, revision: int) -> Value:
        if path not in self.extra:
            return super().committed(path, revision)
        return self.extra[path].get(revision, ABSENT)

    def committed_mode(self, path: str, revision: int) -> Value:
        if path not in self.extra:
            return super().committed_mode(path, revision)
        value = self.committed(path, revision)
        return ABSENT if value == ABSENT else mode_value(value == DIR)

    def initial(self, cell: Cell) -> Value:
        if cell.key in self.extra and cell == path_cell(cell.key):
            return self.committed(cell.key, self.initial_revision)
        if cell.key in self.extra and cell == mode_cell(cell.key):
            return self.committed_mode(cell.key, self.initial_revision)
        return super().initial(cell)


# A directory revision 2 removes, holding one file.
REMOVED_DIR = "tree/n"
KEPT = "tree/n/f.dat"
REMOVED_TREE: dict[str, dict[int, Value]] = {
    REMOVED_DIR: {TIP: DIR},
    KEPT: {TIP: file_value(b"f")},
}


class DirectoryNotEmptyCountTest(unittest.TestCase):
    """A Thrift checkout's count includes one DIRECTORY_NOT_EMPTY per directory
    the destination removes or retypes while NORMAL keeps a path below it
    (`TreeInode::finalizeDirectoryRemoval`)."""

    def _history(
        self, op: dict[str, Any], done: dict[str, Any], *, mode: str = "normal"
    ) -> tuple[RunSpec, list[Event]]:
        spec = run_spec(
            {"x": op, "c": checkout_op(2, mode=mode)},
            [{"id": "a", "actions": ["x", "c"]}],
            registers=(R0, R1, op["path"]),
        )
        return spec, [
            ev("invoke", "x", 1.0),
            ev("ok", "x", 2.0, **done),
            ev("invoke", "c", 3.0),
        ]

    def _write(self, path: str = KEPT, **kwargs: Any) -> tuple[RunSpec, list[Event]]:
        return self._history(
            action_payload("write", path=path, value_id="w"),
            {"bytes_written": 64},
            **kwargs,
        )

    def _judged(
        self,
        history: tuple[RunSpec, list[Event]],
        tree: dict[str, dict[int, Value]],
        counts: range,
    ) -> dict[int, bool]:
        """Return, per recorded count, whether it has an explanation."""
        spec, events = history
        pinned = ExtraEntries(spec, tree)
        found = {}
        for count in counts:
            recorded = [
                *events,
                ev("ok", "c", 4.0, revision=2, **{**THRIFT_OK, "conflicts": count}),
            ]
            facts = HistoryFacts.build(spec, recorded, VOCAB_TARGET)
            self.assertIsNone(count_judgment(spec, facts, pinned, "c").gap)
            sink = contract_sink(spec, recorded, pinned)
            found[count] = engine_explains(
                pinned, sink.groups, sink.observations, sink.constraints
            )
        return found

    def test_a_kept_child_adds_its_removed_directory(self) -> None:
        """MODIFIED_REMOVED for the dirty child plus DIRECTORY_NOT_EMPTY for its
        directory; a count without the directory has no explanation."""
        self.assertEqual(
            {1: False, 2: True, 3: False},
            self._judged(self._write(), REMOVED_TREE, range(1, 4)),
        )

    def test_a_removed_directory_left_empty_is_not_reported(self) -> None:
        """A clean child is removed with its directory (0); an unlinked one is
        MISSING_REMOVED alone (1)."""
        clean = self._history(action_payload("getattr", path=KEPT), {"size": 1})
        unlinked = self._history(action_payload("unlink", path=KEPT), {"path": KEPT})
        for history, expected in (
            (clean, {0: True, 1: False}),
            (unlinked, {0: False, 1: True, 2: False}),
        ):
            with self.subTest(op=history[0].action("x").kind):
                self.assertEqual(
                    expected, self._judged(history, REMOVED_TREE, range(len(expected)))
                )

    def test_nested_removed_directories_each_count_once(self) -> None:
        """The kept child keeps `tree/n/d`, which keeps `tree/n`: both are reported."""
        nested = {
            REMOVED_DIR: {TIP: DIR},
            "tree/n/d": {TIP: DIR},
            "tree/n/d/f.dat": {TIP: file_value(b"f")},
        }
        self.assertEqual(
            {2: False, 3: True, 4: False},
            self._judged(self._write("tree/n/d/f.dat"), nested, range(2, 5)),
        )

    def test_a_directory_turned_into_a_file_counts_like_a_removed_one(self) -> None:
        retyped = {**REMOVED_TREE, REMOVED_DIR: {TIP: DIR, 2: file_value(b"n")}}
        self.assertEqual(
            {1: False, 2: True, 3: False},
            self._judged(self._write(), retyped, range(1, 4)),
        )

    def test_force_removes_the_kept_child_and_its_directory(self) -> None:
        self.assertEqual(
            {0: False, 1: True, 2: False},
            self._judged(self._write(mode="force"), REMOVED_TREE, range(3)),
        )

    def test_an_untracked_child_leaves_the_count_unjudged(self) -> None:
        """A path neither revision holds may keep the directory: no count is derived."""
        spec, events = self._write("tree/n/u.dat")
        recorded = [*events, ev("ok", "c", 4.0, revision=2, **THRIFT_OK)]
        gap = count_judgment(
            spec,
            HistoryFacts.build(spec, recorded, VOCAB_TARGET),
            ExtraEntries(spec, {**REMOVED_TREE, "tree/n/u.dat": {}}),
            "c",
        ).gap
        self.assertIsNotNone(gap)
        self.assertIn("tree/n/u.dat", gap or "")

    def test_a_class_representative_below_a_removed_directory_bounds_the_count(
        self,
    ) -> None:
        """Starting dirt on indexes 2 (`tree/a0/r3`) and 99 (`tree/a1/r3`) gives
        one dirty representative of weight 2, kept below removed directories:
        EdenFS reports 2 MODIFIED_REMOVED and 2 DIRECTORY_NOT_EMPTY. Members'
        directories are not enumerated, so the other member may add 0 or 1."""
        spec = run_spec(
            {"s": action_payload("status", channel="thrift"), "c": checkout_op(2)},
            [{"id": "a", "actions": ["s", "c"]}],
            file_count=200,
            scatter_dirty=100,
        )
        pinned = EdenPinned.for_spec(spec)
        found = {}
        for count in range(2, 6):
            events = [
                ev("invoke", "s", 1.0),
                ev("ok", "s", 2.0, status=[], parent="n"),
                ev("invoke", "c", 3.0),
                ev("ok", "c", 4.0, revision=2, **{**THRIFT_OK, "conflicts": count}),
            ]
            facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
            self.assertEqual(
                CountJudgment(None, 2), count_judgment(spec, facts, pinned, "c")
            )
            sink = contract_sink(spec, events, pinned)
            (observation,) = [o for o in sink.observations if o.id == "o:c:conflicts"]
            self.assertEqual(
                ("k0/tree/a0/r3/00002.dat", "k0/tree/a0/r3"),
                tuple(cell.key for cell in observation.cells),
            )
            found[count] = engine_explains(
                pinned, sink.groups, sink.observations, sink.constraints
            )
        self.assertEqual({2: False, 3: True, 4: True, 5: False}, found)
