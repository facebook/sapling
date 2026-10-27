# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live namespace and checkout-shape histories judged by `EDENFS_TARGET`.

A serial sequence of renames (onto a register), unlinks, a mkdir, an
`sl mv`, presence observers, a dirty Thrift NORMAL checkout (its conflict
count against the contract per-path conflict states), sl status, two
listings, and a manifest probe runs against a live daemon: the verdict
must accept, every namespace, checkout, existence, manifest, and listing
obligation must be satisfied, and the conflict count is judged by the
engine. Generated specs that compose namespace operations must not be
rejected either. An sl status issued while a Thrift checkout is parked at
`checkout-begin` past Sapling's 60 s wait exits with EdenFS's status
refusal, and that history must accept too.
"""

from __future__ import annotations

import json
import os
import unittest
from collections.abc import Mapping
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.generate import compose, derived_rng
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    Environment,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    sample_environment,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _fail_on_setup_failure,
    _require_live,
)
from eden.integration.correctness.exploratory.edenfs.test.live import run_contained

_R0, _R1, _R2 = (f"tree/registers/r{index}.reg" for index in range(3))
_DIR = "tree/registers/dir-x"
_MOVED = "tree/registers/moved-x"
_JUDGED_PROPERTIES = ("namespace", "checkout", "existence", "manifest", "listing")
_NAMESPACE_KINDS = frozenset({"rename", "unlink", "mkdir", "sl_rm", "sl_mv"})


def _removes_before_sl_goto(spec: RunSpec) -> bool:
    """Return whether a plain `sl goto` of `spec` waits for a register's FUSE removal.

    Every revision holds a different register image, so the destination
    changes the register: the locally deleted path blocks plain `sl goto`,
    and today's daemon lets the goto through, which `checkout.sl-abort`
    reports (T294527512).
    """
    registers = set(spec.environment.target["repository"]["registers"])
    removes = {
        action.id
        for action in spec.actions
        if (action.kind == "unlink" and action.payload["path"] in registers)
        or (action.kind == "rename" and action.payload["source"] in registers)
    }
    return any(
        action.kind == "checkout"
        and action.payload["via"] == "goto"
        and action.payload["mode"] == "normal"
        and any(required in removes for required, _state in action.requires)
        for action in spec.actions
    )


def _action(action_id: str, kind: str, **payload: Any) -> Action:
    return Action(action_id, kind, DEADLINE_OPERATION, payload=payload)


def sequence_spec() -> RunSpec:
    """Return the serial namespace and checkout sequence."""
    actions = [
        _action("w", "write", path=_R0, value_id="live-w"),
        _action("m", "rename", source=_R0, destination=_R1),
        _action("g1", "getattr", path=_R1),
        _action("g0", "getattr", path=_R0),
        _action("r1", "read", path=_R1, channel="fuse"),
        _action("mk", "mkdir", path=_DIR),
        _action("gd", "getattr", path=_DIR),
        _action("rd", "read", path=_DIR, channel="fuse"),
        _action("u", "unlink", path=_R1),
        _action("gu", "getattr", path=_R1),
        _action("hm", "sl_mv", source=_R2, destination=_MOVED),
        _action("gm", "getattr", path=_MOVED),
        _action("s1", "status", channel="sl"),
        _action("c", "checkout", revision=2, via="thrift", mode="normal"),
        _action("s2", "status", channel="sl"),
        _action("lf", "readdir", path="tree/registers", channel="fuse"),
        _action("lt", "readdir", path="tree/registers", channel="thrift"),
        _action("p", "manifest_presence", from_revision=3, to_revision=2),
        _action("g2", "getattr", path=_R2),
    ]
    environment = {
        "repository": {
            "file_count": 32,
            "commit_count": 4,
            "registers": [_R0, _R1, _R2],
        },
        "channel": "fuse",
        "case_sensitive": True,
        "fixture_source": "backing",
        "setup_profile": {"materialize": False, "scatter_dirty": 0},
    }
    return RunSpec(
        seed=11,
        target=EDENFS_TARGET.name,
        environment=Environment(environment),
        budgets=Budgets(120.0, 60.0, 540.0),
        actors=(Actor(id="a", actions=tuple(action.id for action in actions)),),
        actions=tuple(actions),
    )


def refused_status_spec() -> RunSpec:
    """Return a Thrift checkout parked at `checkout-begin` with an sl and a Thrift status.

    The block outlasts Sapling's 60 s wait for a running checkout, so the sl
    status exits with EdenFS's status refusal (observed live).
    """

    def act(action_id: str, kind: str, *requires: str, **payload: Any) -> Action:
        deadline = "run" if kind == "checkout" else DEADLINE_OPERATION
        return Action(
            action_id,
            kind,
            deadline,
            payload=payload,
            requires=tuple((required, "finished") for required in requires),
        )

    hit = Action(
        "h",
        "fault_await",
        DEADLINE_OPERATION,
        payload={"fault_id": "F"},
        requires=(("c", "started"),),
    )
    actions = [
        act(
            "f",
            "fault_install",
            fault_id="F",
            site="checkout-begin",
            target_action="c",
            behavior="block",
        ),
        hit,
        act("z", "sleep", "h", seconds=70.0),
        act("x", "fault_remove", "z", fault_id="F"),
        act("c", "checkout", "f", revision=2, via="thrift", mode="normal"),
        act("s", "status", "h", channel="sl"),
        act("t", "status", "h", channel="thrift", revision=3),
        act("r", "read", "c", path=_R0, channel="fuse"),
    ]
    environment = {
        "repository": {
            "file_count": 32,
            "commit_count": 4,
            "registers": [_R0, _R1, _R2],
        },
        "channel": "fuse",
        "case_sensitive": True,
        "fixture_source": "backing",
        "setup_profile": {"materialize": False, "scatter_dirty": 0},
    }
    return RunSpec(
        seed=21,
        target=EDENFS_TARGET.name,
        environment=Environment(environment),
        budgets=Budgets(150.0, 60.0, 540.0),
        actors=(
            Actor(id="n", actions=("f", "h", "z", "x")),
            Actor(id="a", actions=("c", "r")),
            Actor(id="b", actions=("s",)),
            Actor(id="d", actions=("t",)),
        ),
        actions=tuple(actions),
    )


def generated_specs(count: int) -> list[RunSpec]:
    """Return the first `count` fault-free generated specs composing a namespace op.

    Specs T294527512 rejects on today's daemon are skipped.
    """
    found = []
    seed = 0
    while len(found) < count:
        environment = dict(sample_environment(derived_rng(seed, "n8-live")))
        spec = compose(EDENFS_TARGET, seed, environment, no_fault=True)
        if any(
            action.kind in _NAMESPACE_KINDS for action in spec.actions
        ) and not _removes_before_sl_goto(spec):
            found.append(spec)
        seed += 1
    return found


def _summary(verdict: checker.Verdict) -> Mapping[str, Any]:
    assert verdict.report is not None
    return {
        f"{item.rule_id}[{item.scope}]": [str(item.status), item.not_evaluated_reason]
        for result in verdict.report.results
        for item in result.obligations
        if str(item.status) != "satisfied"
    }


class NamespaceLiveTest(unittest.TestCase):
    def _live(self, spec: RunSpec, prefix: str) -> checker.Verdict:
        return self._live_history(spec, prefix)[0]

    def _live_history(
        self, spec: RunSpec, prefix: str
    ) -> tuple[checker.Verdict, list[dict[str, Any]]]:
        _require_live(self)
        with _artifact_directory(prefix) as tmp:
            result = run_contained(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
            with open(os.path.join(tmp, "history.jsonl")) as handle:
                history = handle.read()
        self.assertEqual("completed", result.execution.status, stderr[-4000:])
        print(f"{prefix} verdict: {result.verdict.kind} {result.verdict.reason}")
        print(f"{prefix} history:\n{history}")
        return result.verdict, [json.loads(line) for line in history.splitlines()]

    def test_an_sl_status_refused_by_a_running_checkout_is_accepted(self) -> None:
        verdict, events = self._live_history(refused_status_spec(), "n8-refused.")
        completions = {
            event["action"]: event
            for event in events
            if event.get("type") in ("ok", "fail", "info")
        }
        sl = completions["s"]
        self.assertEqual("fail", sl["type"], sl)
        self.assertEqual(255, sl["returncode"], sl)
        self.assertIn(
            "cannot compute status while a checkout is currently in progress",
            sl["stderr"],
        )
        self.assertEqual("fail", completions["t"]["type"], completions["t"])
        summary = _summary(verdict)
        self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))

    def test_the_namespace_sequence_is_accepted(self) -> None:
        verdict = self._live(sequence_spec(), "n8-sequence.")
        summary = _summary(verdict)
        print("unsatisfied: " + json.dumps(summary, sort_keys=True))
        self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))
        assert verdict.report is not None
        judged = {
            (item.rule_id, item.scope): str(item.status)
            for result in verdict.report.results
            if result.property_id in _JUDGED_PROPERTIES
            for item in result.obligations
        }
        # `c` is settled (nothing overlaps it), so no parent-before-paths
        # order is derived for it.
        self.assertNotIn(("checkout.parent-before-paths", "c"), judged)
        for key in (
            ("checkout.conflict-count", "c"),
            ("existence.present-after-remove", "g1"),
            ("existence.missing-after-create", "g0"),
            ("existence.missing-after-create", "gu"),
            ("existence.mkdir-type", "gd"),
            ("existence.directory-read", "rd"),
            ("listing.channel-disagreement", "lt"),
            ("manifest.checkout-mismatch", "p"),
            ("namespace.precondition", "hm"),
            ("namespace.precondition", "m"),
            ("namespace.precondition", "mk"),
            ("namespace.precondition", "u"),
        ):
            with self.subTest(obligation=key):
                self.assertEqual("satisfied", judged.get(key))

    def test_generated_namespace_specs_are_not_rejected(self) -> None:
        for spec in generated_specs(4):
            with self.subTest(seed=spec.seed):
                verdict = self._live(spec, f"n8-generated-{spec.seed}.")
                self.assertNotEqual(
                    checker.REJECT, verdict.kind, json.dumps(_summary(verdict))
                )
