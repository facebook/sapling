# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Running one spec live under containment, for the EdenFS live tests.

The runner is a `python -c` program that calls `live_runner_main` directly, so a live test
needs no CLI binary; its argv[0] is this file, from which EdenFS's FindExe locates
the built binaries.
"""

from __future__ import annotations

import os
import sys

from eden.integration.correctness.exploratory.core.containment import (
    ExplorationResult,
    run_exploration,
    RUN_SPEC_NAME,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET

RUNNER = (
    "import sys\n"
    "spec_path, red_control, sys.argv[0] = sys.argv[1:4]\n"
    "from eden.integration.correctness.exploratory.edenfs.target import live_runner_main\n"
    "sys.exit(live_runner_main(spec_path, red_control or None))\n"
)


def run_contained(
    spec: RunSpec,
    artifact_dir: str,
    *,
    target: Target = EDENFS_TARGET,
    red_control: str | None = None,
) -> ExplorationResult:
    """Run one spec under containment, judged by `target`."""
    return run_exploration(
        spec,
        artifact_dir,
        target=target,
        red_control=red_control,
        runner_argv=[
            sys.executable,
            "-c",
            RUNNER,
            os.path.join(artifact_dir, RUN_SPEC_NAME),
            red_control or "",
            os.path.abspath(__file__),
        ],
    )
