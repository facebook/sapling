# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The EdenFS checker target as a whole: its properties in order, red controls as the
union of the rules' controls, contract inputs support only
registered rules, and each red-control rule's recorded live capture
trips it while the same shape without the control accepts.
"""

from __future__ import annotations

import importlib.resources
import json
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.history import event_from_json
from eden.integration.correctness.exploratory.core.property import control_rules
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    SERVING_PATHS,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    RECORDING_TARGET,
)
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    COMPLETED,
    violations_of,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    R0,
    R1,
    read_ok,
    Run,
    WRITE_OK,
)

RED_CONTROL_RULES = {
    "torn-write": "bytes.torn-read",
    "status-loss": "status.status-diff-mismatch",
    "diff-loss": "status.status-diff-mismatch",
    "common-status-loss": "status.sl-fuse-stale-status",
}


class TargetTest(unittest.TestCase):
    def test_target_is_the_ported_slice(self) -> None:
        self.assertEqual(
            (
                "bytes",
                "status",
                "journal",
                "cross-channel",
                "namespace",
                "checkout",
                "existence",
                "manifest",
                "listing",
                "result-shape",
                "fd",
                "attributes",
                "readdir",
                "differential",
            ),
            tuple(p.id for p in EDENFS_TARGET.properties),
        )
        self.assertEqual(
            frozenset(control_rules(EDENFS_TARGET.properties)),
            EDENFS_TARGET.red_controls,
        )
        self.assertEqual(set(RED_CONTROL_RULES), set(EDENFS_TARGET.red_controls))
        self.assertEqual(SERVING_PATHS, EDENFS_TARGET.serving_paths)
        self.assertEqual((), RECORDING_TARGET.properties)
        self.assertEqual(RECORDING_TARGET.actions, EDENFS_TARGET.actions)
        self.assertIsNotNone(EDENFS_TARGET.timeout_capable)
        self.assertIs(
            RECORDING_TARGET.install_witnessed, EDENFS_TARGET.install_witnessed
        )

    def test_contract_inputs_support_only_registered_rules(self) -> None:
        """Contract obligations of unregistered rules are dropped (a missed-signal window)."""
        run = Run("t", "rename-then-write", "overlapping write and rename")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("b", "m", "rename", source=R0, destination=R1)
        run.inv("w", "m")
        run.done("m", "ok", source=R0, destination=R1)
        run.done("w", "ok", **WRITE_OK)
        run.finish()
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        facts = checker.HistoryFacts.build(
            spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
        )
        rule_ids = {
            rule.id
            for p in checker.registry(EDENFS_TARGET)
            for rule in p.rules.values()
        }
        derived = checker.contract_effects(EDENFS_TARGET, spec, facts, rule_ids)
        assert not isinstance(derived, checker.PropertyError)
        refs = derived[1].contract_obligations()
        self.assertEqual(
            set(), {ref.rule_id for ref in refs} & (CONTRACT_RULES - rule_ids)
        )


class RedControlCaptureTest(unittest.TestCase):
    """Histories the live backend recorded under each red control (red_control_live_test)."""

    def _capture(self, control: str) -> tuple[Any, list[Any]]:
        root = importlib.resources.files(__package__).joinpath("red_controls", control)
        spec = from_json(root.joinpath("run-spec.json").read_text(), EDENFS_TARGET)
        events = [
            event_from_json(json.loads(line))
            for line in root.joinpath("history.jsonl").read_text().splitlines()
        ]
        return spec, events

    def test_every_red_control_rule_has_a_tripping_capture(self) -> None:
        control_rules = {
            rule.id
            for definition in EDENFS_TARGET.properties
            for rule in definition.rules.values()
            if rule.red_controls
        }
        self.assertLessEqual(control_rules, set(RED_CONTROL_RULES.values()))
        for control, rule_id in sorted(RED_CONTROL_RULES.items()):
            with self.subTest(control=control):
                spec, events = self._capture(control)
                verdict = checker.check(
                    EDENFS_TARGET, spec, events, execution=COMPLETED
                )
                self.assertEqual(checker.REJECT, verdict.kind)
                violated = {key[0] for key in violations_of(verdict)}
                self.assertIn(rule_id, violated)

    def test_without_the_control_the_same_shape_accepts(self) -> None:
        """The capture's twin: the control's fault removed, nothing is violated."""
        for control in sorted(RED_CONTROL_RULES):
            with self.subTest(control=control):
                spec, events = self._capture(control)
                repaired = [_repair(spec, event) for event in events]
                verdict = checker.check(
                    EDENFS_TARGET, spec, repaired, execution=COMPLETED
                )
                self.assertEqual({}, violations_of(verdict))


def _repair(spec: Any, event: Any) -> Any:
    """Return an event with the observation a red control damaged made whole again."""
    if event.type != "ok" or event.data.get("action") != "o":
        return event
    data = dict(event.data)
    if spec.action("o").kind == "read":
        data.update(read_ok(encode_value(spec.seed, "a", "red-w")))
    else:
        entries = {item["path"]: item["code"] for item in data["status"]}
        entries[spec.action("w").payload["path"]] = "M"
        data["status"] = [{"path": p, "code": entries[p]} for p in sorted(entries)]
        data["diff_paths"] = sorted({*data["diff_paths"], *entries})
    return type(event)(event.type, event.time, data)
