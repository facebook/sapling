# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The command line's target registry: the one seam a target is wired through.

Each entry names a target, its environment sampler, and how the CLI file
budget applies to a sampled environment. Every verb except `emit` takes `--target NAME`;
nothing defaults to a target.

`edenfs` is the property-bearing `EDENFS_TARGET`. A target that can run
paired differential RunSpecs also names a differential environment
sampler, which `campaign --differential` composes with; no other module
names an EdenFS target.
"""

from __future__ import annotations

import random
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from typing import Any

from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    sample_differential_environment as sample_edenfs_differential_environment,
    sample_environment as sample_edenfs_environment,
)
from eden.integration.correctness.exploratory.fake.target import (
    FAKE,
    sample_environment as sample_fake_environment,
)

Sampler = Callable[[random.Random], Mapping[str, Any]]
FileBudget = Callable[[Mapping[str, Any], int], dict[str, Any]]


@dataclass(frozen=True)
class TargetEntry:
    """One target the CLI can drive.

    `sample_differential_environment` samples an environment the target's
    differential validator admits (None: the target has no differential
    mode).
    """

    target: Target
    sample_environment: Sampler
    apply_files: FileBudget
    sample_differential_environment: Sampler | None = None


def _unchanged(environment: Mapping[str, Any], _files: int) -> dict[str, Any]:
    return dict(environment)


def edenfs_file_budget(environment: Mapping[str, Any], files: int) -> dict[str, Any]:
    """Apply the CLI file budget to a sampled EdenFS environment."""
    repository = dict(environment["repository"])
    repository["file_count"] = files
    profile = dict(environment["setup_profile"])
    profile["scatter_dirty"] = min(profile["scatter_dirty"], files)
    return {**environment, "repository": repository, "setup_profile": profile}


def _fake() -> TargetEntry:
    return TargetEntry(FAKE, sample_fake_environment, _unchanged)


def _edenfs() -> TargetEntry:
    return TargetEntry(
        EDENFS_TARGET,
        sample_edenfs_environment,
        edenfs_file_budget,
        sample_edenfs_differential_environment,
    )


def target_entries() -> dict[str, TargetEntry]:
    """Return every CLI target by name."""
    entries = (_edenfs(), _fake())
    return {entry.target.name: entry for entry in entries}
