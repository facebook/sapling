# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Command line for exploratory runs.

Verbs: validate, check, generate, run, explore, campaign, red-controls,
findings, replay, file, release-summary, and emit; `<verb> --help` describes
each.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time
from dataclasses import asdict, replace
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory import advisory, filing, findings
from eden.integration.correctness.exploratory.core import containment, jsonio
from eden.integration.correctness.exploratory.core.campaign import (
    admitted_strata,
    KEEP_ALL,
    KEEP_CHOICES,
    report_rows,
    retain,
    run_campaign,
    Stratum,
    summary_row,
    write_report,
    write_summary_row,
)
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    check,
    encode_verdict,
)
from eden.integration.correctness.exploratory.core.generate import (
    compose,
    derived_rng,
    shape_signature,
)
from eden.integration.correctness.exploratory.core.runspec import (
    _parse_budgets,
    DEFAULT_RUNNER_DEADLINE_SECONDS,
    from_json,
    RunSpec,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.core.sites import (
    DAEMON_LABELS,
    PUBLIC_MASTER,
)
from eden.integration.correctness.exploratory.core.target import (
    EnvironmentDegraded,
    Target,
)
from eden.integration.correctness.exploratory.schemas import (
    admission_for,
    DaemonProvenance,
    document_digest,
    optional_text,
    require_bool,
    require_text,
    RunPurpose,
    SchemaError,
    strict_loads,
    write_admission,
)
from eden.integration.correctness.exploratory.targets import target_entries, TargetEntry


def campaign_id(fields: dict[str, Any]) -> str:
    """Return the logical campaign ID: the digest of its defining parameters."""
    return document_digest({"campaign": dict(fields)})


# Filled on first use (`_registry`), so importing the CLI builds no target.
ENTRIES: dict[str, TargetEntry] = {}
TARGETS: dict[str, Target] = {}


def _registry() -> dict[str, Target]:
    """Return `TARGETS`, filled from `target_entries()`; a name already present keeps its target."""
    if not ENTRIES:
        ENTRIES.update(target_entries())
    for name, entry in ENTRIES.items():
        TARGETS.setdefault(name, entry.target)
    return TARGETS


def _entry(target_name: str) -> TargetEntry:
    _registry()
    return ENTRIES[target_name]


def _read_spec(path: str, target: Target) -> RunSpec | None:
    """Parse a spec file for a target, printing the error and returning None when invalid."""
    try:
        with open(path) as handle:
            return from_json(handle.read(), target)
    except (OSError, SpecError) as error:
        print(f"{path}: {error}", file=sys.stderr)
        return None


def _cmd_validate(target_name: str, spec_path: str) -> int:
    spec = _read_spec(spec_path, _registry()[target_name])
    if spec is None:
        return 1
    print(f"{spec_path}: valid ({len(spec.actions)} actions)")
    return 0


def _run_code(result: containment.ExplorationResult) -> int:
    """Exit 0 only for a completed run with an accepted verdict."""
    if result.execution.status == containment.COMPLETED and (
        result.verdict.kind == ACCEPT
    ):
        return 0
    return 1


def _cmd_run(
    target_name: str,
    spec_path: str,
    artifact_dir: str,
    red_control: str | None,
    campaign: str | None = None,
) -> int:
    target = _registry()[target_name]
    try:
        with open(spec_path) as handle:
            spec = from_json(handle.read(), target)
    except (OSError, SpecError) as error:
        print(f"{spec_path}: {error}", file=sys.stderr)
        return 2
    provenance, _degradation = _campaign_provenance(target_name)
    result = containment.run_exploration(
        spec, artifact_dir, target=target, red_control=red_control
    )
    purpose = RunPurpose.MANUAL if red_control is None else RunPurpose.RED_CONTROL
    _write_admission(Path(artifact_dir), purpose, campaign, provenance, None)
    print(json.dumps(asdict(result.execution), sort_keys=True))
    return _run_code(result)


def _environment_target(
    target_name: str, seed: int, files: int, differential: bool = False
) -> dict[str, Any]:
    """Sample target-owned environment fields and apply the CLI file budget.

    `differential` samples from the target's differential sampler (the same
    draws, opted into the reference side), so a differential campaign's
    seed composes over the environment its plain twin would, with the
    mount forced case-sensitive, plus the reference checkout.
    """
    entry = _entry(target_name)
    rng = derived_rng(seed, "exploratory-environment")
    sampler = entry.sample_environment
    if differential:
        if entry.sample_differential_environment is None:
            raise ValueError(f"target {target_name} has no differential mode")
        sampler = entry.sample_differential_environment
    return entry.apply_files(sampler(rng), files)


def _compose(
    target_name: str,
    seed: int,
    files: int,
    daemon_label: str,
    stratum: Stratum | None = None,
    no_fault: bool = False,
) -> RunSpec:
    """Compose one validated spec, forcing a stratum when one is selected."""
    target = _registry()[target_name]
    environment = _environment_target(target_name, seed, files)
    return compose(
        target,
        seed,
        environment,
        stratum=stratum,
        forced=stratum is not None,
        daemon_label=daemon_label,
        no_fault=no_fault,
    )


def _stratum_argument(target: Target, text: str, daemon_label: str) -> Stratum:
    """Parse `SITE/BEHAVIOR/KILL` and require it to be an admitted stratum."""
    parts = tuple(text.split("/"))
    if len(parts) != 3 or parts not in admitted_strata(target, daemon_label):
        raise ValueError(f"stratum {text!r} is not admitted on daemon {daemon_label!r}")
    return parts[0], parts[1], parts[2]


def _cmd_generate(
    target_name: str,
    seed: int,
    files: int,
    stratum: str | None,
    daemon_label: str,
    no_fault: bool,
) -> int:
    target = _registry()[target_name]
    selected = None
    try:
        if no_fault and stratum is not None:
            raise ValueError("--no-fault takes no --stratum")
        if stratum is not None:
            selected = _stratum_argument(target, stratum, daemon_label)
    except ValueError as error:
        print(f"generate: {error}", file=sys.stderr)
        return 2
    spec = _compose(target_name, seed, files, daemon_label, selected, no_fault)
    print(to_json(spec), end="")
    return 0


def _cmd_explore(
    seeds: int,
    seed_start: int,
    files: int,
    artifact_dir: str,
    target_name: str,
) -> int:
    target = _registry()[target_name]

    os.makedirs(artifact_dir, exist_ok=True)
    summary_path = os.path.join(artifact_dir, "summary.jsonl")
    provenance, _degradation = _campaign_provenance(target_name)
    passed = 0
    with open(summary_path, "w") as summary:
        for seed in range(seed_start, seed_start + seeds):
            spec = _compose(target_name, seed, files, PUBLIC_MASTER)
            seed_dir = os.path.join(artifact_dir, f"seed-{seed}")
            result = containment.run_exploration(spec, seed_dir, target=target)
            _write_admission(
                Path(seed_dir),
                RunPurpose.MANUAL,
                None,
                provenance,
                None,
            )
            passed += _run_code(result) == 0
            write_summary_row(
                summary,
                summary_row(seed, None, shape_signature(spec, target), result),
            )
    print(f"{seeds} seeds: {passed} pass, {seeds - passed} fail")
    return 0 if passed == seeds else 1


def _cmd_campaign(
    seeds: int,
    seed_start: int,
    files: int,
    artifact_dir: str,
    target_name: str,
    report_path: str | None,
    daemon_label: str,
    keep: str,
    no_fault: bool,
    differential: bool = False,
    runner_deadline: float = DEFAULT_RUNNER_DEADLINE_SECONDS,
) -> int:
    """Run one stratified campaign, a fault-free one, or a differential one, and print its acceptance summary.

    A differential campaign is its own campaign (its ID names the flag):
    every seed composes a paired RunSpec over the target's differential
    environment. `runner_deadline` is every composed RunSpec's whole-run
    deadline; a non-default one is part of the campaign ID.
    """
    target = _registry()[target_name]
    if differential and (
        no_fault or _entry(target_name).sample_differential_environment is None
    ):
        print(
            f"campaign: --differential needs a differential target and no"
            f" --no-fault ({target_name})",
            file=sys.stderr,
        )
        return 2
    provenance, degradation = _campaign_provenance(target_name)
    identity = (
        f"sha256:{provenance['daemon_sha256']}"
        if provenance
        else _daemon_identity(target_name)
    )
    campaign = campaign_id(
        {
            "target": target.name,
            "semantics_profile": target.semantics_profile,
            "seed_start": seed_start,
            "seeds": seeds,
            "files": files,
            "daemon_label": daemon_label,
            "no_fault": no_fault,
            "daemon_identity": identity,
            **({"fatal_eden_bug": provenance["fatal_eden_bug"]} if provenance else {}),
            **({"differential": True} if differential else {}),
            **(
                {"runner_deadline_seconds": runner_deadline}
                if runner_deadline != DEFAULT_RUNNER_DEADLINE_SECONDS
                else {}
            ),
        }
    )
    extra = {"daemon_identity": identity, "campaign_id": campaign, **provenance}
    report = run_campaign(
        target,
        seeds,
        seed_start,
        lambda seed: _environment_target(target_name, seed, files, differential),
        artifact_dir,
        lambda spec, path, selected: _run_campaign_seed(
            spec, path, selected, provenance, degradation, keep, campaign
        ),
        daemon_label,
        no_fault=no_fault,
        differential=differential,
        budgets=_parse_budgets({"runner_deadline_seconds": runner_deadline}),
        extra=extra,
    )
    covered = len(report.completed_strata)
    observable = covered + len(report.missing_strata)
    print(
        f"{report.attempts} attempts: {report.completed} completed, "
        f"{report.harness_failures} harness failures, "
        f"{report.environment_degraded} environment degraded; "
        f"{covered}/{observable} observable strata covered"
    )
    if report_path is not None:
        write_report(report_path, report, extra)
    return int(
        report.completed != report.attempts
        or report.verdicts.get(ACCEPT, 0) != report.attempts
        or bool(report.missing_strata)
    )


def _file_sha256(path: str) -> str:
    """Hash a binary without reading it all into memory."""
    digest = hashlib.sha256()
    with open(path, "rb") as daemon:
        while chunk := daemon.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _daemon_identity(target_name: str) -> str:
    """Return a stable identity for the daemon exercised by a campaign."""
    if target_name != "edenfs":
        return target_name
    path = os.environ.get("EDENFS_SERVER_PATH")
    return f"sha256:{_file_sha256(path)}" if path is not None else "edenfs:unresolved"


# The variable that names each binary a live run launches; `FindExe` reads
# the same variables before any fallback (eden/integration/lib/find_executables.py),
# so the hashed `edenfs` is the daemon the run starts.
BINARY_ENV = {
    "edenfs": "EDENFS_SERVER_PATH",
    "privhelper": "EDENFS_PRIVHELPER_PATH",
    "edenfsctl": "EDENFSCTL_RUST_PATH",
    "edenfsctl.real": "EDENFSCTL_REAL_PATH",
    "sl": "EDEN_HG_BINARY",
    "hg.real": "HG_REAL_BIN",
}


def _source_timestamp() -> float | None:
    """Return `EDENFS_DAEMON_SOURCE_TIMESTAMP` as a finite number of seconds, or None when unset."""
    text = os.environ.get("EDENFS_DAEMON_SOURCE_TIMESTAMP")
    if text is None:
        return None
    value = strict_loads(text, "provenance:invalid")
    if type(value) not in (int, float):
        raise SchemaError(
            "provenance:invalid", f"EDENFS_DAEMON_SOURCE_TIMESTAMP {text!r}"
        )
    return float(value)


def _campaign_provenance(target_name: str) -> tuple[dict[str, Any], str | None]:
    """Snapshot supplied build provenance and assess its source age once.

    `binaries` names each supplied binary's path and sha256; an unset
    variable is recorded as null. Every field is checked as the admission
    reader checks it, and the daemon must be supplied, so a malformed
    provenance raises `SchemaError` before any run starts.
    """
    source_commit = os.environ.get("EDENFS_DAEMON_SOURCE_COMMIT")
    if target_name != "edenfs" or source_commit is None:
        return {}, None
    decision = "provenance:invalid"
    paths = {name: os.environ.get(env) for name, env in BINARY_ENV.items()}
    if paths["edenfs"] is None:
        raise SchemaError(decision, f"{BINARY_ENV['edenfs']} is unset")
    try:
        binaries = {
            name: {"path": path, "sha256": None if path is None else _file_sha256(path)}
            for name, path in paths.items()
        }
    except OSError as error:
        raise SchemaError(decision, str(error)) from error
    fatal = os.environ.get("EDENFS_DAEMON_FATAL_EDEN_BUG")
    provenance = {
        "daemon_sha256": binaries["edenfs"]["sha256"],
        "daemon_source_commit": require_text(
            source_commit, "EDENFS_DAEMON_SOURCE_COMMIT", decision=decision
        ),
        "fatal_eden_bug": require_bool(
            strict_loads(fatal or "null", decision),
            "EDENFS_DAEMON_FATAL_EDEN_BUG",
            decision=decision,
        ),
        "daemon_log": optional_text(
            os.environ.get("EDENFS_DAEMON_LOG"), "EDENFS_DAEMON_LOG", decision=decision
        ),
        "binaries": binaries,
    }
    timestamp = _source_timestamp()
    if timestamp is None:
        raise SchemaError(decision, "EDENFS_DAEMON_SOURCE_TIMESTAMP is unset")
    stale = time.time() - timestamp > 7 * 24 * 60 * 60
    return provenance, "daemon source commit older than 7 days" if stale else None


def _daemon_provenance(provenance: dict[str, Any]) -> DaemonProvenance | None:
    """Reference the exercised daemon by digest and source; its bytes are never copied."""
    if not provenance:
        return None
    return DaemonProvenance(
        binary_sha256=provenance["daemon_sha256"],
        source_commit=provenance["daemon_source_commit"],
        source_timestamp=_source_timestamp(),
        fatal_eden_bug=provenance["fatal_eden_bug"],
        binary_ref=None,
        log=provenance.get("daemon_log"),
    )


def _write_admission(
    run_dir: Path,
    purpose: RunPurpose,
    campaign: str | None,
    provenance: dict[str, Any],
    degradation: str | None,
) -> None:
    """Write `admission.json` beside a finished run; execution.json is never touched."""
    if not run_dir.is_dir():
        return
    write_admission(
        run_dir,
        admission_for(
            purpose,
            campaign,
            environment_degraded=degradation,
            daemon=_daemon_provenance(provenance),
        ),
    )


def _run_campaign_seed(
    spec: RunSpec,
    path: str,
    target: Target,
    provenance: dict[str, Any],
    degradation: str | None,
    keep: str,
    campaign: str,
) -> containment.ExplorationResult:
    """Apply daemon preflight, write the run's admission, then apply retention."""

    def preflight() -> None:
        if degradation is not None:
            raise EnvironmentDegraded(degradation, provenance)

    result = containment.run_exploration(spec, path, target=target, setup=preflight)
    _write_admission(
        Path(path),
        RunPurpose.EXPLORE,
        campaign,
        provenance,
        degradation,
    )
    return replace(result, dir=retain(path, result, keep))


def _cmd_findings(
    target_name: str, report: str, artifacts: str | None, red_controls: list[str] | None
) -> int:
    """Write the findings report under `findings/` next to the campaign report and print it."""
    report_path = Path(report)
    try:
        lines = findings.write_findings(
            report_path,
            Path(artifacts) if artifacts is not None else report_path.parent,
            report_path.parent / "findings",
            _registry()[target_name],
            None if red_controls is None else [Path(d) for d in red_controls],
        )
    except (OSError, ValueError) as error:
        print(f"findings: campaign refused: {error}", file=sys.stderr)
        return 2
    for line in lines:
        print(line)
    return int(any(line.startswith("FAIL ") for line in lines))


def _replay(target_name: str, run: Path, artifacts: Path) -> findings.Replayed:
    """Re-run one run directory's exact RunSpec into `artifacts` and compare its task keys."""
    target = _registry()[target_name]
    provenance, _degradation = _campaign_provenance(target_name)

    def runner(spec: RunSpec, directory: Path) -> None:
        containment.run_exploration(spec, str(directory), target=target)
        _write_admission(directory, RunPurpose.REPLAY, None, provenance, None)

    return findings.replay(run, artifacts, target, runner)


def _cmd_replay(target_name: str, run: str, artifacts: str) -> int:
    """Re-run one run directory's exact RunSpec once and print both runs' task keys."""
    try:
        replayed = _replay(target_name, Path(run), Path(artifacts))
    except (OSError, SpecError, ValueError) as error:
        print(f"replay: refused: {error}", file=sys.stderr)
        return 2
    print("\n".join(replayed.lines))
    return int(bool(replayed.original - replayed.replayed))


def _cmd_file(args: argparse.Namespace) -> int:
    """Gate and classify a campaign's findings against the parent's subtasks; file unless a dry run."""
    options = filing.Options(
        parent=args.parent,
        owner=args.owner,
        tag=args.tag,
        artifacts_url=args.artifacts_url,
        daemon_package=args.daemon_package,
        dry_run=not args.file,
        skip_replay=args.skip_replay,
    )
    try:
        code, lines = filing.file_findings(
            Path(args.findings),
            _registry()[args.target],
            filing.MetaTasks(args.act_as_crewmate),
            options,
            lambda run, artifacts: _replay(args.target, run, artifacts),
        )
    except (OSError, SpecError, ValueError, filing.TasksError) as error:
        print(f"file: refused: {error}", file=sys.stderr)
        return 2
    print("\n".join(lines))
    return code


def _cmd_emit(report_path: str, findings_path: str | None) -> int:
    """Print a report's fixed scalar rows as JSON lines; no row leaves the process."""
    try:
        report = findings.read_report(Path(report_path))
        found = (
            None
            if findings_path is None
            else findings.read_findings(Path(findings_path))
        )
    except (OSError, ValueError) as error:
        print(f"emit: {error}", file=sys.stderr)
        return 2
    for row in report_rows(report, found):
        print(json.dumps(row, sort_keys=True))
    return 0


def _cmd_check(
    target_name: str,
    replay_path: str | None,
    spec_path: str | None,
    history_path: str | None,
    execution_path: str | None,
) -> int:
    """Print the canonical verdict.json for a replay bundle or spec plus history.

    The execution document decides whether the history must be complete:
    it is read from `--execution`, else from `execution.json` beside a
    replay bundle; without one, a durable prefix is judged as a prefix.
    """
    target = _registry()[target_name]
    try:
        if replay_path is not None:
            spec, events = containment.load_replay(replay_path, target)
            sibling = Path(replay_path).with_name("execution.json")
            if execution_path is None and sibling.is_file():
                execution_path = str(sibling)
        elif spec_path is not None and history_path is not None:
            with open(spec_path) as handle:
                spec = from_json(handle.read(), target)
            events = containment.load_history(history_path)
        else:
            print("check needs --replay or both --spec and --history", file=sys.stderr)
            return 2
        execution = (
            None
            if execution_path is None
            else jsonio.loads(Path(execution_path).read_text())
        )
    except (OSError, SpecError, ValueError) as error:
        print(f"check: {error}", file=sys.stderr)
        return 1
    verdict = check(target, spec, events, execution=execution)
    sys.stdout.write(encode_verdict(verdict).decode("ascii"))
    return 0 if verdict.kind == ACCEPT else 1


def _main(argv: list[str]) -> int:
    if "--runner" in argv:
        parser = argparse.ArgumentParser(prog="cli --runner")
        parser.add_argument("--runner", action="store_true")
        parser.add_argument("--spec", required=True)
        parser.add_argument("--target", required=True, choices=_registry())
        parser.add_argument("--red-control", default=None)
        args = parser.parse_args(argv)
        return containment.runner_main(
            args.spec, _registry()[args.target], args.red_control
        )
    parser = argparse.ArgumentParser(prog="explore", description=__doc__)
    verbs = parser.add_subparsers(dest="verb", required=True)
    validate = verbs.add_parser("validate", help="reject malformed RunSpecs")
    validate.add_argument("--target", required=True, choices=_registry())
    validate.add_argument("--spec", required=True)
    run = verbs.add_parser("run", help="run one spec under containment")
    run.add_argument("--target", required=True, choices=_registry())
    run.add_argument("--spec", required=True)
    run.add_argument("--artifacts", required=True)
    run.add_argument("--red-control", default=None)
    run.add_argument(
        "--campaign-id",
        default=None,
        help="campaign a --red-control run calibrates; recorded in its admission",
    )
    checker = verbs.add_parser("check", help="offline verdict without spawning")
    checker.add_argument("--target", required=True, choices=_registry())
    checker.add_argument("--replay", default=None)
    checker.add_argument("--spec", default=None)
    checker.add_argument("--history", default=None)
    checker.add_argument(
        "--execution", default=None, help="execution.json deciding completeness"
    )
    generate = verbs.add_parser("generate", help="print one generated spec")
    generate.add_argument("--target", required=True, choices=_registry())
    generate.add_argument("--seed", type=int, required=True)
    generate.add_argument("--files", type=int, default=32)
    generate.add_argument(
        "--stratum",
        default=None,
        help="SITE/BEHAVIOR/KILL: a fault site, its behavior, and the kill mode "
        "(none, or process: kill one spawned process, never the daemon)",
    )
    generate.add_argument("--daemon", choices=DAEMON_LABELS, default=PUBLIC_MASTER)
    generate.add_argument(
        "--no-fault", action="store_true", help="skip the fault stage"
    )
    explore = verbs.add_parser("explore", help="generate and run a seed range")
    explore.add_argument("--seeds", type=int, required=True)
    explore.add_argument("--seed-start", type=int, default=0)
    explore.add_argument("--files", type=int, default=32)
    explore.add_argument("--artifacts", required=True)
    explore.add_argument("--target", required=True, choices=_registry())
    campaign = verbs.add_parser("campaign", help="run a stratified campaign")
    campaign.add_argument("--seeds", type=int, required=True)
    campaign.add_argument("--seed-start", type=int, default=0)
    campaign.add_argument("--files", type=int, default=256)
    campaign.add_argument("--artifacts", required=True)
    campaign.add_argument("--target", required=True, choices=_registry())
    campaign.add_argument("--report")
    campaign.add_argument("--daemon", choices=DAEMON_LABELS, default=PUBLIC_MASTER)
    campaign.add_argument(
        "--keep",
        choices=KEEP_CHOICES,
        default=KEEP_ALL,
        help="rejects: drop accepted seeds' stderr, move the rest under rejects/",
    )
    campaign.add_argument(
        "--no-fault", action="store_true", help="unstratified specs with no fault stage"
    )
    campaign.add_argument(
        "--differential",
        action="store_true",
        help="paired runs against the plain reference checkout (its own campaign ID)",
    )
    campaign.add_argument(
        "--runner-deadline",
        type=float,
        default=DEFAULT_RUNNER_DEADLINE_SECONDS,
        help="whole-run deadline in seconds of every composed RunSpec",
    )
    controls = verbs.add_parser("red-controls", help="list a target's red controls")
    controls.add_argument("--target", required=True, choices=_registry())
    finder = verbs.add_parser(
        "findings",
        help="report a campaign's task keys into findings/ next to it",
        description="Nightly layout: --report OUT/report.json --artifacts OUT/artifacts"
        " (seed-N dirs and summary.jsonl; when omitted, the report's own"
        " directory, which is not this layout's)"
        " --red-controls OUT/red-controls/NAME... (one `run --red-control NAME"
        " --campaign-id ID` artifact dir each, run for this campaign).",
    )
    finder.add_argument("--target", required=True, choices=_registry())
    finder.add_argument("--report", required=True)
    finder.add_argument("--artifacts", default=None, help="campaign artifact dir")
    finder.add_argument("--red-controls", nargs="+", default=None, metavar="DIR")
    replayer = verbs.add_parser(
        "replay", help="re-run one finding's run directory and compare its task keys"
    )
    replayer.add_argument("--target", required=True, choices=_registry())
    replayer.add_argument("--run", required=True, help="run directory to replay")
    replayer.add_argument("--artifacts", required=True, help="new artifact dir")
    filer = verbs.add_parser(
        "file",
        help="file a campaign's findings as subtasks of the findings parent task",
        description="Dry run unless --file: reads the parent's subtasks and writes"
        " FINDINGS/filing.json. --file replays each new or regressed key once and"
        " creates its subtask only when the replay reproduces the key.",
    )
    filer.add_argument("--target", required=True, choices=_registry())
    filer.add_argument(
        "--findings", required=True, help="findings dir that `findings` wrote"
    )
    filer.add_argument("--parent", default=filing.FINDINGS_PARENT)
    filer.add_argument("--owner", required=True, help="owner of created subtasks")
    filer.add_argument("--tag", default=filing.DEFAULT_TAG)
    filer.add_argument(
        "--artifacts-url", default=None, help="where the campaign's artifacts live"
    )
    filer.add_argument(
        "--daemon-package", default=None, help="the daemon's package, e.g. NAME:UUID"
    )
    mode = filer.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="the default")
    mode.add_argument("--file", action="store_true", help="create and comment")
    filer.add_argument(
        "--skip-replay", action="store_true", help="file new keys without a replay"
    )
    filer.add_argument(
        "--act-as-crewmate",
        default=None,
        metavar="FBID",
        help="run every task read and write as this crewmate (`meta --act-as-crewmate`)",
    )
    summary = verbs.add_parser("release-summary", help="lanes from deck verdicts")
    summary.add_argument("--target", required=True, choices=_registry())
    summary.add_argument("--artifacts", required=True, help="advisory artifact dir")
    summary.add_argument("--decks", required=True, help="directory of deck RunSpecs")
    emit = verbs.add_parser("emit", help="print a report's scalar rows as JSON lines")
    emit.add_argument("--report", required=True, help="campaign report.json")
    emit.add_argument("--findings", default=None, help="findings/findings.json")
    args = parser.parse_args(argv)
    try:
        return _dispatch(args)
    except SchemaError as error:
        print(f"{args.verb}: {error}", file=sys.stderr)
        return 2


def _dispatch(args: argparse.Namespace) -> int:
    """Run the selected verb; a malformed input document raises `SchemaError`."""
    if args.verb == "emit":
        return _cmd_emit(args.report, args.findings)
    if args.verb == "replay":
        return _cmd_replay(args.target, args.run, args.artifacts)
    if args.verb == "file":
        return _cmd_file(args)
    if args.verb == "release-summary":
        target = _registry()[args.target]
        summary = advisory.release_summary(
            advisory.load_specs(Path(args.decks), target),
            advisory.load_runs(Path(args.artifacts), target),
        )
        print(json.dumps(summary, indent=2, sort_keys=True))
        return 0 if summary["gate"] == "PASS" else 1
    if args.verb == "red-controls":
        print("\n".join(sorted(_registry()[args.target].red_controls)))
        return 0
    if args.verb == "findings":
        return _cmd_findings(
            args.target, args.report, args.artifacts, args.red_controls
        )
    if args.verb == "validate":
        return _cmd_validate(args.target, args.spec)
    if args.verb == "run":
        return _cmd_run(
            args.target, args.spec, args.artifacts, args.red_control, args.campaign_id
        )
    if args.verb == "generate":
        return _cmd_generate(
            args.target, args.seed, args.files, args.stratum, args.daemon, args.no_fault
        )
    if args.verb == "explore":
        return _cmd_explore(
            args.seeds,
            args.seed_start,
            args.files,
            args.artifacts,
            args.target,
        )
    if args.verb == "campaign":
        return _cmd_campaign(
            args.seeds,
            args.seed_start,
            args.files,
            args.artifacts,
            args.target,
            args.report,
            args.daemon,
            args.keep,
            args.no_fault,
            args.differential,
            args.runner_deadline,
        )
    return _cmd_check(args.target, args.replay, args.spec, args.history, args.execution)


def main() -> None:
    """Buck par entry point."""
    raise SystemExit(_main(sys.argv[1:]))


if __name__ == "__main__":
    main()
