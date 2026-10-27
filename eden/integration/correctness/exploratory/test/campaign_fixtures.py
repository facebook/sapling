# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Offline EdenFS campaigns for the golden findings-report tests.

Every run directory is what a real runner leaves: the RunSpec, a history
with the scheduler's close events, `execution.json`, the verdict
`check(EDENFS_TARGET, ...)` computes from them, and `admission.json`.
Nothing is substituted into a verdict: a scenario that needs a finding
writes a synthetic EdenFS history whose facts produce it (`Shape`), and
red-control runs re-judge the checked-in live captures under
`edenfs/test/red_controls/`. The campaign report is `core.campaign.account`
over those runs. Nothing in this module contacts a store or spawns a
process.

`campaign_specs` is the nightly campaign's spec stream (256 files, forced
attempts, the planned strata), for the generator rate and summary tests.
"""

from __future__ import annotations

import json
import shutil
from collections.abc import Iterator, Mapping, Sequence
from dataclasses import asdict, dataclass, replace
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory import findings
from eden.integration.correctness.exploratory.core.campaign import (
    account,
    CampaignAttempt,
    plan,
    report_document,
    summary_row,
)
from eden.integration.correctness.exploratory.core.checker import (
    check,
    encode_verdict,
    Verdict,
)
from eden.integration.correctness.exploratory.core.containment import (
    Execution,
    ExplorationResult,
    load_history,
)
from eden.integration.correctness.exploratory.core.generate import compose
from eden.integration.correctness.exploratory.core.history import Event, event_line
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    RECORDING_TARGET,
)
from eden.integration.correctness.exploratory.schemas import (
    admission_for,
    DaemonProvenance,
    RunPurpose,
    write_admission,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    read_ok,
    Run,
    sl_status,
    TIP,
    value,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.generation_fixtures import (
    sampled_environment,
)

# Every verdict is judged by the property-bearing EdenFS target.
TARGET = EDENFS_TARGET
EXPLORATORY = Path(__file__).parent.parent
RED_CONTROLS = EXPLORATORY / "edenfs" / "test" / "red_controls"
DAEMON = DaemonProvenance(binary_sha256="ab" * 32, source_commit="c" * 40)
R0, R1 = "tree/registers/r0.reg", "tree/registers/r1.reg"


# Histories.


@dataclass(frozen=True)
class Shape:
    """One synthetic EdenFS run: its RunSpec text and its action events."""

    spec_text: str
    events: tuple[Event, ...]


def shape_of(run: Run) -> Shape:
    """Close a written run (`target-closed`, scheduler cleanup) and return its shape."""
    run.finish()
    return Shape(run.spec_text(), tuple(run.events))


def new_run(name: str, description: str) -> Run:
    """Return an empty synthetic run over the corpus fixture (seed, registers, rev3 tip)."""
    return Run("golden", name, description)


def read_args(channel: str) -> dict[str, Any]:
    return {"sync_timeout_seconds": 0} if channel == "thrift" else {}


def written(value_id: str) -> bytes:
    """Return the bytes the write `value_id` of actor `a` stores."""
    return value("a", value_id)


def spliced(value_id: str, path: str = R0) -> bytes:
    """Return the write's first 32 bytes followed by rev3's last 32 of `path`."""
    return written(value_id)[:32] + committed(path, TIP)[32:]


def scripted(
    name: str,
    description: str,
    *,
    writes: Sequence[tuple[str, str]] = (),
    reads: Sequence[tuple[str, str, str, bytes]] = (),
    journal: bool = False,
    status: tuple[Mapping[str, str], Sequence[str]] | None = None,
) -> Shape:
    """Return one actor's run: acked writes, then reads, a journal window, an sl status.

    `writes` are `(value_id, path)`; `reads` are `(action id, path,
    channel, returned bytes)`; `journal` adds a complete window since
    sequence 0 that lists nothing; `status` is sl status's `(entries, diff
    paths)` at the tip parent. Actions run one at a time in that order. The
    RunSpec depends only on the action list and `name`, so replay attempts
    can record other results for the same spec.
    """
    run = new_run(name, description)
    for value_id, path in writes:
        run.act("a", value_id, "write", path=path, value_id=value_id)
    for action_id, path, channel, _data in reads:
        run.act(
            "a", action_id, "read", path=path, channel=channel, **read_args(channel)
        )
    if journal:
        run.act("a", "j", "journal_window", since_sequence=0)
    if status is not None:
        run.act("a", "s", "status", channel="sl")
    for value_id, _path in writes:
        run.call(value_id, "ok", **WRITE_OK)
    for action_id, _path, _channel, data in reads:
        run.call(action_id, "ok", **read_ok(data))
    if journal:
        run.call(
            "j",
            "ok",
            changed_paths=[],
            lost_changes=[],
            mount_generation=1,
            sequence_number=4,
            since_sequence=0,
            truncated=False,
        )
    if status is not None:
        run.call("s", "ok", **sl_status(*status))
    return shape_of(run)


def read_after_write(
    name: str, data: bytes, path: str = R0, channel: str = "fuse"
) -> Shape:
    """Return an acked write `w` of `path`, then one read `r` returning `data`.

    The write's own bytes are legal; rev3's are a stale read; a splice of
    both is a torn read; 64 bytes nobody writes are a torn read too, and
    other widths fabricated.
    """
    return scripted(
        name,
        f"An acked write to {path}, then a {channel} read.",
        writes=[("w", path)],
        reads=[("r", path, channel, data)],
    )


def torn_shape(path: str = R0, channel: str = "fuse") -> Shape:
    """Return a read that splices the acked write onto rev3: `bytes.torn-read[channel]`."""
    return read_after_write(f"torn-{channel}-{path}", spliced("w", path), path, channel)


# Run directories.

COMPLETED = Execution("completed", "ok", 0, 0)


def write_judged_run(
    run_dir: Path,
    spec_text: str,
    history: Sequence[Event],
    execution: Execution = COMPLETED,
) -> Verdict:
    """Write a run's spec, history, and execution, then its real verdict."""
    run_dir.mkdir(parents=True, exist_ok=True)
    (run_dir / "run-spec.json").write_text(spec_text, encoding="utf-8")
    with open(run_dir / "history.jsonl", "wb") as handle:
        for event in history:
            handle.write(event_line(event))
    document = asdict(replace(execution, events=len(history)))
    (run_dir / "execution.json").write_text(
        json.dumps(document, sort_keys=True, indent=2) + "\n"
    )
    verdict = check(
        TARGET,
        from_json(spec_text, TARGET),
        load_history(str(run_dir / "history.jsonl")),
        execution=document,
    )
    (run_dir / "verdict.json").write_bytes(encode_verdict(verdict))
    return verdict


class Campaign:
    """One offline campaign: run directories, `summary.jsonl`, and `report.json`."""

    def __init__(self, root: Path, campaign_id: str = "campaign-1") -> None:
        self.root = root
        self.campaign_id = campaign_id
        self.artifacts = root / "artifacts"
        self.artifacts.mkdir(parents=True)
        (self.artifacts / "summary.jsonl").write_text("")
        self.report = root / "report.json"
        self.attempts: list[CampaignAttempt] = []
        self.write_report()

    def add_run(
        self,
        seed: int,
        shape: Shape,
        *,
        purpose: RunPurpose = RunPurpose.EXPLORE,
        drop: Sequence[str] = (),
        execution: Execution = COMPLETED,
        history: Sequence[Event] | None = None,
    ) -> Path:
        """Write one finished run and its summary row; `drop` removes artifacts after.

        `history` replaces the shape's history (a captured run's).
        """
        run_dir = self.artifacts / f"seed-{seed}"
        events = list(history) if history is not None else list(shape.events)
        verdict = write_judged_run(run_dir, shape.spec_text, events, execution)
        execution = replace(execution, events=len(events))
        write_admission(
            run_dir, admission_for(purpose, self.campaign_id, daemon=DAEMON)
        )
        result = ExplorationResult(execution, verdict, str(run_dir))
        with open(self.artifacts / "summary.jsonl", "a") as summary:
            summary.write(
                json.dumps(summary_row(seed, None, None, result), sort_keys=True) + "\n"
            )
        for name in drop:
            (run_dir / name).unlink()
        self.attempts.append(CampaignAttempt(seed, None, result))
        self.write_report()
        return run_dir

    def write_report(self) -> None:
        """Write `report.json`: `account` over the runs written so far."""
        document = report_document(
            account(self.attempts, (), TARGET),
            {
                "campaign_id": self.campaign_id,
                "daemon_sha256": DAEMON.binary_sha256,
                "daemon_source_commit": DAEMON.source_commit,
            },
        )
        self.report.write_text(json.dumps(document, sort_keys=True, indent=2) + "\n")


def control_shape(control: str) -> tuple[Shape, list[Event]]:
    """Return a red control's captured live RunSpec and history."""
    directory = RED_CONTROLS / control
    spec_text = (directory / "run-spec.json").read_text(encoding="utf-8")
    events = load_history(str(directory / "history.jsonl"))
    return Shape(spec_text, ()), events


def control_runs(root: Path, campaign_id: str = "campaign-1") -> list[Path]:
    """Write one completed run per red control, admitted for one campaign.

    Each run is the control's checked-in live capture re-judged by
    EDENFS_TARGET.
    """
    dirs = []
    controls = findings.target_control_rules(TARGET)
    assert set(controls) == {p.name for p in RED_CONTROLS.iterdir()}, controls
    for control in controls:
        campaign = Campaign(root / "controls" / campaign_id / control, campaign_id)
        shape, events = control_shape(control)
        run_dir = campaign.add_run(
            1,
            shape,
            purpose=RunPurpose.RED_CONTROL,
            history=events,
        )
        directory = root / "red-controls" / campaign_id / control
        directory.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(run_dir, directory)
        dirs.append(directory)
    return dirs


def campaign_specs(
    seeds: range, target: Target = RECORDING_TARGET, **kwargs: Any
) -> Iterator[RunSpec]:
    """The nightly campaign shape: 256 files, forced attempts, and the strata
    `campaign.plan()` assigns a campaign starting at seed 0."""
    for entry in plan(target, 0, seeds.stop, "public-master")[seeds.start :]:
        yield compose(
            target,
            entry.seed,
            sampled_environment(entry.seed, 256),
            stratum=entry.stratum,
            forced=entry.stratum is not None,
            **kwargs,
        )
