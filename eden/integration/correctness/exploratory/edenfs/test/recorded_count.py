# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The recorded seed-13008 history judged with a given `i0` conflict count.

Its `i0` is a Thrift NORMAL checkout from rev3 to rev0 that nothing writes
around; `i2`, an indeterminate checkout to rev3, overlaps it. `i2` can
only land before `i0` moves the parent, where it changes nothing, and no
shortcut decides a planted count: the engine runs out of explanation
budget on planted counts 1-3 (about 50 s each) and violates 4-6, never
satisfying one. `checkout_count_property_test` judges the recorded count and
`namespace_checkout_slow_test` the planted counts.
"""

from __future__ import annotations

import json
from pathlib import Path

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.history import event_from_json
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET


def recorded_verdict(conflicts: int) -> checker.Verdict:
    """Judge seed-13008 with `i0` reporting `conflicts` conflicts."""
    inputs = Path(__file__).with_name("recorded")
    spec = from_json((inputs / "seed-13008.run-spec.json").read_text(), EDENFS_TARGET)
    events = []
    for line in (inputs / "seed-13008.history.jsonl").read_text().splitlines():
        document = json.loads(line)
        if document.get("action") == "i0" and document.get("type") == "ok":
            document["conflicts"] = conflicts
        events.append(event_from_json(document))
    return checker.check(EDENFS_TARGET, spec, events, execution=None)


def count_outcome(verdict: checker.Verdict) -> tuple[str, str | None]:
    """The status and not-evaluated reason of `checkout.conflict-count` on `i0`."""
    assert verdict.report is not None
    (item,) = (
        item
        for result in verdict.report.results
        for item in result.obligations
        if (item.rule_id, item.scope) == ("checkout.conflict-count", "i0")
    )
    return str(item.status), item.not_evaluated_reason
