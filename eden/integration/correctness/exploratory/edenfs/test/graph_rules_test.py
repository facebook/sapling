# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""RunSpec graph rules under the EdenFS vocabulary.

Every `requires` entry names a declared action with a known edge kind, and the
graph of explicit edges plus actor order is acyclic; every action belongs to
exactly one actor; a `run` deadline is only for actions that may outlast an
operation. Fault installs, awaits, and removes pair by fault ID with legal
lifetimes and orders (a block is awaited, a remove never waits on its own
target, nothing deadlocks by construction); a fault names a declared site and a
behavior that site supports, and one key has at most one install. A process
kill targets a started action that spawns the process it names. Each rule has an
accepted and a rejected spec.
"""

from __future__ import annotations

import copy
import json
import unittest
from dataclasses import replace

from eden.integration.correctness.exploratory.core import graph_rules
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    from_json as _from_json,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.core.sites import Site
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    VOCAB_TARGET,
    WORKED_EXAMPLE,
)


def from_json(text: str) -> RunSpec:
    return _from_json(text, VOCAB_TARGET)


def _worked_dict() -> dict:
    return json.loads(WORKED_EXAMPLE)


def _without_actions(raw: dict, *action_ids: str) -> dict:
    for action_id in action_ids:
        del raw["actions"][action_id]
    for actor in raw["actors"]:
        actor["actions"] = [
            action_id for action_id in actor["actions"] if action_id not in action_ids
        ]
    for action in raw["actions"].values():
        if "requires" in action:
            action["requires"] = [
                requirement
                for requirement in action["requires"]
                if requirement[0] not in action_ids
            ]
    return raw


def _nonblock_fault(behavior: str) -> dict:
    raw = _without_actions(_worked_dict(), "h1")
    raw["actions"]["f1"]["behavior"] = behavior
    raw["actions"]["c0"]["deadline"] = "operation"
    raw["actions"]["w1"].pop("requires", None)
    raw["actions"]["x1"]["requires"] = [["c0", "finished"]]
    return raw


class GraphRulesTest(unittest.TestCase):
    def test_kill_process_target_must_spawn_and_be_started_dep(self) -> None:
        raw = _worked_dict()
        raw["actions"]["k1"] = {
            "kind": "kill_process",
            "deadline": "operation",
            "target_action": "r1",
            "after_ms": 1,
            "requires": [["r1", "started"]],
        }
        raw["actors"].append({"id": "killer", "actions": ["k1"]})
        with self.assertRaisesRegex(SpecError, "does not spawn a process"):
            from_json(json.dumps(raw))

        raw["actions"]["k1"]["target_action"] = "c0"
        raw["actions"]["k1"].pop("requires")
        with self.assertRaisesRegex(SpecError, "started"):
            from_json(json.dumps(raw))

        raw["actions"]["k1"]["requires"] = [["c0", "started"]]
        self.assertEqual(
            "c0", from_json(json.dumps(raw)).action("k1").payload["target_action"]
        )

    def test_process_kill_binding_matches_spawned_payload(self) -> None:
        accepted = (
            ("status", {"channel": "sl"}),
            ("checkout", {"revision": 2, "via": "goto", "mode": "normal"}),
            ("checkout", {"revision": 2, "via": "goto", "mode": "normal"}),
        )
        rejected = (
            ("status", {"channel": "thrift"}),
            ("checkout", {"revision": 2, "via": "thrift", "mode": "normal"}),
        )

        for kind, payload in accepted:
            with self.subTest(kind=kind, payload=payload):
                raw = _worked_dict()
                raw["actions"]["p1"] = {
                    "kind": kind,
                    "deadline": "operation",
                    "requires": [["h1", "finished"]],
                    **payload,
                }
                raw["actions"]["k1"] = {
                    "kind": "kill_process",
                    "deadline": "operation",
                    "target_action": "p1",
                    "after_ms": 1,
                    "requires": [["p1", "started"]],
                }
                raw["actors"].extend(
                    (
                        {"id": "process", "actions": ["p1"]},
                        {"id": "killer", "actions": ["k1"]},
                    )
                )
                self.assertEqual(
                    "p1",
                    from_json(json.dumps(raw)).action("k1").payload["target_action"],
                )

        for kind, payload in rejected:
            with self.subTest(kind=kind, payload=payload):
                raw = _worked_dict()
                raw["actions"]["p1"] = {
                    "kind": kind,
                    "deadline": "operation",
                    **payload,
                }
                raw["actions"]["k1"] = {
                    "kind": "kill_process",
                    "deadline": "operation",
                    "target_action": "p1",
                    "after_ms": 1,
                    "requires": [["p1", "started"]],
                }
                raw["actors"].extend(
                    (
                        {"id": "process", "actions": ["p1"]},
                        {"id": "killer", "actions": ["k1"]},
                    )
                )
                with self.assertRaisesRegex(SpecError, "spawn"):
                    from_json(json.dumps(raw))

    def test_install_pre_armed_ok(self) -> None:
        raw = _nonblock_fault("error")
        self.assertEqual(
            "error", from_json(json.dumps(raw)).action("f1").payload["behavior"]
        )

    def test_install_mid_flight_ok(self) -> None:
        raw = _nonblock_fault("delay")
        raw["actions"]["c0"].pop("requires")
        raw["actions"]["f1"]["requires"] = [["c0", "started"]]
        self.assertEqual(
            "delay", from_json(json.dumps(raw)).action("f1").payload["behavior"]
        )

    def test_install_unordered_rejects(self) -> None:
        raw = _nonblock_fault("error")
        raw["actions"]["c0"].pop("requires")
        with self.assertRaisesRegex(SpecError, "install.*finished.*target.*started"):
            from_json(json.dumps(raw))

    def test_block_requires_await(self) -> None:
        raw = _without_actions(_worked_dict(), "h1")
        raw["actions"]["w1"].pop("requires", None)
        raw["actions"]["x1"]["requires"] = [["c0", "started"]]
        with self.assertRaisesRegex(SpecError, "block.*wait"):
            from_json(json.dumps(raw))

    def test_unsupported_behavior_for_site_rejects(self) -> None:
        raw = _nonblock_fault("unsupported")
        with self.assertRaisesRegex(SpecError, "does not support behavior"):
            from_json(json.dumps(raw))

    def test_two_faults_distinct_keys_may_overlap(self) -> None:
        raw = _nonblock_fault("error")
        raw["actions"].update(
            {
                "f2": {
                    "kind": "fault_install",
                    "deadline": "operation",
                    "fault_id": "G",
                    "target_action": "s9",
                    "site": "status-cache-fill",
                    "behavior": "error",
                },
                "s9": {
                    "kind": "status",
                    "deadline": "operation",
                    "channel": "sl",
                    "requires": [["f2", "finished"]],
                },
                "x2": {
                    "kind": "fault_remove",
                    "deadline": "operation",
                    "fault_id": "G",
                    "requires": [["s9", "finished"]],
                },
            }
        )
        raw["actors"].extend(
            [
                {"id": "nemesis2", "actions": ["f2", "x2"]},
                {"id": "probe", "actions": ["s9"]},
            ]
        )
        self.assertEqual("status", from_json(json.dumps(raw)).action("s9").kind)

    def test_same_key_overlap_rejects(self) -> None:
        raw = _nonblock_fault("error")
        raw["actions"].update(
            {
                "f2": {
                    **raw["actions"]["f1"],
                    "fault_id": "G",
                    "target_action": "c1",
                },
                "c1": {
                    **raw["actions"]["c0"],
                    "requires": [["f2", "finished"]],
                },
                "x2": {
                    "kind": "fault_remove",
                    "deadline": "operation",
                    "fault_id": "G",
                    "requires": [["c1", "finished"]],
                },
            }
        )
        raw["actors"].extend(
            [
                {"id": "nemesis2", "actions": ["f2", "x2"]},
                {"id": "checkout2", "actions": ["c1"]},
            ]
        )
        with self.assertRaisesRegex(SpecError, "fault key"):
            from_json(json.dumps(raw))

    def test_unknown_site_rejects(self) -> None:
        raw = _worked_dict()
        raw["actions"]["f1"]["site"] = "unknown"
        with self.assertRaisesRegex(SpecError, "unsupported site"):
            from_json(json.dumps(raw))

    def test_site_payload_on_another_kind_is_not_a_fault_install(self) -> None:
        spec = from_json(WORKED_EXAMPLE)
        actions = tuple(
            replace(a, payload={**a.payload, "site": "unknown"}) if a.id == "w1" else a
            for a in spec.actions
        )
        spec = replace(spec, actions=actions)
        try:
            graph_rules._validate_sites(
                spec, VOCAB_TARGET, {action.id: action for action in actions}
            )
        except SpecError as error:
            self.fail(f"a write's site payload was judged as a fault install: {error}")

    def test_site_validate_is_called(self) -> None:
        def reject(_spec: RunSpec, _install: Action, _target: Action) -> None:
            raise SpecError("site validation called")

        site = Site(
            key_class="checkout",
            target_kinds=frozenset({"checkout"}),
            behaviors=frozenset({"block"}),
            daemon="public-master",
            validate=reject,
            effect="pre",
        )
        target = replace(VOCAB_TARGET, sites={"checkout-begin": site})
        with self.assertRaisesRegex(SpecError, "site validation called"):
            _from_json(WORKED_EXAMPLE, target)

    def test_two_installs_one_key_reject(self) -> None:
        raw = _worked_dict()
        raw["actions"]["f2"] = {
            **raw["actions"]["f1"],
            "fault_id": "G",
        }
        raw["actions"]["h2"] = {
            "kind": "fault_await",
            "deadline": "operation",
            "fault_id": "G",
            "requires": [["c0", "started"]],
        }
        raw["actions"]["x2"] = {
            "kind": "fault_remove",
            "deadline": "operation",
            "fault_id": "G",
            "requires": [["h2", "finished"]],
        }
        raw["actions"]["c0"]["requires"].append(["f2", "finished"])
        raw["actors"][0]["actions"] = ["f1", "f2", "h1", "h2", "x1", "x2"]
        with self.assertRaisesRegex(SpecError, "fault key"):
            from_json(json.dumps(raw))

        raw["actions"]["c0"]["requires"] = [["f1", "finished"]]
        raw["actions"]["c1"] = {
            **raw["actions"]["c0"],
            "requires": [["f2", "finished"]],
        }
        raw["actions"]["f2"].update(
            {
                "target_action": "c1",
                "requires": [["x1", "finished"], ["c0", "finished"]],
            }
        )
        raw["actions"]["h2"]["requires"] = [["c1", "started"]]
        raw["actors"][0]["actions"] = ["f1", "h1", "x1", "f2", "h2", "x2"]
        raw["actors"][1]["actions"].append("c1")
        self.assertEqual("checkout", from_json(json.dumps(raw)).action("c1").kind)

    def test_one_ordinal_install_accepts(self) -> None:
        raw = _worked_dict()
        raw["actions"]["f1"]["ordinal"] = 2
        spec = from_json(json.dumps(raw))
        self.assertEqual(2, spec.action("f1").payload["ordinal"])

    def test_bad_requires_shape_rejects(self) -> None:
        bad_entries = [
            ["h1"],
            ["h1", "finished", "extra"],
            ["h1", "begun"],
            "h1",
            [["h1", "finished"]],
        ]
        for entry in bad_entries:
            with self.subTest(entry=entry):
                raw = _worked_dict()
                raw["actions"]["w1"]["requires"] = [entry]
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_unknown_requires_target_rejects(self) -> None:
        raw = _worked_dict()
        raw["actions"]["w1"]["requires"] = [["nope", "finished"]]
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))

    def test_explicit_cycle_rejects(self) -> None:
        raw = _worked_dict()
        raw["actions"]["f1"]["requires"] = [["x1", "finished"]]
        raw["actors"][0]["actions"] = ["f1", "h1", "x1"]
        with self.assertRaisesRegex(SpecError, "cycle"):
            from_json(json.dumps(raw))

    def test_implicit_actor_edge_cycle_rejects(self) -> None:
        # h1 requires r2 finished while actor b orders r2 after w1 which
        # requires h1 finished: the implicit edges close the loop.
        raw = _worked_dict()
        raw["actions"]["h1"]["requires"] = [["r2", "finished"]]
        with self.assertRaisesRegex(SpecError, "cycle"):
            from_json(json.dumps(raw))

    def test_fault_lifetime_violations_reject(self) -> None:
        base = _worked_dict()

        def without(*ids: str) -> dict:
            raw = copy.deepcopy(base)
            for action_id in ids:
                del raw["actions"][action_id]
            for actor in raw["actors"]:
                actor["actions"] = [a for a in actor["actions"] if a not in ids]
            return raw

        with self.subTest("missing remove"):
            with self.assertRaises(SpecError):
                from_json(json.dumps(without("x1")))
        with self.subTest("missing install"):
            with self.assertRaises(SpecError):
                from_json(json.dumps(without("f1")))
        with self.subTest("await without install"):
            raw = without("f1", "x1")
            raw["actions"]["w1"]["requires"] = [["h1", "finished"]]
            with self.assertRaises(SpecError):
                from_json(json.dumps(raw))
        with self.subTest("duplicate install"):
            raw = copy.deepcopy(base)
            raw["actions"]["f2"] = {
                "kind": "fault_install",
                "fault_id": "F",
                "target_action": "c0",
                "site": "checkout-begin",
                "behavior": "block",
                "deadline": "operation",
            }
            raw["actors"][0]["actions"].append("f2")
            with self.assertRaises(SpecError):
                from_json(json.dumps(raw))

    def test_deadlock_by_construction_rejects(self) -> None:
        # Parked checkout: the remove waits on the checkout finishing, but
        # the checkout can never finish while the fault is installed, and
        # no wait action witnesses the hit.
        raw = _worked_dict()
        del raw["actions"]["h1"]
        del raw["actions"]["w1"]["requires"]
        raw["actions"]["x1"]["requires"] = [["c0", "finished"]]
        raw["actors"][0]["actions"] = ["f1", "x1"]
        with self.assertRaisesRegex(SpecError, "wait"):
            from_json(json.dumps(raw))

    def test_remove_waiting_on_target_finish_rejects(self) -> None:
        # The witness is present and ordered, but the remove also waits on
        # the parked checkout's finish: same deadlock, one edge further out.
        raw = _worked_dict()
        raw["actions"]["x1"]["requires"] = [["h1", "finished"], ["c0", "finished"]]
        with self.assertRaisesRegex(SpecError, "must not wait on"):
            from_json(json.dumps(raw))

    def test_remove_ordered_only_after_the_wait_started_rejects(self) -> None:
        # In its own actor, a remove that requires only the wait's start can
        # fire before the wait witnesses the block.
        raw = _worked_dict()
        raw["actors"][0]["actions"] = ["f1", "h1"]
        raw["actors"].append({"id": "remover", "actions": ["x1"]})
        raw["actions"]["x1"]["requires"] = [["h1", "started"]]
        with self.assertRaisesRegex(SpecError, "must start after wait 'h1' finished"):
            from_json(json.dumps(raw))
        raw["actions"]["x1"]["requires"] = [["h1", "finished"]]
        self.assertEqual(
            (("h1", "finished"),), from_json(json.dumps(raw)).action("x1").requires
        )

    def test_deadline_run_rule(self) -> None:
        with self.subTest("block target must be run"):
            raw = _worked_dict()
            raw["actions"]["c0"]["deadline"] = "operation"
            with self.assertRaisesRegex(SpecError, "deadline 'run'"):
                from_json(json.dumps(raw))
        with self.subTest("run forbidden elsewhere"):
            raw = _worked_dict()
            raw["actions"]["r1"]["deadline"] = "run"
            with self.assertRaises(SpecError):
                from_json(json.dumps(raw))

    def test_action_ownership_rules(self) -> None:
        with self.subTest("unowned action"):
            raw = _worked_dict()
            raw["actors"][2]["actions"] = ["w1", "r1", "t1"]
            with self.assertRaisesRegex(SpecError, "owned by no actor"):
                from_json(json.dumps(raw))
        with self.subTest("double owned action"):
            raw = _worked_dict()
            raw["actors"][0]["actions"].append("w1")
            with self.assertRaises(SpecError):
                from_json(json.dumps(raw))
        with self.subTest("actor owns unknown action"):
            raw = _worked_dict()
            raw["actors"][2]["actions"].append("ghost")
            with self.assertRaises(SpecError):
                from_json(json.dumps(raw))

    def test_lookup_site_refuses_a_checkout_target(self) -> None:
        """A checkout's walk never reaches `FuseDispatcherImpl::lookup`."""
        raw = _worked_dict()
        raw["actions"]["f1"]["site"] = "fuse-lookup"
        raw["environment"]["target"]["repository"]["registers"][0] = (
            "tree/registers/r9.reg"
        )
        for action_id in ("r1", "r2", "t1", "w1"):
            raw["actions"][action_id]["path"] = "tree/registers/r1.reg"
        with self.assertRaisesRegex(SpecError, "cannot target kind 'checkout'"):
            from_json(json.dumps(raw))
