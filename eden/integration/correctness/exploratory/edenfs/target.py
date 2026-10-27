# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The EdenFS targets: the first checker target and the recording-only target.

`EDENFS_TARGET` composes the EdenFS action vocabulary, fault sites,
environment, live backends, effect contracts, and pinned fixture facts
with the properties in `PROPERTIES`. Its red controls are those
properties' union, its serving
paths map every observation channel to its independence group, and its
contracts hook supports the contract rules some registered property
declares, plus `core.input-legality`. The CLI's `edenfs` target is this one (`targets.py`).

`RECORDING_TARGET` registers no property. It generates, executes, and
captures runs; `check()` never evaluates one (not-evaluated,
`no-evaluation`). A run with ``setup_profile.reference_clone`` pairs every
shared action with the plain reference checkout through
``DifferentialBackend``; any other run uses ``LiveBackend`` unchanged.
"""

from __future__ import annotations

import os
import random
from collections.abc import Mapping
from dataclasses import replace
from pathlib import Path
from typing import Any, cast

from eden.integration.correctness.exploratory.core.containment import (
    ExplorationResult,
    run_exploration,
    runner_argv_for,
    runner_main,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP_REMOVED,
    HistorySink,
)
from eden.integration.correctness.exploratory.core.property import (
    control_rules,
    PropertyDef,
)
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.target import (
    Backend,
    Environment,
    RUN_DIR_ENV,
    Target,
    TargetSources,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    CASE_INSENSITIVE_MOUNT,
    expects_reference,
    GENERATION,
    reference_clone,
    ReferenceNotApplicable,
    repository,
    sample_fault_install,
    timeout_capable,
)
from eden.integration.correctness.exploratory.edenfs.contracts import make_contracts
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    INPUT_LEGALITY_RULE,
    pinned_facts,
)
from eden.integration.correctness.exploratory.edenfs.properties.attributes import (
    ATTRIBUTES_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.bytes import (
    BYTES_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.checkout import (
    CHECKOUT_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    SERVING_PATHS,
)
from eden.integration.correctness.exploratory.edenfs.properties.cross_channel import (
    CROSS_CHANNEL_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.differential import (
    DIFFERENTIAL_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.existence import (
    EXISTENCE_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.fd import FD_PROPERTY
from eden.integration.correctness.exploratory.edenfs.properties.journal import (
    JOURNAL_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.listing import (
    LISTING_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.manifest import (
    MANIFEST_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.namespace import (
    NAMESPACE_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.readdir import (
    READDIR_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.result_shape import (
    RESULT_SHAPE_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.properties.status import (
    STATUS_PROPERTY,
)
from eden.integration.correctness.exploratory.edenfs.sites import SITES

SEMANTICS_PROFILE = "edenfs-linux-fuse-v1"
ENVIRONMENT_FIELDS: Mapping[str, type] = {
    "repository": dict,
    "channel": str,
    "case_sensitive": bool,
    "fixture_source": str,
    "setup_profile": dict,
}

_MIB = 1024 * 1024
_MEMORY_QUANTUM_MB = 256
_MIN_MEMORY_MAX_MB = 1024
_OBSERVED_MEMORY_PEAK_BYTES = 362_426_368


def _calibrated_memory_max_mb(multiplier: int) -> int:
    """Round an observed-peak multiple up to the supported memory quantum."""
    required_mb = (_OBSERVED_MEMORY_PEAK_BYTES * multiplier + _MIB - 1) // _MIB
    rounded_mb = (
        (required_mb + _MEMORY_QUANTUM_MB - 1) // _MEMORY_QUANTUM_MB
    ) * _MEMORY_QUANTUM_MB
    return max(_MIN_MEMORY_MAX_MB, rounded_mb)


_MEMORY_MAX_MB_CHOICES = (
    None,
    *(_calibrated_memory_max_mb(multiplier) for multiplier in (2, 3, 4)),
)


def _make_environment(spec: RunSpec, sink: HistorySink) -> Environment:
    # The live environment and backend need buck-generated modules; the
    # checker side of the target imports without them.
    from eden.integration.correctness.exploratory.edenfs import (
        environment as edenfs_environment,
    )
    from eden.integration.correctness.exploratory.edenfs.reference import (
        refuse_inapplicable,
    )

    refuse_inapplicable(spec)
    repo = repository(spec)
    target_environment = spec.environment.target
    setup_profile = target_environment["setup_profile"]
    # Attribute lookup at call time: tests substitute the module's Environment.
    return edenfs_environment.Environment(
        spec.seed,
        repo["file_count"],
        repo["commit_count"],
        registers=tuple(repo["registers"]),
        case_sensitive=target_environment["case_sensitive"],
        materialize=setup_profile["materialize"],
        scatter_dirty=setup_profile["scatter_dirty"],
        memory_max_mb=setup_profile.get("memory_max_mb"),
        warm_readdir=setup_profile.get("warm_readdir", False),
        aged_overlay=setup_profile.get("aged_overlay", 0),
        wide_dir=setup_profile.get("wide_dir"),
        type_changes=repo.get("type_changes", ()),
        blob_size=repo.get("blob_size"),
        reference_clone=reference_clone(spec),
        run_dir=Path(os.environ[RUN_DIR_ENV]),
        sink=sink,
    )


def _make_backend(
    environment: Environment, spec: RunSpec, red_control: str | None
) -> Backend:
    from eden.integration.correctness.exploratory.edenfs import (
        environment as edenfs_environment,
    )
    from eden.integration.correctness.exploratory.edenfs.backends import LiveBackend
    from eden.integration.correctness.exploratory.edenfs.differential_backend import (
        DifferentialBackend,
    )
    from eden.integration.correctness.exploratory.edenfs.reference import (
        ReferenceBackend,
    )

    env = cast(edenfs_environment.Environment, environment)
    primary = LiveBackend(env, spec, red_control)
    if not reference_clone(spec):
        return primary
    reference = env.reference
    return DifferentialBackend(
        primary, ReferenceBackend(reference, spec), spec, runtime=reference.runtime
    )


# Share of sampled environments that start with no scattered dirt. Starting
# dirt moves with the earliest replaced generated paths, so with it every
# NORMAL checkout to an earlier revision conflicts; clean starts keep
# successful NORMAL checkouts common.
CLEAN_START_RATE = 0.5


def sample_environment(rng: random.Random) -> Mapping[str, Any]:
    """Sample an EdenFS environment from calibrated setup dimensions."""
    file_count = 32
    clean_start = rng.random() < CLEAN_START_RATE
    return {
        "repository": {
            "file_count": file_count,
            "commit_count": 4,
            "registers": ["tree/registers/r0.reg", "tree/registers/r1.reg"],
        },
        "channel": "fuse",
        "case_sensitive": rng.choice((True, False)),
        "fixture_source": "backing",
        "setup_profile": {
            "materialize": rng.choice((True, False)),
            "scatter_dirty": 0 if clean_start else rng.randint(1, file_count),
            "memory_max_mb": rng.choice(_MEMORY_MAX_MB_CHOICES),
            "warm_readdir": rng.choice((True, False)),
            "aged_overlay": rng.randrange(4),
        },
    }


def with_reference_clone(environment: Mapping[str, Any]) -> dict[str, Any]:
    """Return a sampled environment opted into the differential reference checkout.

    The default sampler never sets ``reference_clone``, so same-seed
    generation stays byte-identical; a targeted differential campaign or a
    deck opts in explicitly here. A case-insensitive environment raises
    ReferenceNotApplicable rather than producing a spec validation refuses.
    """
    if not environment["case_sensitive"]:
        raise ReferenceNotApplicable(CASE_INSENSITIVE_MOUNT)
    return {
        **environment,
        "setup_profile": {**environment["setup_profile"], "reference_clone": True},
    }


def sample_differential_environment(rng: random.Random) -> dict[str, Any]:
    """Sample an environment a differential run admits: case-sensitive, with a reference clone.

    The same draws as `sample_environment`; only the mount's case
    sensitivity is fixed, since the reference checkout is a case-sensitive
    plain filesystem.
    """
    return with_reference_clone({**sample_environment(rng), "case_sensitive": True})


def install_witnessed(_spec: RunSpec, facts: HistoryFacts, install: Action) -> bool:
    """Return whether the fault's await recorded a hit, or its cleanup-removed
    facts show that it fired.

    A completed `fault_await` with `hit: true` saw the target parked, even
    when the run stops before unwind records cleanup. Unwind records `hit`
    when a blocking install parked its target and `miss` when nothing
    reached the site; error and delay installs never park, so a consumed
    one reports `hit: false, miss: false`. Only the first cleanup record
    for the fault counts, and no modeled state is consulted.
    """
    fault_id = install.payload.get("fault_id")
    for awaited in facts.actions_of("fault_await"):
        if (
            awaited.payload.get("fault_id") == fault_id
            and awaited.result is not None
            and awaited.result.fields.get("hit") is True
        ):
            return True
    for record in facts.run_events:
        if record.type == CLEANUP_REMOVED and record.data.get("fault_id") == fault_id:
            return record.data.get("hit") is True or record.data.get("miss") is False
    return False


ACTIONS = {
    **ACTION_DEFS,
    "fault_install": replace(ACTION_DEFS["fault_install"], sample=sample_fault_install),
}


_SOURCES = TargetSources(
    implementation=__name__,
    external=(
        "eden.fs",
        "eden.integration.lib",
        "eden.test_support",
        "eden.thrift",
        "thrift",
    ),
)

RECORDING_TARGET = Target(
    name="edenfs",
    actions=ACTIONS,
    generation=GENERATION,
    sites=SITES,
    properties=(),
    install_witnessed=install_witnessed,
    environment_fields=ENVIRONMENT_FIELDS,
    make_environment=_make_environment,
    make_backend=_make_backend,
    red_controls=frozenset(),
    semantics_profile=SEMANTICS_PROFILE,
    timeout_capable=timeout_capable,
    sources=_SOURCES,
    contracts=make_contracts((INPUT_LEGALITY_RULE,)),
    pinned=pinned_facts,
    expects_reference=expects_reference,
)

PROPERTIES: tuple[PropertyDef, ...] = (
    BYTES_PROPERTY,
    STATUS_PROPERTY,
    JOURNAL_PROPERTY,
    CROSS_CHANNEL_PROPERTY,
    NAMESPACE_PROPERTY,
    CHECKOUT_PROPERTY,
    EXISTENCE_PROPERTY,
    MANIFEST_PROPERTY,
    LISTING_PROPERTY,
    RESULT_SHAPE_PROPERTY,
    FD_PROPERTY,
    ATTRIBUTES_PROPERTY,
    READDIR_PROPERTY,
    DIFFERENTIAL_PROPERTY,
)
RED_CONTROLS: frozenset[str] = frozenset(control_rules(PROPERTIES))
# Result fields the contracts hook reads: indeterminate-write and Thrift
# read and readdir failure classification, killed or rejected Sapling
# commands, and
# refused statuses.
CONTRACT_CONSUMES: frozenset[tuple[str, str]] = frozenset(
    {
        ("write", "close_errno"),
        ("read", "error_code"),
        ("readdir", "error_type"),
        ("sl_rm", "returncode"),
        ("sl_mv", "returncode"),
        ("checkout", "returncode"),
        ("checkout", "stderr"),
        ("checkout", "error_type"),
        ("status", "returncode"),
        ("status", "stderr"),
    }
)
# Consumed result fields no property reads, each with the reason.
_JOURNAL_PRESENCE_UNPORTED = (
    "the backend folds a path's journal changes into one entry, and "
    "changesSinceV2 walks the journal newest first, so the entry is the path's "
    "oldest change since sequence zero (possibly a setup write), at no known "
    "point, and a checkout changes presence only through a commitTransition "
    "that names no path: neither flag is the path's presence at a window end"
)
UNPORTED_FIELDS: Mapping[tuple[str, str], str] = {
    ("journal_window", "changed_paths.existed_before"): _JOURNAL_PRESENCE_UNPORTED,
    ("journal_window", "changed_paths.existed_after"): _JOURNAL_PRESENCE_UNPORTED,
}


def _registered_rules(properties: tuple[PropertyDef, ...]) -> frozenset[str]:
    return frozenset(rule for definition in properties for rule in definition.rules)


EDENFS_TARGET = Target(
    name="edenfs",
    actions=ACTIONS,
    generation=GENERATION,
    sites=SITES,
    properties=PROPERTIES,
    install_witnessed=install_witnessed,
    environment_fields=ENVIRONMENT_FIELDS,
    make_environment=_make_environment,
    make_backend=_make_backend,
    red_controls=RED_CONTROLS,
    semantics_profile=SEMANTICS_PROFILE,
    serving_paths=SERVING_PATHS,
    timeout_capable=timeout_capable,
    sources=_SOURCES,
    contracts=make_contracts(_registered_rules(PROPERTIES) | {INPUT_LEGALITY_RULE}),
    pinned=pinned_facts,
    expects_reference=expects_reference,
    contract_consumes=CONTRACT_CONSUMES,
    unported_fields=UNPORTED_FIELDS,
)


def live_runner_argv_for(spec_path: str, red_control: str | None = None) -> list[str]:
    """Return the target-selected runner vector for EdenFS execution."""
    return runner_argv_for(spec_path, EDENFS_TARGET.name, red_control)


def live_runner_main(spec_path: str, red_control: str | None) -> int:
    """Run EDENFS_TARGET's vocabulary through the common runner.

    The runner executes and records; it accepts the red controls the
    checker target maps. Execution is identical for RECORDING_TARGET,
    which shares every action, site, and backend.
    """
    return runner_main(spec_path, EDENFS_TARGET, red_control)


def run_live(
    spec: RunSpec,
    artifact_dir: str,
    *,
    red_control: str | None = None,
) -> ExplorationResult:
    """Run one spec against a fresh live Environment owned by a runner process.

    The runner builds the Environment from the spec, runs the scheduler
    with the selected backend, and streams history JSONL on stdout. The
    supervisor SIGKILLs the runner at its deadline and still leaves a
    replayable history prefix plus full manifests. Cleanup failures (fault
    unwind, daemon close) surface via the runner's exit code and stderr;
    none pass. The run is judged by EDENFS_TARGET.
    """
    return run_exploration(
        spec,
        artifact_dir,
        target=EDENFS_TARGET,
        red_control=red_control,
    )
