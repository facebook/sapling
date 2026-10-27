# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Opt-in frozen-spec replays and the release summary over their verdicts.

The deck directory, the target that parses it, and the live runner are
supplied by the caller, so this module names no target.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    NOT_EVALUATED,
    REJECT,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    EXECUTION_NAME,
    ExplorationResult,
    VERDICT_NAME,
)
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.findings import execution_findings
from eden.integration.correctness.exploratory.schemas import (
    admission_for,
    load_verdict_file,
    RunPurpose,
    SchemaError,
    strict_loads,
    VerdictFile,
    write_admission,
)

MISSING = "missing"
UNMEASURED = "unmeasured"
DECKS_DECISION = "release-summary:invalid-decks"
LANES = ("accept", "reject", "not-evaluated")
Runner = Callable[[RunSpec, str], ExplorationResult]


def not_evaluated_reasons(verdict: Verdict) -> list[str]:
    """Return the sorted declared reasons of a verdict's not-evaluated obligations."""
    report = verdict.report
    if report is None:
        return []
    return sorted(
        {
            obligation.not_evaluated_reason
            for result in report.results
            for obligation in result.obligations
            if obligation.status == ObligationStatus.NOT_EVALUATED
            and obligation.not_evaluated_reason is not None
        }
    )


def _advisory_run(
    run: Runner, spec: RunSpec, run_dir: Path, purpose: RunPurpose
) -> Verdict:
    """Run one live spec and write its admission."""
    result = run(spec, str(run_dir))
    if run_dir.is_dir():
        write_admission(run_dir, admission_for(purpose, None))
    return result.verdict


def _require_dir(path: Path) -> None:
    if not path.is_dir():
        raise SchemaError(DECKS_DECISION, f"{path} is not a directory")


def load_specs(decks: Path, target: Target) -> dict[str, RunSpec]:
    """Load every frozen composition in `decks` under the target's vocabulary."""
    _require_dir(decks)
    return {
        path.stem: from_json(path.read_text(), target)
        for path in sorted(decks.iterdir())
        if path.suffix == ".json"
    }


@dataclass(frozen=True)
class DeckRun:
    """One replay directory as the release summary reads it.

    `kind` is None when the run left no readable verdict; `execution_ids`
    are the run's typed execution findings, in order.
    """

    status: str | None
    kind: str | None
    not_evaluated: tuple[str, ...] = ()
    execution_ids: tuple[str, ...] = ()


def _execution_status(directory: Path) -> str | None:
    path = directory / EXECUTION_NAME
    if not path.is_file():
        return None
    document = strict_loads(path.read_text())
    return document.get("status") if type(document) is dict else None


def load_runs(root: Path, target: Target) -> dict[str, DeckRun]:
    """Read every run directory under `root` that left an execution or a verdict."""
    _require_dir(root)
    runs = {}
    for directory in sorted(root.iterdir()):
        if (
            not (directory / EXECUTION_NAME).is_file()
            and not (directory / VERDICT_NAME).is_file()
        ):
            continue
        try:
            verdict_file: VerdictFile | SchemaError = load_verdict_file(
                directory / VERDICT_NAME
            )
        except SchemaError as error:
            verdict_file = error
        verdict = (
            None if isinstance(verdict_file, SchemaError) else verdict_file.verdict
        )
        runs[directory.name] = DeckRun(
            _execution_status(directory),
            None if verdict is None else verdict.kind,
            () if verdict is None else tuple(not_evaluated_reasons(verdict)),
            tuple(
                dict.fromkeys(
                    f.execution_id
                    for f in execution_findings(directory, target, verdict_file)
                )
            ),
        )
    return runs


def _reproduced(spec: RunSpec, run: DeckRun | None) -> bool:
    """A reject deck reproduces when it completes rejecting, or leaves every execution finding it expects."""
    if run is None:
        return False
    if run.status == COMPLETED:
        return run.kind == REJECT
    expected = spec.metadata.get("expected_execution")
    return bool(expected) and set(expected) <= set(run.execution_ids)


def release_summary(
    specs: Mapping[str, RunSpec], runs: Mapping[str, DeckRun]
) -> dict[str, Any]:
    """Split measured deck verdicts into lanes by `metadata.expected` and gate the blocking ones.

    Accept decks must complete and accept; not-evaluated decks must complete
    not evaluated for exactly `metadata.expected_not_evaluated`. A blocking
    run that left no verdict is `missing`, one that did not complete is
    `unmeasured`, and either fails the gate. Reject decks are advisory:
    `reproduced` names those that completed rejecting or left their
    `expected_execution`.
    """
    for name, spec in specs.items():
        if spec.metadata.get("expected") not in LANES:
            raise SchemaError(
                DECKS_DECISION,
                f"{name}: expected is {spec.metadata.get('expected')!r}, not one of {LANES}",
            )
        if spec.metadata["expected"] == "not-evaluated" and not isinstance(
            spec.metadata.get("expected_not_evaluated"), list
        ):
            raise SchemaError(
                DECKS_DECISION, f"{name}: expected_not_evaluated is not a list"
            )

    def kind(name: str) -> str:
        run = runs.get(name)
        if run is None or run.kind is None:
            return MISSING
        return run.kind if run.status == COMPLETED else UNMEASURED

    lanes: dict[str, dict[str, str]] = {"accept": {}, "reject": {}, "not-evaluated": {}}
    for name, spec in specs.items():
        lanes[spec.metadata["expected"]][name] = kind(name)
    blocking = {**lanes["accept"], **lanes["not-evaluated"]}
    failures = [name for name, k in lanes["accept"].items() if k != ACCEPT]
    failures += [
        name
        for name, k in lanes["not-evaluated"].items()
        if k != NOT_EVALUATED
        or list(runs[name].not_evaluated)
        != specs[name].metadata["expected_not_evaluated"]
    ]
    return {
        "accept_decks": lanes["accept"],
        "reject_decks": lanes["reject"],
        "not_evaluated_decks": lanes["not-evaluated"],
        "reproduced": sorted(
            name for name in lanes["reject"] if _reproduced(specs[name], runs.get(name))
        ),
        "flip_candidates": [name for name, k in lanes["reject"].items() if k == ACCEPT],
        "missing": sorted(name for name, k in blocking.items() if k == MISSING),
        "unmeasured": sorted(name for name, k in blocking.items() if k == UNMEASURED),
        "failures": sorted(failures),
        "gate": "FAIL" if failures else "PASS",
    }


def run_advisory(
    artifacts: Path,
    *,
    specs: Mapping[str, RunSpec],
    run: Runner,
    opt_in: bool = False,
) -> str:
    """Replay each spec once on the caller's SUT, into `artifacts/<deck>`.

    Replay verdicts remain in each run's ordinary artifacts.
    """
    if not opt_in:
        return "skipped"
    for name, spec in specs.items():
        _advisory_run(run, spec, artifacts / name, RunPurpose.REPLAY)
    return "replayed"
