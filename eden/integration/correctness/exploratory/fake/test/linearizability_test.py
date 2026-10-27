# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The fake target's verdicts, cross-checked by a from-scratch linearizability check.

Random raw write/read histories are judged by the fake target through the
real checker (contracts hook, bytes collector, consistency engine) and by an
independent brute-force register linearizability check over the raw
events; the two must agree on every history.
"""

from __future__ import annotations

import itertools
import json
import math
import random
import unittest
from collections import Counter

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.facts import INFO, OK
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.fake.target import (
    BYTES_FIELD,
    encode_value,
    FAKE,
)


def linearizable(
    seed: int, events: list[Event], kinds: dict[str, dict[str, str]]
) -> bool:
    """Brute-force register linearizability over raw events, path by path.

    An ok operation takes effect inside [invoke, ok]; an info write, or a
    write with no terminal event (a prefix history), may take effect once
    anywhere after its invocation, or never; a read returns the last
    write's bytes, or the register's initial bytes.
    """
    start, end, outcome, returned = {}, {}, {}, {}
    for event in events:
        action = event.data["action"]
        if event.type == "invoke":
            start[action] = event.time
        else:
            end[action], outcome[action] = event.time, event.type
            if BYTES_FIELD in event.data:
                returned[action] = bytes.fromhex(event.data[BYTES_FIELD])
    paths = sorted({payload["path"] for payload in kinds.values()})
    for path in paths:
        ops = [a for a, p in kinds.items() if p["path"] == path and a in start]
        must = [a for a in ops if outcome.get(a) == "ok"]
        maybe = [
            a
            for a in ops
            if outcome.get(a) in ("info", None) and kinds[a]["kind"] == "write"
        ]
        found = False
        for size in range(len(maybe) + 1):
            for chosen in itertools.combinations(maybe, size):
                chosen_ops = must + list(chosen)
                for order in itertools.permutations(chosen_ops):
                    if _replays(
                        seed, path, order, start, end, outcome, returned, kinds
                    ):
                        found = True
                        break
                if found:
                    break
            if found:
                break
        if not found:
            return False
    return True


def _replays(seed, path, order, start, end, outcome, returned, kinds) -> bool:
    hi = {a: end[a] if outcome.get(a) == "ok" else math.inf for a in order}
    for i, a in enumerate(order):
        if any(hi[b] < start[a] for b in order[i + 1 :]):
            return False
    value = encode_value(seed, "init", path)
    for a in order:
        if kinds[a]["kind"] == "write":
            value = encode_value(seed, kinds[a]["actor"], kinds[a]["value_id"])
        elif returned[a] != value:
            return False
    return True


def _random_history(
    rng: random.Random,
) -> tuple[RunSpec, list[Event], dict[str, dict[str, str]]]:
    paths = ["register-0", "register-1"][: rng.choice((1, 1, 2))]
    actions: dict[str, dict[str, str]] = {}
    for index in range(rng.randrange(2, 7)):
        name = f"x{index}"
        if rng.random() < 0.5:
            actions[name] = {
                "kind": "write",
                "deadline": "operation",
                "path": rng.choice(paths),
                "value_id": f"v{index}",
            }
        else:
            actions[name] = {
                "kind": "read",
                "deadline": "operation",
                "path": rng.choice(paths),
            }
    raw = {
        "schema": "edenfs-exploratory-run",
        "schema_version": 1,
        "seed": 7,
        "target": "fake",
        "environment": {"target": {}},
        "budgets": {
            "operation_deadline_seconds": 60,
            "cleanup_deadline_seconds": 10,
            "runner_deadline_seconds": 120,
        },
        "actors": [{"id": f"a{name}", "actions": [name]} for name in actions],
        "actions": actions,
    }
    spec = from_json(json.dumps(raw), FAKE)
    kinds = {name: {**body, "actor": f"a{name}"} for name, body in actions.items()}
    writes = {
        name: encode_value(spec.seed, f"a{name}", body["value_id"])
        for name, body in actions.items()
        if body["kind"] == "write"
    }
    events: list[tuple[float, int, Event]] = []
    for order, (name, body) in enumerate(actions.items()):
        lo = float(rng.randrange(0, 6))
        hi = lo + rng.randrange(1, 4)
        events.append((lo, order, Event("invoke", lo, {"action": name})))
        if body["kind"] == "write":
            draw = rng.random()
            if draw >= 0.25:
                events.append((hi, order, Event(OK, hi, {"action": name})))
            elif draw < 0.15 or order == 0:
                events.append((hi, order, Event(INFO, hi, {"action": name})))
            # Otherwise the write has no terminal event: a prefix history
            # (never the first action, so some modeled action completes).
            continue
        same = [v for w, v in writes.items() if actions[w]["path"] == body["path"]]
        other = [v for w, v in writes.items() if actions[w]["path"] != body["path"]]
        candidates = [encode_value(spec.seed, "init", body["path"]), *same]
        value = rng.choice(candidates)
        draw = rng.random()
        if draw < 0.05:
            value = encode_value(spec.seed, "ghost", f"g{order}")
        elif draw < 0.10 and len(candidates) > 1:
            first, second = rng.sample(candidates, 2)
            value = first[: len(first) // 2] + second[len(second) // 2 :]
        elif draw < 0.15 and other:
            value = rng.choice(other)
        events.append(
            (hi, order, Event(OK, hi, {"action": name, BYTES_FIELD: value.hex()}))
        )
    return (
        spec,
        [event for *_key, event in sorted(events, key=lambda item: item[:2])],
        kinds,
    )


class FakeLinearizabilityTest(unittest.TestCase):
    def test_fake_verdicts_agree_with_raw_linearizability(self) -> None:
        """Over 1,000 generated fake histories the checker's verdict is linearizability."""
        tally: Counter[str] = Counter({"discrepancies": 0})
        for seed in range(1000):
            spec, events, kinds = _random_history(random.Random(seed))
            verdict = checker.check(FAKE, spec, events)
            reads = any(body["kind"] == "read" for body in kinds.values())
            if reads:
                self.assertIn(verdict.kind, (checker.ACCEPT, checker.REJECT), seed)
            else:
                # No read means nothing to judge (or, with only info writes,
                # no operation shown to have run).
                self.assertEqual(checker.NOT_EVALUATED, verdict.kind, seed)
            expected = linearizable(spec.seed, events, kinds)
            tally["linearizable" if expected else "not-linearizable"] += 1
            if (verdict.kind != checker.REJECT) != expected:
                tally["discrepancies"] += 1
        self.assertEqual(0, tally["discrepancies"], tally)
        self.assertGreater(tally["not-linearizable"], 300, tally)
        self.assertGreater(tally["linearizable"], 300, tally)

    def test_named_register_regressions(self) -> None:
        """The old byte engine's named regressions, as fake histories, with their twins.

        Each operation is (name, kind, value, invoke, completion, outcome):
        a write's value is its own, `init` is the register's initial value.
        Both the checker and the brute-force check must give the verdict.
        """
        cases = {
            # Initial bytes cannot reappear after the sole write was observed.
            "flicker": (
                [("w", "write", "w", 1, 10, INFO), ("rb", "read", "w", 2, 3, OK)],
                ("ra", "read", "init", 4, 5, OK),
                ("ra", "read", "init", 2.5, 5, OK),
            ),
            # A producer applies once; its overwritten bytes cannot return.
            "resurrection": (
                [
                    ("a", "write", "a", 0, 1, OK),
                    ("b", "write", "b", 2, 10, INFO),
                    ("rb", "read", "b", 3, 4, OK),
                ],
                ("ra", "read", "a", 5, 6, OK),
                ("ra", "read", "a", 3.5, 6, OK),
            ),
            # b, a, b with each producer applying once.
            "b-a-b": (
                [
                    ("b", "write", "b", 1, 10, OK),
                    ("a", "write", "a", 0, 10, INFO),
                    ("r1", "read", "b", 2, 3, OK),
                    ("r2", "read", "a", 4, 5, OK),
                ],
                ("r3", "read", "b", 6, 7, OK),
                ("r3", "read", "b", 4.5, 7, OK),
            ),
            # A write with no terminal event may have applied, but not before
            # its invocation.
            "unterminated": (
                [("w", "write", "w", 2, math.inf, None)],
                ("r", "read", "w", 0.5, 1, OK),
                ("r", "read", "w", 3, 4, OK),
            ),
            # A read overlapping an ack may see the old bytes; a later one may not.
            "optional-after-overlap": (
                [
                    ("b", "write", "b", 1, 5, OK),
                    ("a", "write", "a", 6, 7, INFO),
                    ("r2", "read", "b", 9, 10, OK),
                ],
                ("r1", "read", "init", 7.5, 8, OK),
                ("r1", "read", "init", 4, 8, OK),
            ),
        }
        for name, (common, illegal, legal) in cases.items():
            for last, linear in ((illegal, False), (legal, True)):
                with self.subTest(case=name, legal=linear):
                    spec, events, kinds = _explicit_history([*common, last])
                    self.assertEqual(linear, linearizable(spec.seed, events, kinds))
                    verdict = checker.check(FAKE, spec, events)
                    self.assertEqual(
                        checker.ACCEPT if linear else checker.REJECT, verdict.kind
                    )


def _explicit_history(
    operations: list[tuple[str, str, str, float, float, str | None]],
) -> tuple[RunSpec, list[Event], dict[str, dict[str, str]]]:
    """Return a one-register fake spec and history of the given operations.

    An operation whose outcome is None gets no terminal event.
    """
    path = "register-0"
    actions: dict[str, dict[str, str]] = {}
    for name, kind, _value, *_rest in operations:
        actions[name] = {"kind": kind, "deadline": "operation", "path": path}
        if kind == "write":
            actions[name]["value_id"] = name
    raw = {
        "schema": "edenfs-exploratory-run",
        "schema_version": 1,
        "seed": 7,
        "target": "fake",
        "environment": {"target": {}},
        "budgets": {
            "operation_deadline_seconds": 60,
            "cleanup_deadline_seconds": 10,
            "runner_deadline_seconds": 120,
        },
        "actors": [{"id": f"a{name}", "actions": [name]} for name in actions],
        "actions": actions,
    }
    spec = from_json(json.dumps(raw), FAKE)
    kinds = {name: {**body, "actor": f"a{name}"} for name, body in actions.items()}
    timed: list[tuple[float, int, Event]] = []
    for order, (name, kind, value, lo, hi, outcome) in enumerate(operations):
        timed.append((lo, order, Event("invoke", lo, {"action": name})))
        data: dict[str, str] = {"action": name}
        if kind == "read":
            writer = "init" if value == "init" else f"a{value}"
            value_id = path if value == "init" else value
            data[BYTES_FIELD] = encode_value(spec.seed, writer, value_id).hex()
        if outcome is not None:
            timed.append((hi, order, Event(outcome, hi, data)))
    events = [event for *_key, event in sorted(timed, key=lambda item: item[:2])]
    return spec, events, kinds
