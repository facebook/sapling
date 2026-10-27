# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared fixtures for exploratory correctness tests."""

from __future__ import annotations

import json
import os
import subprocess
import sys
from typing import Any

from ..actions import CORE_ACTION_DEFS
from ..history import Event
from ..runspec import from_json, SpecError
from ..target import Target


def minimal_spec(
    actions: dict[str, dict[str, Any]],
    actors: list[dict[str, Any]],
    *,
    target_name: str,
    environment_target: dict[str, Any],
) -> str:
    """Return a minimal RunSpec JSON document containing the given workload."""
    return json.dumps(
        {
            "schema": "edenfs-exploratory-run",
            "schema_version": 1,
            "seed": 1,
            "target": target_name,
            "environment": {"target": environment_target},
            "budgets": {
                "operation_deadline_seconds": 120,
                "cleanup_deadline_seconds": 30,
                "runner_deadline_seconds": 600,
            },
            "actors": actors,
            "actions": actions,
        }
    )


def ev(type: str, action: str, t: float, **facts: Any) -> Event:
    """Return one action history event with the supplied facts."""
    return Event(type=type, time=t, data={"action": action, **facts})


def assert_rejects(spec_json: str, substring: str, target: Target) -> None:
    """Assert that parsing rejects a spec with the expected diagnostic."""
    try:
        from_json(spec_json, target)
    except SpecError as error:
        if substring not in str(error):
            raise AssertionError(
                f"expected {substring!r} in rejection {str(error)!r}"
            ) from error
    else:
        raise AssertionError("expected RunSpec to reject")


def history(*events: Event) -> list[Event]:
    """Return history events as a mutable list."""
    return list(events)


def _unused(*_args: Any) -> Any:
    raise AssertionError("parser tests do not run target code")


TOY_TARGET = Target(
    name="toy",
    actions=dict(CORE_ACTION_DEFS),
    sites={},
    properties=(),
    install_witnessed=_unused,
    environment_fields={},
    make_environment=_unused,
    make_backend=_unused,
    red_controls=frozenset(),
)


def run_in_fresh_interpreter(code: str, seed: str) -> str:
    """Run `code` in a new interpreter with PYTHONHASHSEED=`seed`; return stdout."""
    root = os.path.abspath(__file__)
    for _ in range(__name__.count(".") + 1):
        root = os.path.dirname(root)
    env = dict(os.environ, PYTHONHASHSEED=seed)
    env["PYTHONPATH"] = os.pathsep.join([root, *sys.path])
    lib = os.path.normpath(
        os.path.join(os.path.dirname(os.path.abspath(sys.executable)), "..", "lib")
    )
    if os.path.isdir(lib):
        env["LD_LIBRARY_PATH"] = os.pathsep.join(
            filter(None, [lib, env.get("LD_LIBRARY_PATH")])
        )
    return subprocess.run(
        [sys.executable, "-c", code],
        env=env,
        check=True,
        capture_output=True,
        text=True,
    ).stdout


# Real seconds before a test gives up on an event, a join, or a process; a hang
# guard, not a measurement.
HANG_GUARD = 60.0
