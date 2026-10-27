# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Published generation rates and distributions, measured over seed ranges.

A band around a published rate is the module constant plus or minus a
tolerance: forced and Thrift NORMAL checkout shares, the paused-readdir
decoration, and repeated-producer tails. Checkouts revisit a revision at a
small measured rate, repeated-producer tails keep the rest of the spec, a
continue follows only an interruptible checkout, normal checkouts succeed
and conflict at useful rates, and actor widths, contention, and mid-flight
installs each keep a floor.
"""

from __future__ import annotations

import dataclasses
import unittest
from collections import Counter
from typing import Any

from eden.integration.correctness.exploratory.core.generate import (
    compose,
    FAULT_REPEATED_PRODUCERS,
    FAULT_REPEATED_SHARE,
    FRESH_PRODUCERS,
    generation_summary,
    held_inputs,
    may_overlap,
    must_end_before,
    PRODUCER_STRATUM,
    REPEATED_PRODUCER_RATE,
    REPEATED_PRODUCERS,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    FINISHED,
    RunSpec,
    STARTED,
    validate,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    CHECKOUT_CONTINUE,
    forbid_overlap,
    FORCE_CHECKOUT_RATE,
    THRIFT_NORMAL_RATE,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    RECORDING_TARGET,
    RECORDING_TARGET as EDENFS_TARGET,
)
from eden.integration.correctness.exploratory.fake.target import FAKE
from eden.integration.correctness.exploratory.test.campaign_fixtures import (
    campaign_specs,
)
from eden.integration.correctness.exploratory.test.generation_fixtures import (
    action_owners,
    classify_checkouts,
    compose_sampled,
    fault_window,
    generate_specs,
    repeated_coverage,
    sampled_environment,
)


def _planned_in_fault(shape: Any, seed: int) -> RunSpec | None:
    """Compose one seed with a planned fault-window tail; None when the target refuses it."""
    try:
        return next(shape(range(seed, seed + 1), producers=FAULT_REPEATED_PRODUCERS))
    except ValueError as error:
        if "declines a fault-window tail" not in str(error):
            raise
        return None


class ComposedRatesTest(unittest.TestCase):
    def test_checkout_specs_carry_paused_readdir_stream_at_published_rate(
        self,
    ) -> None:
        """About half of the checkout-targeted specs hold a stream across the checkout."""
        checkouts = decorated = 0
        for seed in range(1000):
            spec = compose_sampled(EDENFS_TARGET, seed)
            streams = [a for a in spec.actions if a.kind == "readdir_stream"]
            if spec.action("i0").kind != "checkout":
                self.assertEqual([], streams)
                continue
            checkouts += 1
            if not streams:
                continue
            decorated += 1
            resume = spec.action("d1")
            self.assertEqual("d0", streams[0].id)
            self.assertEqual(
                ("readdir_resume", "d0"), (resume.kind, resume.payload["stream"])
            )
            self.assertIn(("d0", FINISHED), spec.action("i0").requires)
            owner = next(a.actions for a in spec.actors if "i0" in a.actions)
            start = owner.index("d0")
            self.assertEqual(("d0", "i0", "d1"), owner[start : start + 3])
            validate(spec, EDENFS_TARGET)
        self.assertGreater(checkouts, 0)
        (held,) = held_inputs(EDENFS_TARGET)
        self.assertAlmostEqual(
            held.probability,
            decorated / checkouts,
            delta=0.15,
            msg=(decorated, checkouts),
        )

    def test_actor_width_distribution(self) -> None:
        """A fault-window producer tail runs on two actors of its own."""
        widths = [
            len(compose_sampled(EDENFS_TARGET, seed, producers=FRESH_PRODUCERS).actors)
            for seed in range(2000)
        ]
        self.assertEqual({2, 3, 4, 8}, set(widths))
        for width in (2, 3, 4, 8):
            self.assertGreaterEqual(widths.count(width), 100)

    def test_contended_and_uncontended_paths_each_exceed_twenty_percent(
        self,
    ) -> None:
        contended = 0
        for seed in range(500):
            spec = compose_sampled(EDENFS_TARGET, seed)
            owners = action_owners(spec)
            path_owners: dict[str, set[str]] = {}
            for action in spec.actions:
                if action.kind == "write":
                    path_owners.setdefault(action.payload["path"], set()).add(
                        owners[action.id]
                    )
            contended += any(len(users) > 1 for users in path_owners.values())
        self.assertGreaterEqual(contended, 100)
        self.assertGreaterEqual(500 - contended, 100)

    def test_mid_flight_installs_exceed_twenty_percent(self) -> None:
        installs = [
            next(
                action
                for action in compose_sampled(EDENFS_TARGET, seed).actions
                if action.kind == "fault_install"
            )
            for seed in range(500)
        ]
        mid_flight = sum(
            (action.payload["target_action"], "started") in action.requires
            for action in installs
        )
        self.assertGreaterEqual(mid_flight, 100)


def _organic_revisits(spec: RunSpec) -> tuple[int, int]:
    """Return (checkouts outside the tail, those naming the tip or a repeated revision).

    A `--continue` names the revision of the checkout it resumes by
    construction (`CONTINUE_RATE`), so it is no revisit and is not counted.
    """
    tip = spec.environment.target["repository"]["commit_count"] - 1
    revisions = [
        action.payload["revision"]
        for action in spec.actions
        if action.kind == "checkout"
        and not action.id.startswith("v")
        and action.payload["mode"] != CHECKOUT_CONTINUE
    ]
    return len(revisions), len(revisions) - len(set(revisions) - {tip})


def _without_tail(spec: RunSpec) -> RunSpec:
    """Drop the tail actions, actors left empty, their edges, and metadata."""
    actors = tuple(
        Actor(actor.id, tuple(a for a in actor.actions if not a.startswith("v")))
        for actor in spec.actors
    )
    return dataclasses.replace(
        spec,
        metadata={},
        actors=tuple(actor for actor in actors if actor.actions),
        actions=tuple(
            dataclasses.replace(
                action,
                requires=tuple(
                    (dependency, event)
                    for dependency, event in action.requires
                    if not dependency.startswith("v")
                ),
            )
            for action in spec.actions
            if not action.id.startswith("v")
        ),
    )


class RevisitCalibrationTest(unittest.TestCase):
    def test_checkouts_revisit_at_a_small_measured_rate(self) -> None:
        """Outside the tails, 7 to 18 percent of checkouts repeat a revision.

        `REVISIT_RATE` (12.5 percent) returns to the tip or a composed
        checkout's revision; the rest pick an unvisited revision until the
        three non-tip revisions run out.
        """
        for shape in (campaign_specs, generate_specs):
            checkouts = revisits = 0
            for spec in shape(range(800), producers=FRESH_PRODUCERS):
                total, repeated = _organic_revisits(spec)
                checkouts += total
                revisits += repeated
            with self.subTest(shape=shape.__name__, revisits=revisits, of=checkouts):
                self.assertTrue(0.07 <= revisits / checkouts <= 0.18)

    def test_unplanned_mix_keeps_repeated_producers_outside_tails(self) -> None:
        """Fresh specs alone repeat producers, and revisits run inside fault windows.

        Before organic revisits, fresh specs gave 25 repeated producers in
        878 nightly specs and the tail ran after every fault, so fault
        windows held almost no revisit.
        """
        for shape, floors in (
            (campaign_specs, {"fresh-repeated": 0.06, "window-revisits": 0.05}),
            (generate_specs, {"fresh-repeated": 0.10, "window-revisits": 0.15}),
        ):
            coverage = repeated_coverage(shape(range(800)))
            with self.subTest(shape=shape.__name__, coverage=dict(coverage)):
                for name, floor in floors.items():
                    self.assertGreaterEqual(coverage[name] / coverage["specs"], floor)
                self.assertGreater(coverage["tail-window-revisits"], 0)
                self.assertGreater(
                    coverage["window-revisits"], 2 * coverage["tail-window-revisits"]
                )
        coverage = repeated_coverage(generate_specs(range(800), no_fault=True))
        self.assertGreaterEqual(coverage["fresh-repeated"] / coverage["specs"], 0.08)

    def test_tails_leave_the_rest_of_the_spec_unchanged(self) -> None:
        """The producer stratum draws from its own streams."""
        compared = 0
        for shape in (campaign_specs, generate_specs):
            for seed in range(120):
                fresh, repeated = (
                    next(shape(range(seed, seed + 1), producers=producers))
                    for producers in (FRESH_PRODUCERS, REPEATED_PRODUCERS)
                )
                in_fault = _planned_in_fault(shape, seed)
                with self.subTest(shape=shape.__name__, seed=seed):
                    self.assertEqual(_without_tail(fresh), _without_tail(repeated))
                    if in_fault is not None:
                        self.assertEqual(_without_tail(fresh), _without_tail(in_fault))
                    self.assertNotEqual(fresh, repeated)
                    compared += 1
        self.assertEqual(240, compared)

    def test_repeated_producer_stratum_shape(self) -> None:
        """Every tail reproduces the initial value; some keep both as candidates.

        An earlier producer of the observed path that finishes before the
        tail (a checkout, a write or chmod of it, a move to or from it, an
        `sl rm` of it) supersedes the initial state, so then only the tail's
        return repeats the initial value.
        """
        tails = repeated = 0
        for shape in (campaign_specs, generate_specs):
            for spec in shape(range(150), producers=REPEATED_PRODUCERS):
                with self.subTest(seed=spec.seed, shape=shape.__name__):
                    tails += 1
                    repeated += self._check_tail(spec)
        self.assertGreaterEqual(repeated / tails, 0.3, (repeated, tails))

    def _check_legs(self, spec: RunSpec) -> None:
        tip = spec.environment.target["repository"]["commit_count"] - 1
        depart, back, during, after = (
            spec.action(action_id) for action_id in ("v0", "v1", "v2", "v3")
        )
        self.assertEqual(
            ("thrift", "force"), (depart.payload["via"], depart.payload["mode"])
        )
        self.assertNotEqual(tip, depart.payload["revision"])
        self.assertEqual(
            (tip, "thrift", "normal"),
            (back.payload["revision"], back.payload["via"], back.payload["mode"]),
        )
        self.assertIn(during.kind, {"read", "getattr"})
        self.assertIn(after.kind, {"read", "getattr"})
        before = must_end_before(spec)
        self.assertLessEqual(
            {("v0", "v2"), ("v1", "v2")}, set(may_overlap(spec, before))
        )
        self.assertIn("v1", before["v3"])
        validate(spec, RECORDING_TARGET)

    def _check_tail(self, spec: RunSpec) -> bool:
        self._check_legs(spec)
        during = spec.action("v2")
        owners = {
            action_id: actor.id for actor in spec.actors for action_id in actor.actions
        }
        last = spec.actors[-1].actions
        self.assertEqual(("v0", "v1", "v3"), last[-3:])
        self.assertEqual("v2", spec.actors[-2].actions[-1])
        self.assertNotEqual(owners["v0"], owners["v2"])
        others = {action.id for action in spec.actions if not action.id.startswith("v")}
        before = must_end_before(spec)
        for action_id in ("v0", "v2"):
            self.assertEqual(others, set(before[action_id]) & others)
        self.assertEqual(REPEATED_PRODUCERS, spec.metadata[PRODUCER_STRATUM])
        summary = generation_summary(spec, RECORDING_TARGET)
        self.assertEqual(1, summary["strata:repeated"])
        self.assertEqual(0, summary["strata:repeated-in-fault"])
        self.assertGreaterEqual(summary["producers:revisits-initial"], 2)
        path = during.payload["path"]
        superseded = any(
            action.kind == "checkout"
            or (action.kind in ("write", "chmod") and action.payload["path"] == path)
            or (
                action.kind in ("rename", "sl_mv", "sl_rm")
                and path in action.payload.values()
            )
            for action in spec.actions
            if action.id in others
        )
        if not superseded:
            self.assertGreaterEqual(summary["producers:repeated"], 1)
        return summary["producers:repeated"] >= 1

    def test_fault_window_tail_runs_while_the_fault_is_armed(self) -> None:
        """The tail starts after the install and the removal cannot wait on a parked leg.

        Block faults gate the tail on the target being parked and let the
        removal wait only for `v0` and `v2` to start; other faults gate it on
        the target finishing and remove only after the tail. Sites whose
        validators reserve tree-wide actions, and parked checkout targets,
        decline the tail, and a planned tail there is refused. A checkout
        tail ordered against a remove or transfer (`forbid_overlap`) may
        leave the window.
        """
        placed: Counter[str] = Counter()
        for shape in (campaign_specs, generate_specs):
            for fresh in shape(range(300), producers=FRESH_PRODUCERS):
                install = next(a for a in fresh.actions if a.kind == "fault_install")
                target_action = fresh.action(install.payload["target_action"])
                block = install.payload["behavior"] == "block"
                declined = install.payload["site"] in (
                    "fetch-tree",
                    "fuse-lookup",
                ) or (block and target_action.kind == "checkout")
                spec = _planned_in_fault(shape, fresh.seed)
                with self.subTest(seed=fresh.seed, shape=shape.__name__):
                    if declined:
                        self.assertIsNone(spec)
                        placed["declined"] += 1
                        continue
                    assert spec is not None
                    stratum = spec.metadata[PRODUCER_STRATUM]
                    self.assertEqual(FAULT_REPEATED_PRODUCERS, stratum)
                    if any(
                        forbid_overlap(spec, spec.action(tail), other)
                        for tail in ("v0", "v1")
                        for other in spec.actions
                    ):
                        placed["ordered"] += 1
                        continue
                    self._check_fault_tail(spec, install, block)
                    placed["block" if block else "unparked"] += 1
        self.assertGreater(placed["block"], 30, placed)
        self.assertGreater(placed["unparked"], 30, placed)
        self.assertGreater(placed["declined"], 30, placed)

    def _check_fault_tail(self, spec: RunSpec, install: Action, block: bool) -> None:
        self._check_legs(spec)
        self.assertEqual(("v0", "v1", "v3"), spec.actors[-2].actions)
        self.assertEqual(("v2",), spec.actors[-1].actions)
        before = must_end_before(spec)
        window = fault_window(spec, before)
        self.assertLessEqual({"v0", "v1", "v2", "v3"}, window)
        remove = next(a for a in spec.actions if a.kind == "fault_remove")
        if block:
            parked = next(a.id for a in spec.actions if a.kind == "fault_await")
            self.assertIn(parked, before["v0"])
            self.assertLessEqual(
                {("v0", STARTED), ("v2", STARTED)}, set(remove.requires)
            )
            for leg in ("v0", "v1", "v2", "v3"):
                self.assertNotIn(leg, before[remove.id])
        else:
            self.assertIn(install.payload["target_action"], before["v0"])
            self.assertLessEqual({"v0", "v1", "v2", "v3"}, before[remove.id])
        summary = generation_summary(spec, RECORDING_TARGET)
        self.assertEqual(1, summary["strata:repeated-in-fault"])
        self.assertEqual(1, summary["strata:repeated"])
        self.assertGreaterEqual(summary["producers:revisits-initial"], 1)

    def test_unplanned_composition_draws_repeated_producers_at_the_published_rate(
        self,
    ) -> None:
        drawn = Counter(
            spec.metadata[PRODUCER_STRATUM] for spec in generate_specs(range(2000))
        )
        tails = drawn[REPEATED_PRODUCERS] + drawn[FAULT_REPEATED_PRODUCERS]
        self.assertTrue(
            REPEATED_PRODUCER_RATE - 0.03
            <= tails / 2000
            <= REPEATED_PRODUCER_RATE + 0.03,
            drawn,
        )
        # Some fault-window draws fall back to the trailing tail.
        in_fault = REPEATED_PRODUCER_RATE * FAULT_REPEATED_SHARE
        self.assertTrue(
            0.02 <= drawn[FAULT_REPEATED_PRODUCERS] / 2000 <= in_fault, drawn
        )
        no_fault = Counter(
            spec.metadata[PRODUCER_STRATUM]
            for spec in generate_specs(range(500), no_fault=True)
        )
        self.assertEqual(0, no_fault[FAULT_REPEATED_PRODUCERS])

    def test_a_target_without_a_progressive_producer_never_revisits(self) -> None:
        for seed in range(200):
            spec = compose(FAKE, seed, {})
            self.assertEqual(FRESH_PRODUCERS, spec.metadata[PRODUCER_STRATUM])
        for producers in (REPEATED_PRODUCERS, FAULT_REPEATED_PRODUCERS):
            with self.assertRaisesRegex(ValueError, "cannot revisit"):
                compose(FAKE, 0, {}, producers=producers)
        with self.assertRaisesRegex(ValueError, "unknown producer stratum"):
            compose(RECORDING_TARGET, 0, sampled_environment(0), producers="sometimes")
        with self.assertRaisesRegex(ValueError, "needs a fault"):
            compose(
                RECORDING_TARGET,
                0,
                sampled_environment(0),
                no_fault=True,
                producers=FAULT_REPEATED_PRODUCERS,
            )


class ConflictCalibrationTest(unittest.TestCase):
    """Normal checkouts still both succeed and conflict now that registers change."""

    def test_normal_checkouts_succeed_and_conflict_at_useful_rates(self) -> None:
        for shape in (campaign_specs, generate_specs):
            outcomes: Counter[str] = Counter()
            channels: Counter[str] = Counter()
            for spec in shape(range(600), producers=FRESH_PRODUCERS):
                for checkout, outcome in classify_checkouts(spec):
                    outcomes[outcome] += 1
                    channels[f"{checkout.payload['via']}:{outcome}"] += 1
            normal = sum(
                outcomes[name]
                for name in ("trivial", "clean", "conflict", "maybe-conflict")
            )
            with self.subTest(shape=shape.__name__, outcomes=dict(outcomes)):
                self.assertLessEqual(outcomes["trivial"] / normal, 0.2)
                self.assertGreaterEqual(outcomes["clean"] / normal, 0.3)
                self.assertGreaterEqual(outcomes["conflict"] / normal, 0.3)
                self.assertGreater(channels["thrift:clean"], 10)
                self.assertGreater(channels["goto:clean"], 10)
                self.assertGreater(channels["goto:conflict"], 10)
                # Forced checkout-site attempts start clean, so typed
                # Thrift conflicts are rarer in the nightly shape.
                self.assertGreater(
                    channels["thrift:conflict"], 10 if shape is generate_specs else 2
                )

    def test_thrift_normal_and_forced_rates(self) -> None:
        counts: Counter[str] = Counter()
        for spec in generate_specs(range(1000), producers=FRESH_PRODUCERS):
            killed = {
                action.payload["target_action"]
                for action in spec.actions
                if action.kind == "kill_process"
            }
            for action in spec.actions:
                if action.kind == "checkout" and action.id not in killed:
                    counts[f"{action.payload['via']}/{action.payload['mode']}"] += 1
        total = sum(counts.values())
        normal = counts["thrift/normal"] + counts["goto/normal"]
        self.assertAlmostEqual(
            FORCE_CHECKOUT_RATE, counts["thrift/force"] / total, delta=0.08, msg=counts
        )
        self.assertAlmostEqual(
            THRIFT_NORMAL_RATE, counts["thrift/normal"] / normal, delta=0.07, msg=counts
        )

    def test_mixed_strata_keep_the_forced_and_sl_goto_shares(self) -> None:
        """With the tails, forced checkouts stay near the pre-tail share.

        Before the tails (1000 specs): forced 21.5 percent of
        nightly and 23.6 percent of `generate` checkouts; `sl goto` 59.8
        and 76.4 percent. Outside the tails, `sl` (`sl goto` and the
        `sl go --continue` resumes that replace some) keeps that share;
        every tail adds two Thrift checkouts.
        """
        for shape, forced_share, update_share in (
            (campaign_specs, 0.215, 0.598),
            (generate_specs, 0.236, 0.764),
        ):
            counts: Counter[str] = Counter()
            for spec in shape(range(1000)):
                for action in spec.actions:
                    if action.kind != "checkout":
                        continue
                    channel = f"{action.payload['via']}/{action.payload['mode']}"
                    counts[channel] += 1
                    if not action.id.startswith("v"):
                        counts[f"organic:{channel}"] += 1
            total = sum(v for k, v in counts.items() if ":" not in k)
            organic = sum(v for k, v in counts.items() if k.startswith("organic:"))
            sl = counts["organic:goto/normal"] + counts["organic:goto/continue"]
            with self.subTest(shape=shape.__name__, counts=dict(counts)):
                self.assertLess(
                    abs(counts["thrift/force"] / total - forced_share), 0.04
                )
                self.assertLess(abs(sl / organic - update_share), 0.04)


class ContinueCalibrationTest(unittest.TestCase):
    def test_continue_follows_only_an_interruptible_checkout(self) -> None:
        """`sl go --continue` resumes a checkout an error fault or a kill targets.

        The resume terminates on EdenFS, so the composer
        samples it, and only where such a checkout exists, for its revision,
        on either channel.
        """
        resumed = 0
        channels: Counter[str] = Counter()
        for shape in (campaign_specs, generate_specs):
            for spec in shape(range(800)):
                targets = {
                    action.payload.get("target_action")
                    for action in spec.actions
                    if action.kind in ("fault_install", "kill_process")
                }
                for action in spec.actions:
                    if (
                        action.kind != "checkout"
                        or action.payload["mode"] != "continue"
                    ):
                        continue
                    resumed += 1
                    with self.subTest(seed=spec.seed, shape=shape.__name__):
                        found = [
                            other.payload["via"]
                            for other in spec.actions
                            if other.kind == "checkout"
                            and other.id in targets
                            and other.payload["mode"] != "continue"
                            and other.payload["revision"] == action.payload["revision"]
                        ]
                        self.assertTrue(found)
                        channels["thrift" if set(found) == {"thrift"} else "sl"] += 1
        self.assertGreaterEqual(resumed, 16)
        self.assertGreater(channels["thrift"], 0)
        self.assertGreater(channels["sl"], 0)
