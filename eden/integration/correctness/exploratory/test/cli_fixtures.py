# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared builders for the CLI, campaign, and findings-report tests.

A real fake-target campaign costs about a second through the CLI, and the
report tests need several. `fake_campaign` runs it once per process
for each distinct shape and hands every caller a private copy whose
absolute paths are rewritten to the caller's root, so a test sees exactly
what a fresh campaign would have written there. `run_cli` runs one CLI verb
with keyword flags, `fake_spec` is a minimal fake write-then-read, and
`tearing_fake` plants a product bug in the CLI's `fake` target.
"""

from __future__ import annotations

import atexit
import dataclasses
import io
import json
import shutil
import sys
import tempfile
from collections.abc import Iterator
from contextlib import contextmanager, redirect_stdout
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory import cli
from eden.integration.correctness.exploratory.core import containment
from eden.integration.correctness.exploratory.core.campaign import summary_row
from eden.integration.correctness.exploratory.core.checker import decode_verdict
from eden.integration.correctness.exploratory.core.containment import (
    Execution,
    ExplorationResult,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.target import Environment
from eden.integration.correctness.exploratory.fake.target import (
    FAKE,
    FakeBackend,
    FakeEnvironment,
    TORN_WRITE,
)


def _tearing_backend(
    environment: Environment, spec: RunSpec, red_control: str | None
) -> FakeBackend:
    """Build the fake backend with every write torn, whatever red control is asked."""
    del red_control
    assert isinstance(environment, FakeEnvironment)
    return FakeBackend(environment, spec, TORN_WRITE)


# The fake target with a planted product bug: every write tears as the
# torn-write red control's does, in every run, with no red control requested,
# so a replay of its run reproduces the finding.
TEARING_FAKE = dataclasses.replace(FAKE, make_backend=_tearing_backend)
_TEARING_RUNNER = (
    "import sys\n"
    "from eden.integration.correctness.exploratory import cli\n"
    "from eden.integration.correctness.exploratory.test.cli_fixtures import "
    "TEARING_FAKE\n"
    "cli.TARGETS['fake'] = TEARING_FAKE\n"
    "sys.exit(cli._main(sys.argv[1:]))\n"
)


@contextmanager
def tearing_fake() -> Iterator[None]:
    """Make the CLI's `fake` target `TEARING_FAKE`, here and in every runner it spawns."""
    spawn = containment.runner_argv_for

    def runner_argv(
        spec_path: str, target_name: str, red_control: str | None = None
    ) -> list[str]:
        argv = spawn(spec_path, target_name, red_control)
        return [sys.executable, "-c", _TEARING_RUNNER, *argv[2:]]

    with (
        mock.patch.dict(cli.TARGETS, {"fake": TEARING_FAKE}),
        mock.patch.object(containment, "runner_argv_for", runner_argv),
    ):
        yield


def fake_spec() -> str:
    """Return a minimal fake-target write then read."""
    return json.dumps(
        {
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
            "actors": [{"id": "a", "actions": ["w", "r"]}],
            "actions": {
                "w": {
                    "kind": "write",
                    "deadline": "operation",
                    "path": "register",
                    "value_id": "written",
                },
                "r": {"kind": "read", "deadline": "operation", "path": "register"},
            },
        }
    )


def cli_argv(verb: str, *positional: str, **flags: Any) -> list[str]:
    """Return the argv for one verb: `seed_start=3` is `--seed-start 3`.

    `True` is a bare flag, `False` and `None` are omitted, and a list or
    tuple repeats its values after one flag.
    """
    argv = [verb, *positional]
    for name, value in flags.items():
        flag = "--" + name.replace("_", "-")
        if value is None or value is False:
            continue
        if value is True:
            argv.append(flag)
        elif isinstance(value, (list, tuple)):
            argv += [flag, *(str(v) for v in value)]
        else:
            argv += [flag, str(value)]
    return argv


def run_cli(verb: str, *positional: str, **flags: Any) -> int:
    """Run one CLI verb with stdout captured; return its exit code."""
    with redirect_stdout(io.StringIO()):
        return cli._main(cli_argv(verb, *positional, **flags))


def _build_fake_campaign(
    root: Path, red_seeds: tuple[int, ...], red_control: str | None
) -> None:
    artifacts = root / "artifacts"
    run_cli(
        "campaign",
        target="fake",
        seeds=1,
        artifacts=artifacts,
        report=root / "report.json",
    )
    spec = root / "spec.json"
    spec.write_text(fake_spec())
    for seed in red_seeds:
        run_dir = artifacts / f"seed-{seed}"
        run_cli(
            "run",
            target="fake",
            spec=spec,
            artifacts=run_dir,
            red_control=red_control,
        )
        verdict = decode_verdict((run_dir / "verdict.json").read_bytes())
        execution = json.loads((run_dir / "execution.json").read_text())
        result = ExplorationResult(
            Execution(
                execution["status"],
                execution["reason"],
                execution["events"],
                execution["returncode"],
            ),
            verdict,
            str(run_dir),
        )
        with open(artifacts / "summary.jsonl", "a") as handle:
            handle.write(
                json.dumps(summary_row(seed, None, None, result), sort_keys=True) + "\n"
            )


_CACHE: dict[tuple[int, tuple[int, ...], str | None], Path] = {}
_CACHE_ROOT: list[Path] = []


def _cache_root() -> Path:
    if not _CACHE_ROOT:
        path = Path(tempfile.mkdtemp(prefix="fake-campaign-"))
        atexit.register(shutil.rmtree, path, True)
        _CACHE_ROOT.append(path)
    return _CACHE_ROOT[0]


def _copy_rewritten(source: Path, root: Path) -> None:
    """Copy a built campaign to `root`, rewriting its absolute paths to `root`."""
    old, new = str(source).encode(), str(root).encode()
    for path in sorted(source.rglob("*")):
        target = root / path.relative_to(source)
        if path.is_dir():
            target.mkdir(parents=True, exist_ok=True)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        data = path.read_bytes()
        target.write_bytes(data.replace(old, new) if old in data else data)
        shutil.copystat(path, target)


def fake_campaign(
    root: Path,
    red_seeds: tuple[int, ...] = (9,),
    red_control: str | None = TORN_WRITE,
) -> Path:
    """Write a real fake-target campaign plus rejecting write-then-read runs under `root`.

    The campaign (one seed) writes `report.json` and `artifacts/`; each red
    seed adds a `fake_spec` run, under `red_control` (by default a torn-write
    red-control run), to the summary as a rejecting seed. The CLI's current
    `fake` target builds it, so a test that swaps that target (under
    `tearing_fake`, with no red control) gets a campaign of its own. Returns
    the artifacts directory.
    """
    key = (id(cli._registry()["fake"]), tuple(red_seeds), red_control)
    built = _CACHE.get(key)
    if built is None:
        built = _cache_root() / str(len(_CACHE))
        built.mkdir()
        _build_fake_campaign(built, tuple(red_seeds), red_control)
        _CACHE[key] = built
    root.mkdir(parents=True, exist_ok=True)
    _copy_rewritten(built, root)
    return root / "artifacts"


NOW = "2026-10-13T12:00:00+00:00"
