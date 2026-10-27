# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live red controls trip the bytes and status rules that map them in `EDENFS_TARGET`.

Each control is a silent fault the live backend injects (`backends.py`):
`torn-write` writes half an image, `status-loss` drops an `M` entry from sl
status, `diff-loss` from sl diff, `common-status-loss` from both. The run is
executed and judged by `EDENFS_TARGET`; the mapped rule must be violated,
both in the supervisor's verdict and when the captured history is checked
again. With `EDEN_RED_CONTROL_CAPTURE` set, each run's spec
and history are copied under that directory (`edenfs/test/red_controls/`
holds the checked-in captures `properties_test` judges offline).
"""

from __future__ import annotations

import json
import os
import shutil
import unittest

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.containment import (
    load_history,
    RUN_SPEC_NAME,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    DEADLINE_OPERATION,
    FINISHED,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _fail_on_setup_failure,
    _require_live,
    _spec,
    REGISTERS,
)
from eden.integration.correctness.exploratory.edenfs.test.live import run_contained

_REGISTER = REGISTERS[0]
CAPTURE_ENV = "EDEN_RED_CONTROL_CAPTURE"
# Each control, the observing action, and the rule it must trip.
CONTROLS = {
    "torn-write": ("read", "bytes.torn-read"),
    "status-loss": ("status", "status.status-diff-mismatch"),
    "diff-loss": ("status", "status.status-diff-mismatch"),
    "common-status-loss": ("status", "status.sl-fuse-stale-status"),
}


def control_spec(observer: str) -> RunSpec:
    """Return one write of the register followed by a FUSE read or an sl status."""
    write = Action(
        "w",
        "write",
        DEADLINE_OPERATION,
        payload={"path": _REGISTER, "value_id": "red-w"},
    )
    payload = (
        {"path": _REGISTER, "channel": "fuse"}
        if observer == "read"
        else {"channel": "sl"}
    )
    observe = Action(
        "o", observer, DEADLINE_OPERATION, requires=(("w", FINISHED),), payload=payload
    )
    return _spec(_REGISTER, [write, observe], [("a", ["w", "o"])])


class RedControlLiveTest(unittest.TestCase):
    def _trips(self, control: str) -> None:
        observer, rule_id = CONTROLS[control]
        spec = control_spec(observer)
        _require_live(self)
        with _artifact_directory(f"red-{control}.") as tmp:
            result = run_contained(spec, tmp, red_control=control)
            _fail_on_setup_failure(result, tmp)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
            self.assertEqual("completed", result.execution.status, stderr[-4000:])
            events = load_history(os.path.join(tmp, "history.jsonl"))
            capture = os.environ.get(CAPTURE_ENV)
            if capture:
                target_dir = os.path.join(capture, control)
                os.makedirs(target_dir, exist_ok=True)
                for name in (RUN_SPEC_NAME, "history.jsonl"):
                    shutil.copyfile(
                        os.path.join(tmp, name), os.path.join(target_dir, name)
                    )
        self.assertEqual(checker.REJECT, result.verdict.kind)
        verdict = checker.check(
            EDENFS_TARGET, spec, events, execution={"status": "completed"}
        )
        report = verdict.report
        if report is None:
            self.fail(f"{control}: the re-check produced no report")
        violated = {
            finding.rule_id for result in report.results for finding in result.findings
        }
        self.assertEqual(checker.REJECT, verdict.kind, json.dumps(sorted(violated)))
        self.assertIn(rule_id, violated)

    def test_torn_write_trips_torn_read(self) -> None:
        self._trips("torn-write")

    def test_status_loss_trips_status_diff_mismatch(self) -> None:
        self._trips("status-loss")

    def test_diff_loss_trips_status_diff_mismatch(self) -> None:
        self._trips("diff-loss")

    def test_common_status_loss_trips_sl_fuse_stale_status(self) -> None:
        self._trips("common-status-loss")
