# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The generation summary, generated-spec bounds, and hash-seed determinism.

The summary's effect classes come from action declarations; it counts
interaction families and producers (a producer that starts after the observer
ends is no candidate; a revisit repeats the initial value) and covers the core
families deterministically. Generated specs fit the default budgets and
observation bounds, and specs and summaries are identical across Python hash
seeds.
"""

from __future__ import annotations

import json
import unittest

from eden.integration.correctness.exploratory.core.generate import (
    declared_families,
    effect_class,
    generation_summary,
    summarize_generation,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEFAULT_BUDGETS,
    FINISHED,
    STARTED,
    to_json,
    validate,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    run_in_fresh_interpreter,
)
from eden.integration.correctness.exploratory.edenfs.actions import produced_value
from eden.integration.correctness.exploratory.edenfs.observation_bounds import (
    validate_observation_sizes,
)
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from eden.integration.correctness.exploratory.test.campaign_fixtures import (
    campaign_specs,
)
from eden.integration.correctness.exploratory.test.generation_fixtures import (
    actor_spec,
    checkout_action,
    generate_specs,
    REGISTERS,
)


class GenerationSummaryTest(unittest.TestCase):
    def test_effect_classes_come_from_declarations(self) -> None:
        classes = {
            kind: effect_class(definition)
            for kind, definition in RECORDING_TARGET.actions.items()
        }
        expected = {
            "checkout": "progressive",
            "rename": "transfer",
            "sl_mv": "transfer",
            "unlink": "remove",
            "sl_rm": "remove",
            "write": "produce",
            "chmod": "produce",
            "mkdir": "produce",
            "read": "observe",
            "getattr": "observe",
            "open": "observe",
            "read_fd": "observe",
            "readdir": "observe-aggregate",
            "readdir_stream": "observe-aggregate",
            "readdir_resume": "observe-aggregate",
            "status": "observe-aggregate",
            "status_counts": "observe-aggregate",
            "manifest_presence": "observe-aggregate",
            "journal_window": "observe-aggregate",
        }
        self.assertEqual(expected, {k: v for k, v in classes.items() if v is not None})
        self.assertEqual(18, len(declared_families(RECORDING_TARGET)))
        self.assertIn("produce~transfer", declared_families(RECORDING_TARGET))

    def test_summary_counts_families_and_producers(self) -> None:
        r0, r1 = REGISTERS

        def write(action_id: str, path: str) -> Action:
            return Action(
                action_id,
                "write",
                "operation",
                payload={"path": path, "value_id": action_id},
            )

        read = Action(
            "x0", "read", "operation", payload={"path": r0, "channel": "fuse"}
        )
        status = Action("s0", "status", "operation", payload={"channel": "sl"})
        spec = actor_spec(
            {
                "a0": [write("w0", r0), checkout_action("c0", 2, "normal")],
                "a1": [write("w1", r0), status],
                "a2": [read, write("w2", r1)],
            }
        )
        summary = generation_summary(spec, RECORDING_TARGET)
        nonzero = {k: v for k, v in summary.items() if v}
        self.assertEqual(
            {
                # w0~w1 (r0); w1~w2 and w0~w2 touch different registers.
                "family:produce~produce": 1,
                # x0 reads r0 while w0 and w1 may run; c0 waits for w0 only.
                "family:observe~produce": 2,
                "family:observe~progressive": 1,
                # c0 and w1, c0 and w2, c0 and s0.
                "family:produce~progressive": 2,
                "family:observe-aggregate~progressive": 1,
                # s0 follows w1; w0 and w2 may run.
                "family:observe-aggregate~produce": 2,
                # x0 sees r0 from the initial state, w0, w1, or c0; s0 sees r0
                # from w1 (finished), w0, c0 and r1 from w2, c0 or the start.
                "producers:4": 1,
                "producers:3": 2,
                "producers:ambiguous": 3,
                # c0 and the initial state differ (revision 2 vs the tip).
                "producers:repeated": 0,
            },
            nonzero | {"producers:repeated": summary["producers:repeated"]},
        )

    def test_a_producer_that_starts_after_the_observer_ends_is_no_candidate(
        self,
    ) -> None:
        r0 = REGISTERS[0]
        read = Action(
            "x0", "read", "operation", payload={"path": r0, "channel": "fuse"}
        )
        later = Action(
            "w0",
            "write",
            "operation",
            requires=(("x0", FINISHED),),
            payload={"path": r0, "value_id": "w0"},
        )
        concurrent = Action(
            "w1",
            "write",
            "operation",
            requires=(("x0", STARTED),),
            payload={"path": r0, "value_id": "w1"},
        )
        summary = generation_summary(
            actor_spec({"a0": [read], "a1": [later]}), RECORDING_TARGET
        )
        self.assertEqual(
            (1, 0), (summary["producers:1"], summary["producers:ambiguous"])
        )
        # A started edge orders only starts: w1 may still run during x0.
        summary = generation_summary(
            actor_spec({"a0": [read], "a1": [concurrent]}), RECORDING_TARGET
        )
        self.assertEqual(
            (1, 1), (summary["producers:2"], summary["producers:ambiguous"])
        )

    def test_a_revisit_repeats_the_initial_value(self) -> None:
        tip = 3
        read = Action(
            "x0",
            "read",
            "operation",
            payload={"path": REGISTERS[0], "channel": "fuse"},
        )
        back = Action(
            "c1",
            "checkout",
            "operation",
            payload={"revision": tip, "via": "thrift", "mode": "normal"},
        )
        spec = actor_spec({"a0": [checkout_action("c0", 1), back], "a1": [read]})
        self.assertEqual(("revision", tip), produced_value(spec, None, REGISTERS[0]))
        summary = generation_summary(spec, RECORDING_TARGET)
        self.assertEqual(1, summary["producers:3"])
        self.assertEqual(1, summary["producers:repeated"])

    def test_summary_is_deterministic_and_covers_core_families(self) -> None:
        first = summarize_generation(campaign_specs(range(300)), RECORDING_TARGET)
        second = summarize_generation(campaign_specs(range(300)), RECORDING_TARGET)
        self.assertEqual(first, second)
        self.assertEqual(list(first), sorted(first))
        for name in (
            "produce~produce",
            "observe~produce",
            "produce~progressive",
            "observe~progressive",
            "observe-aggregate~progressive",
            "progressive~progressive",
        ):
            self.assertGreater(first[f"family:{name}"], 20, name)
        self.assertGreater(first["producers:ambiguous"], 100)
        self.assertGreater(first["producers:repeated"], 10)
        self.assertGreater(first["strata:repeated"], 10)


class GenerationBoundsTest(unittest.TestCase):
    def test_generated_specs_fit_default_budgets_and_observation_bounds(self) -> None:
        for shape in (campaign_specs, generate_specs):
            for spec in shape(range(500)):
                self.assertEqual(DEFAULT_BUDGETS, spec.budgets)
                validate_observation_sizes(spec)
                validate(spec, RECORDING_TARGET)
        for spec in generate_specs(range(200), no_fault=True):
            self.assertEqual(DEFAULT_BUDGETS, spec.budgets)
            validate_observation_sizes(spec)


_HASHSEED_PROGRAM = """
import hashlib, json
from eden.integration.correctness.exploratory.core.runspec import to_json
from eden.integration.correctness.exploratory.core.generate import summarize_generation
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from {campaign} import campaign_specs
from {generation} import generate_specs
digest = hashlib.sha256()
specs = [
    *campaign_specs(range(60)),
    *generate_specs(range(60)),
    *generate_specs(range(30), no_fault=True),
]
for spec in specs:
    digest.update(to_json(spec).encode())
print(json.dumps({{
    "specs": digest.hexdigest(),
    "summary": summarize_generation(specs, RECORDING_TARGET),
}}, sort_keys=True))
"""


class HashSeedDeterminismTest(unittest.TestCase):
    def test_specs_and_summary_are_identical_across_hash_seeds(self) -> None:
        program = _HASHSEED_PROGRAM.format(
            campaign=campaign_specs.__module__, generation=generate_specs.__module__
        )
        outputs = {
            seed: json.loads(run_in_fresh_interpreter(program, seed))
            for seed in ("0", "1", "4242")
        }
        self.assertEqual(
            1, len({json.dumps(o, sort_keys=True) for o in outputs.values()})
        )
        self.assertEqual(
            summarize_generation(
                [
                    *campaign_specs(range(60)),
                    *generate_specs(range(60)),
                    *generate_specs(range(30), no_fault=True),
                ],
                RECORDING_TARGET,
            ),
            outputs["0"]["summary"],
        )
        in_process = [to_json(spec) for spec in campaign_specs(range(3))]
        self.assertEqual(
            in_process, [to_json(spec) for spec in campaign_specs(range(3))]
        )
