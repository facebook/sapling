# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""`common.disturbed`, behind every EdenFS direct check, against the engine.

The direct checks compare two results only over a span `disturbed` calls
undisturbed for the cells they read, claiming every legal state holds those
cells constant there. So whenever `disturbed` says that of a register's
path cell over two getattrs, the getattrs disagreeing on presence must
have no legal explanation: the engine violates one of their existence
obligations.
"""

from __future__ import annotations

import errno
import random
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.effects import Window
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import path_cell
from eden.integration.correctness.exploratory.edenfs.properties.common import disturbed
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge_isolated,
    obligations_of,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import getattr_facts
from eden.integration.correctness.exploratory.test.corpus.author import (
    errno_fail,
    R0,
    R1,
    Run,
    WRITE_OK,
)


def _history(seed: int) -> Run:
    """Up to four writes, unlinks, and renames of r0 and r1, legal in sequence,
    with two getattrs of one register among them (present, then absent); some
    steps overlap the next event or stay indeterminate."""
    rng = random.Random(seed)
    run = Run("t", f"disturbed-{seed}", "random namespace history, two getattrs")
    present = {R0: True, R1: True}
    steps: list[tuple[str, dict[str, Any]]] = []
    for index in range(rng.randint(1, 4)):
        live = [path for path, here in present.items() if here]
        if not live:
            break
        name, target = f"x{index}", rng.choice(live)
        kind = rng.choice(["write", "unlink", "rename"])
        if kind == "write":
            run.act("ab"[index % 2], name, kind, path=target, value_id=name)
            steps.append((name, WRITE_OK))
            continue
        if kind == "unlink":
            present[target] = False
            payload: dict[str, Any] = {"path": target}
        else:
            other = R1 if target == R0 else R0
            present[target], present[other] = False, True
            payload = {"source": target, "destination": other}
        run.act("ab"[index % 2], name, kind, **payload)
        steps.append((name, payload))
    path = rng.choice([R0, R1])
    run.act("c", "g1", "getattr", path=path)
    run.act("c", "g2", "getattr", path=path)
    first = rng.randint(0, len(steps))
    second = rng.randint(first, len(steps))
    order = [*steps]
    order.insert(second, ("g2", errno_fail(errno.ENOENT, "stat")))
    order.insert(first, ("g1", getattr_facts()))
    pending: list[tuple[str, dict[str, Any]]] = []
    for name, facts in order:
        run.inv(name)
        for earlier, earlier_facts in pending:
            if rng.random() < 0.3:
                run.done(earlier, "info", reason="timed out")
            else:
                run.done(earlier, "ok", **earlier_facts)
        getattr_step = name in ("g1", "g2")
        pending = [] if getattr_step or rng.random() >= 0.3 else [(name, facts)]
        if not pending:
            run.done(name, "fail" if name == "g2" else "ok", **facts)
    for earlier, earlier_facts in pending:
        run.done(earlier, "ok", **earlier_facts)
    run.finish()
    return run


class DisturbedParityTest(unittest.TestCase):
    def test_an_undisturbed_span_admits_no_presence_change(self) -> None:
        undisturbed = 0
        for seed in range(300):
            run = _history(seed)
            spec = from_json(run.spec_text(), EDENFS_TARGET)
            facts = checker.HistoryFacts.build(
                spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
            )
            derived = checker.contract_effects(
                EDENFS_TARGET, spec, facts, CONTRACT_RULES
            )
            assert not isinstance(derived, checker.PropertyError)
            first, second = facts.action("g1"), facts.action("g2")
            assert first.invoked is not None and second.completed is not None
            span = Window(first.invoked, second.completed)
            cells = (path_cell(spec.action("g1").payload["path"]),)
            if disturbed(derived[1], cells, span):
                continue
            judged = {
                item.status
                for (rule, scope), item in obligations_of(
                    judge_isolated(("existence",), run)
                ).items()
                if rule.startswith("existence.") and scope in ("g1", "g2")
            }
            if not judged:
                continue
            undisturbed += 1
            with self.subTest(seed=seed):
                self.assertIn(ObligationStatus.VIOLATED, judged)
        self.assertGreater(undisturbed, 30)
