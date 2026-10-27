# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Differential and held-descriptor composition by the generator: generated specs pass
the validator, plain composition over a reference environment is rejected,
composition is deterministic and independent of the hash seed, held descriptors reach
their sequenced families, and no held open follows a path remover.
"""

from __future__ import annotations

import dataclasses
import unittest
from collections import Counter
from collections.abc import Iterable
from unittest import mock

from eden.integration.correctness.exploratory.core import generate
from eden.integration.correctness.exploratory.core.differential_schedule import (
    Ordering,
    schedule_problems,
)
from eden.integration.correctness.exploratory.core.effects import footprints_interact
from eden.integration.correctness.exploratory.core.generate import (
    compose,
    derived_rng,
    effect_class,
    family,
    MUTATOR_CLASSES,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    may_overlap,
    must_end_before,
    RunSpec,
    SpecError,
    to_json,
    validate,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    run_in_fresh_interpreter,
)
from eden.integration.correctness.exploratory.edenfs.actions import expects_reference
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    sample_differential_environment,
    sample_environment,
    with_reference_clone,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import footprint_of


# Acceptance the differential composer must reach without retries.
MIN_DIFFERENTIAL_ACCEPTANCE = 0.95


DIFFERENTIAL_SEEDS = range(300)


def _differential_spec(seed: int) -> RunSpec:
    environment = sample_differential_environment(
        derived_rng(seed, "differential-environment")
    )
    return compose(EDENFS_TARGET, seed, environment, differential=True)


def sequenced_families(spec: RunSpec) -> Counter[str]:
    """Count ordered pairs with interacting footprints and a mutator, by family."""
    ordering = Ordering(spec)
    found: Counter[str] = Counter()
    actions = list(spec.actions)
    for index, first in enumerate(actions):
        for second in actions[index + 1 :]:
            left = effect_class(EDENFS_TARGET.actions[first.kind])
            right = effect_class(EDENFS_TARGET.actions[second.kind])
            if left is None or right is None:
                continue
            if left not in MUTATOR_CLASSES and right not in MUTATOR_CLASSES:
                continue
            footprints = [
                EDENFS_TARGET.actions[action.kind].footprint(spec, action)
                for action in (first, second)
            ]
            if ordering.ordered(first.id, second.id) and footprints_interact(
                *footprints
            ):
                found[family(left, right)] += 1
    return found


class DifferentialCompositionTest(unittest.TestCase):
    def test_generated_specs_pass_the_validator(self) -> None:
        accepted: list[RunSpec] = []
        rejected: Counter[str] = Counter()
        for seed in DIFFERENTIAL_SEEDS:
            try:
                accepted.append(_differential_spec(seed))
            except SpecError as error:
                rejected[str(error)[:80]] += 1
        rate = len(accepted) / len(DIFFERENTIAL_SEEDS)
        families: Counter[str] = Counter()
        kinds: Counter[str] = Counter()
        for spec in accepted:
            families.update(sequenced_families(spec))
            kinds.update(action.kind for action in spec.actions)
            validate(from_json(to_json(spec), EDENFS_TARGET), EDENFS_TARGET)
            self.assertEqual((), schedule_problems(spec, footprint_of))
            self.assertTrue(all(expects_reference(spec, a) for a in spec.actions))
        self.assertEqual(1.0, rate, dict(rejected))
        for name in (
            "observe~produce",
            "observe~remove",
            "observe~transfer",
            "observe-aggregate~produce",
            "observe~progressive",
            "produce~progressive",
        ):
            self.assertGreater(families[name], 0, name)

    def test_plain_composition_over_a_reference_environment_is_rejected(self) -> None:
        """Without ordering, composition rarely yields an admissible schedule."""
        accepted = 0
        for seed in range(60):
            environment = with_reference_clone(
                {
                    **sample_environment(derived_rng(seed, "differential-environment")),
                    "case_sensitive": True,
                }
            )
            try:
                compose(EDENFS_TARGET, seed, environment, no_fault=True)
            except (SpecError, ValueError):
                continue
            accepted += 1
        self.assertLess(accepted, 60 * MIN_DIFFERENTIAL_ACCEPTANCE)

    def test_differential_composition_is_deterministic(self) -> None:
        for seed in range(10):
            self.assertEqual(
                to_json(_differential_spec(seed)), to_json(_differential_spec(seed))
            )

    def test_differential_composition_ignores_the_hash_seed(self) -> None:
        script = (
            "import hashlib\n"
            "from eden.integration.correctness.exploratory.core.runspec import to_json\n"
            "from eden.integration.correctness.exploratory.test import "
            "generate_compose_test as t\n"
            "digest = hashlib.sha256()\n"
            "for seed in range(40):\n"
            "    digest.update(to_json(t._differential_spec(seed)).encode())\n"
            "print(digest.hexdigest())\n"
        )
        digests = {
            run_in_fresh_interpreter(script, seed).strip().splitlines()[-1]
            for seed in ("0", "1", "4242")
        }
        self.assertEqual(1, len(digests), digests)

    def test_non_differential_composition_ignores_the_reference_switch(self) -> None:
        """A plain environment keeps every checkout variant the sampler draws."""
        vias: Counter[str] = Counter()
        for seed in range(200):
            environment = sample_environment(derived_rng(seed, "plain-environment"))
            spec = compose(EDENFS_TARGET, seed, environment, no_fault=True)
            vias.update(
                action.payload["via"]
                for action in spec.actions
                if action.kind == "checkout"
            )
        self.assertGreater(vias["thrift"], 0)


def descriptor_families(specs: Iterable[RunSpec]) -> Counter[str]:
    """Count each `read_fd` and mutator pair whose footprints interact, by family and schedule."""
    found: Counter[str] = Counter()
    for spec in specs:
        overlapping = {frozenset(pair) for pair in may_overlap(spec)}
        for read in spec.actions:
            if read.kind != "read_fd":
                continue
            reads = footprint_of(spec, read)
            for other in spec.actions:
                left = effect_class(EDENFS_TARGET.actions[read.kind])
                right = effect_class(EDENFS_TARGET.actions[other.kind])
                if right not in MUTATOR_CLASSES or left is None:
                    continue
                if not footprints_interact(reads, footprint_of(spec, other)):
                    continue
                schedule = (
                    "concurrent"
                    if frozenset({read.id, other.id}) in overlapping
                    else "ordered"
                )
                found[f"{family(left, right)}:{schedule}"] += 1
    return found


class HeldDescriptorCompositionTest(unittest.TestCase):
    """The composer holds a descriptor across an actor's inputs (`HELD_DESCRIPTOR`)."""

    def _specs(self, differential: bool) -> list[RunSpec]:
        specs = []
        for seed in range(300):
            if differential:
                specs.append(_differential_spec(seed))
            else:
                environment = sample_environment(derived_rng(seed, "plain-environment"))
                specs.append(compose(EDENFS_TARGET, seed, environment, no_fault=True))
        return specs

    def test_held_descriptors_reach_their_families(self) -> None:
        for differential in (False, True):
            with self.subTest(differential=differential):
                specs = self._specs(differential)
                held = [s for s in specs if any(a.kind == "open" for a in s.actions)]
                families = descriptor_families(held)
                self.assertGreater(len(held), len(specs) // 8)
                for spec in held:
                    kinds = Counter(a.kind for a in spec.actions)
                    self.assertEqual(1, kinds["open"])
                    self.assertEqual(1, kinds["read_fd"])
                    self.assertEqual(1, kinds["close"])
                self.assertGreater(families["observe~produce:ordered"], 0)
                if differential:
                    self.assertEqual(
                        0, sum(n for k, n in families.items() if "concurrent" in k)
                    )
                else:
                    self.assertGreater(families["observe~produce:concurrent"], 0)

    def test_fault_specs_hold_no_descriptor_and_plain_specs_list_nothing(
        self,
    ) -> None:
        for seed in range(100):
            environment = sample_environment(derived_rng(seed, "plain-environment"))
            for no_fault in (False, True):
                spec = compose(EDENFS_TARGET, seed, environment, no_fault=no_fault)
                kinds = {action.kind for action in spec.actions}
                self.assertNotIn("readdir", kinds)
                if not no_fault:
                    self.assertNotIn("open", kinds)

    def test_a_declined_draw_leaves_the_spec_unchanged(self) -> None:
        """Specs without a descriptor equal composition without the family."""
        bare_target = dataclasses.replace(
            EDENFS_TARGET,
            generation=dataclasses.replace(
                EDENFS_TARGET.generation, held_descriptor=None
            ),
        )
        for seed in range(60):
            environment = sample_environment(derived_rng(seed, "plain-environment"))
            spec = compose(EDENFS_TARGET, seed, environment, no_fault=True)
            if any(action.kind == "open" for action in spec.actions):
                continue
            bare = compose(bare_target, seed, environment, no_fault=True)
            self.assertEqual(to_json(bare), to_json(spec))

    def test_differential_listings_name_a_known_directory(self) -> None:
        listed = Counter(
            action.payload["path"]
            for seed in range(300)
            for action in _differential_spec(seed).actions
            if action.kind == "readdir"
        )
        self.assertGreater(sum(listed.values()), 0)
        self.assertEqual({"tree/registers"}, set(listed))

    def test_no_held_open_may_follow_a_path_remover(self) -> None:
        """An open a remove or move of its path may precede can fail."""
        for differential in (False, True):
            with self.subTest(differential=differential):
                found = []
                for spec in self._specs(differential):
                    ended = must_end_before(spec)
                    for opened in (a for a in spec.actions if a.kind == "open"):
                        reads = footprint_of(spec, opened)
                        found += [
                            (spec.seed, other.id)
                            for other in spec.actions
                            if other.kind in ("unlink", "rename", "sl_rm", "sl_mv")
                            and opened.id not in ended[other.id]
                            and footprints_interact(footprint_of(spec, other), reads)
                        ]
                self.assertEqual([], found)

    def test_the_composer_declines_a_held_open_after_a_remover(self) -> None:
        """Seed 100 drew an open of r1 after `sl rm r1`."""
        environment = sample_environment(derived_rng(100, "plain-environment"))
        spec = compose(EDENFS_TARGET, 100, environment, no_fault=True)
        self.assertNotIn("open", {action.kind for action in spec.actions})
        with mock.patch.object(generate, "_removed_before_open", return_value=False):
            unchecked = compose(EDENFS_TARGET, 100, environment, no_fault=True)
        (opened,) = [a for a in unchecked.actions if a.kind == "open"]
        self.assertIn(
            ("sl_rm", opened.payload["path"]),
            {(a.kind, a.payload.get("path")) for a in unchecked.actions},
        )
