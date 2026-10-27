# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Advisory signals: the release summary takes its lanes from the supplied decks and
gates only the blocking ones.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.advisory import (
    DeckRun,
    load_runs,
    load_specs,
    release_summary,
)
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    encode_verdict,
    NOT_EVALUATED,
    REJECT,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    CONTAINMENT_TIMEOUT,
    HARNESS_FAILURE,
    MOUNT_WEDGED_PREFIX,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    ObligationResult,
    ObligationStatus,
    path_subject,
    PropertyResult,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.fake.target import FAKE
from eden.integration.correctness.exploratory.schemas import SchemaError
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    spec_document,
    synthetic_verdict,
)


def _violated(rule: str) -> ObligationResult:
    return ObligationResult(
        rule_id=rule,
        scope="r",
        subject=path_subject("r"),
        status=ObligationStatus.VIOLATED,
        attribution=Attribution.UNDETERMINED,
        reason="x",
    )


class ReleaseSummaryTest(unittest.TestCase):
    def _specs(self, *decks: tuple[str, dict[str, Any]]) -> dict[str, RunSpec]:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            for seed, (name, metadata) in enumerate(decks):
                document = spec_document(seed=seed + 5, target="fake")
                document["metadata"] = metadata
                (directory / f"{name}.json").write_text(json.dumps(document))
            return load_specs(directory, FAKE)

    def _decks(self) -> dict[str, RunSpec]:
        return self._specs(
            ("good", {"expected": "accept"}),
            (
                "unknown",
                {"expected": "not-evaluated", "expected_not_evaluated": ["r"]},
            ),
            ("bug", {"expected": "reject"}),
            (
                "hang",
                {
                    "expected": "reject",
                    "expected_execution": ["execution.mount-wedged"],
                },
            ),
        )

    def _passing(self) -> dict[str, DeckRun]:
        return {
            "good": DeckRun(COMPLETED, ACCEPT),
            "unknown": DeckRun(COMPLETED, NOT_EVALUATED, ("r",)),
            "bug": DeckRun(COMPLETED, REJECT),
            "hang": DeckRun(
                CONTAINMENT_TIMEOUT, ACCEPT, (), ("execution.mount-wedged",)
            ),
        }

    def test_blocking_lanes_pass_and_reject_decks_reproduce(self) -> None:
        summary = release_summary(self._decks(), self._passing())
        self.assertEqual(("PASS", []), (summary["gate"], summary["failures"]))
        self.assertEqual(["bug", "hang"], summary["reproduced"])
        self.assertEqual("unmeasured", summary["reject_decks"]["hang"])
        self.assertEqual([], summary["unmeasured"])

    def test_every_blocking_verdict_mismatch_fails_the_gate(self) -> None:
        cases = {
            "good": DeckRun(COMPLETED, REJECT),
            "unknown": DeckRun(COMPLETED, NOT_EVALUATED, ("other",)),
        }
        for name, run in cases.items():
            with self.subTest(name=name):
                summary = release_summary(self._decks(), {**self._passing(), name: run})
                self.assertEqual(
                    ("FAIL", [name]), (summary["gate"], summary["failures"])
                )

    def test_reject_decks_are_advisory_and_never_unmeasured(self) -> None:
        runs = {
            **self._passing(),
            "bug": DeckRun(HARNESS_FAILURE, NOT_EVALUATED),
            "hang": DeckRun(
                CONTAINMENT_TIMEOUT, ACCEPT, (), ("execution.operation-timeout",)
            ),
        }
        summary = release_summary(self._decks(), runs)
        self.assertEqual(
            ("PASS", [], []),
            (summary["gate"], summary["reproduced"], summary["unmeasured"]),
        )
        flipped = release_summary(
            self._decks(), {**self._passing(), "bug": DeckRun(COMPLETED, ACCEPT)}
        )
        self.assertEqual(
            (["bug"], "PASS"), (flipped["flip_candidates"], flipped["gate"])
        )

    def test_an_incomplete_or_missing_blocking_run_fails_the_gate(self) -> None:
        runs = {**self._passing(), "good": DeckRun(HARNESS_FAILURE, ACCEPT)}
        del runs["unknown"]
        summary = release_summary(self._decks(), runs)
        self.assertEqual(["good"], summary["unmeasured"])
        self.assertEqual(["unknown"], summary["missing"])
        self.assertEqual(["good", "unknown"], summary["failures"])

    def test_a_missing_decks_or_artifacts_directory_is_a_schema_error(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            absent = Path(temporary) / "absent"
            with self.assertRaisesRegex(SchemaError, "is not a directory"):
                load_specs(absent, FAKE)
            with self.assertRaisesRegex(SchemaError, "is not a directory"):
                load_runs(absent, FAKE)

    def test_decks_with_bad_metadata_are_schema_errors(self) -> None:
        cases = {
            "expected is None": {
                **self._decks(),
                **self._specs(("bare", {})),
            },
            "expected is 'flaky'": {
                **self._decks(),
                **self._specs(("unknown-lane", {"expected": "flaky"})),
            },
            "expected_not_evaluated is not a list": {
                **self._decks(),
                **self._specs(("ne", {"expected": "not-evaluated"})),
            },
        }
        for message, specs in cases.items():
            with self.subTest(message=message):
                with self.assertRaisesRegex(SchemaError, message):
                    release_summary(specs, self._passing())

    def test_load_runs_reads_verdicts_and_typed_execution_findings(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wedged = root / "hang"
            wedged.mkdir()
            (wedged / "execution.json").write_text(
                json.dumps(
                    {
                        "status": CONTAINMENT_TIMEOUT,
                        "reason": "runner exceeded 600s",
                        "cleanup": MOUNT_WEDGED_PREFIX + "aborted 1 connection",
                    }
                )
            )
            done = root / "good"
            done.mkdir()
            (done / "execution.json").write_text(json.dumps({"status": COMPLETED}))
            (done / "verdict.json").write_bytes(
                encode_verdict(
                    synthetic_verdict(
                        PropertyResult("bytes", (_violated("bytes.torn-read"),))
                    )
                )
            )
            (root / "empty").mkdir()
            runs = load_runs(root, FAKE)
        self.assertEqual({"hang", "good"}, set(runs))
        self.assertEqual((COMPLETED, REJECT), (runs["good"].status, runs["good"].kind))
        self.assertIsNone(runs["hang"].kind)
        self.assertIn("execution.mount-wedged", runs["hang"].execution_ids)
