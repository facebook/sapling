# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Spec builders, stubs, process probes, and live-run helpers shared by the fault tests."""

import os
import threading
import time
import unittest
from collections.abc import Mapping
from dataclasses import replace
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory.core.checker import ACCEPT, Verdict
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    DEADLINE_RUN,
    Environment as SpecEnvironment,
    FINISHED,
    from_json as _from_json,
    RunSpec,
    STARTED,
    validate as _validate,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_committed
from eden.integration.correctness.exploratory.edenfs.fixture import CHURN_FILES
from eden.integration.correctness.exploratory.edenfs.sites import (
    CHECKOUT_BEGIN_SITE,
    LOOKUP_SITE,
    PENDING_WRITES_SITE,
    STATUS_CACHE_FILL_SITE,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    REGISTERS,
)

_SEED = 6207

# The dirt that peer writes leave across a parked checkout. Register bytes
# are revision-specific, so a dirty register conflicts with every normal
# checkout to another revision; a churn file is byte-identical at every
# revision, so the checkout keeps its dirt and heals.
DIRT = CHURN_FILES[0]


def _checkpoint_spec(site: str, behavior: str) -> RunSpec:
    """Exercise a single internal checkpoint after a sibling baseline read."""
    block = behavior == "block"
    unload_first = site == "inodemap-load"
    actions = [
        Action(
            "b0",
            "read",
            DEADLINE_OPERATION,
            payload={
                "path": REGISTERS[1],
                "channel": "fuse",
            },
        ),
        Action(
            "f1",
            "fault_install",
            DEADLINE_OPERATION,
            requires=(
                ("b0", FINISHED),
                *((("u0", FINISHED),) if unload_first else ()),
            ),
            payload={
                "fault_id": "F",
                "site": site,
                "behavior": behavior,
                "target_action": "t1",
            },
        ),
        Action(
            "t1",
            "getattr" if site == "inodemap-load" else "write",
            DEADLINE_RUN if block else DEADLINE_OPERATION,
            requires=(("f1", FINISHED),),
            payload={
                "path": REGISTERS[0],
                **({} if site == "inodemap-load" else {"value_id": "written"}),
            },
        ),
        _await("h1", "F", ("t1", STARTED if block else FINISHED)),
        Action(
            "x1",
            "fault_remove",
            DEADLINE_OPERATION,
            requires=(("h1", FINISHED),),
            payload={"fault_id": "F"},
        ),
    ]
    actors: list[tuple[str, list[str]]] = [
        ("baseline", ["b0"]),
        ("fault", ["f1", "h1", "x1"]),
        ("target", ["t1"]),
    ]
    if unload_first:
        actions.insert(
            1,
            Action(
                "u0",
                "unload_inodes",
                DEADLINE_OPERATION,
                requires=(("b0", FINISHED),),
                payload={"path": str(Path(REGISTERS[0]).parent)},
            ),
        )
        actors[2][1].insert(0, "u0")
    return replace(
        _spec(
            REGISTERS[0],
            actions,
            actors,
        ),
        environment=_spec_env(),
    )


def _spec_env() -> SpecEnvironment:
    """Return the standard live-test environment section."""
    return SpecEnvironment(
        target={
            "repository": {
                "file_count": 32,
                "commit_count": 4,
                "registers": list(REGISTERS),
            },
            "channel": "fuse",
            "case_sensitive": True,
            "fixture_source": "backing",
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }
    )


def from_json(text: str) -> RunSpec:
    return _from_json(text, EDENFS_TARGET)


def validate(spec: RunSpec) -> None:
    _validate(spec, EDENFS_TARGET)


def _spec(
    register: str, actions: list[Action], actors: list[tuple[str, list[str]]]
) -> RunSpec:
    """Return a single-register spec owning every action exactly once."""
    return RunSpec(
        seed=_SEED,
        target=EDENFS_TARGET.name,
        environment=SpecEnvironment(
            target={
                **_spec_env().target,
                "repository": {
                    "file_count": 32,
                    "commit_count": 4,
                    "registers": [register],
                },
            }
        ),
        # Hang guards: these tests check outcomes, not how long a loaded host
        # takes to reach them.
        budgets=Budgets(120.0, 60.0, 600.0),
        actors=tuple(Actor(id=name, actions=tuple(ids)) for name, ids in actors),
        actions=tuple(actions),
    )


def _install(target: str) -> Action:
    """Return a checkout-begin install of fault F for a target checkout."""
    return Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        payload={
            "fault_id": "F",
            "target_action": target,
            "site": "checkout-begin",
            "behavior": "block",
        },
    )


def _install_cache(target: str, *requires: tuple[str, str]) -> Action:
    """Return a status-cache-fill install of fault F for a target probe."""
    return Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        requires=tuple(requires),
        payload={
            "fault_id": "F",
            "target_action": target,
            "site": "status-cache-fill",
            "behavior": "block",
        },
    )


def _probe(action_id: str, *requires: tuple[str, str]) -> Action:
    """Return an operation-deadline sl status probe."""
    return Action(
        id=action_id,
        kind="status",
        deadline=DEADLINE_OPERATION,
        requires=tuple(requires),
        payload={"channel": "sl"},
    )


def _parked_probe() -> Action:
    """Return the run-deadline s1 target a cache-fill install parks."""
    return Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_RUN,
        requires=(("f1", FINISHED),),
        payload={"channel": "sl"},
    )


def _tree_checkout_ordinal_spec(ordinal: int) -> RunSpec:
    """Park the ordinal-th directory below tree/a1 during a Thrift checkout."""
    materialize = Action(
        id="m1",
        kind="manifest_presence",
        deadline=DEADLINE_OPERATION,
        payload={"from_revision": 0, "to_revision": 3},
    )
    install = Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        requires=((materialize.id, FINISHED),),
        payload={
            "fault_id": "F",
            "target_action": "c1",
            "site": "tree-checkout",
            "behavior": "block",
            "key": "tree/a1",
            "ordinal": ordinal,
        },
    )
    checkout = Action(
        id="c1",
        kind="checkout",
        deadline=DEADLINE_RUN,
        requires=((install.id, FINISHED),),
        payload={"revision": 0, "via": "thrift", "mode": "normal"},
    )
    witness = _await("h1", "F", (checkout.id, STARTED))
    remove = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=((witness.id, FINISHED),),
        payload={"fault_id": "F"},
    )
    read = Action(
        id="r1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=((checkout.id, FINISHED), (remove.id, FINISHED)),
        payload={"path": REGISTERS[0], "channel": "fuse"},
    )
    return _spec(
        REGISTERS[0],
        [materialize, install, checkout, witness, remove, read],
        [
            ("installer", [materialize.id, install.id]),
            ("checkout", [checkout.id, read.id]),
            ("fault", [witness.id, remove.id]),
        ],
    )


def _install_wfpw(target: str, *requires: tuple[str, str]) -> Action:
    """Return a wait-for-pending-writes install of fault F for a target read."""
    return Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        requires=tuple(requires),
        payload={
            "fault_id": "F",
            "target_action": target,
            "site": "wait-for-pending-writes",
            "behavior": "block",
        },
    )


def _sync_read(
    action_id: str, register: str, sync: int, *requires: tuple[str, str]
) -> Action:
    """Return a thrift-channel read with one sync timeout and requires edges."""
    return Action(
        id=action_id,
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=tuple(requires),
        payload={
            "path": register,
            "channel": "thrift",
            "sync_timeout_seconds": sync,
        },
    )


def _parked_sync_read(register: str) -> Action:
    """Return the run-deadline t1 target a pending-writes install parks."""
    return Action(
        id="t1",
        kind="read",
        deadline=DEADLINE_RUN,
        requires=(("f1", FINISHED),),
        payload={
            "path": register,
            "channel": "thrift",
            "sync_timeout_seconds": 60,
        },
    )


def _wfpw_spec(register: str) -> RunSpec:
    """Dirty DIRT, park a syncing register read at pending-writes, checkout, probe status.

    The checkout waits on the witness, so the parked read deterministically
    consumes the count-1 block before the checkout starts: no race between
    the two waitForPendingWrites callers.
    """
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": DIRT, "value_id": "fault-w1"},
    )
    h1 = _await("h1", "F", ("t1", STARTED))
    # The checkout heals across the park but never parks itself, so it
    # keeps an operation deadline: only the t1 target may run past it.
    c0 = Action(
        id="c0",
        kind="checkout",
        deadline=DEADLINE_OPERATION,
        requires=(("f1", FINISHED), ("h1", FINISHED)),
        payload={"revision": 2, "via": "goto", "mode": "normal"},
    )
    x1 = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=(("h1", FINISHED), ("c0", FINISHED)),
        payload={"fault_id": "F"},
    )
    actions = [
        _install_wfpw("t1"),
        w1,
        _parked_sync_read(register),
        h1,
        c0,
        x1,
        _probe("s2", ("x1", FINISHED)),
    ]
    actors = [
        ("nemesis", ["f1", "h1", "x1"]),
        ("a", ["c0"]),
        ("b", ["w1", "t1", "s2"]),
    ]
    return _spec(register, actions, actors)


def _status_guard_spec(register: str) -> RunSpec:
    """Prime, dirty DIRT, park the status fill across checkout, release, probe."""
    s0 = _probe("s0")
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": DIRT, "value_id": "fault-w1"},
    )
    h1 = _await("h1", "F", ("s1", STARTED))
    # The checkout heals across the park but never parks itself, so it
    # keeps an operation deadline: only the s1 target may run past it.
    c0 = Action(
        id="c0",
        kind="checkout",
        deadline=DEADLINE_OPERATION,
        requires=(("f1", FINISHED),),
        payload={"revision": 2, "via": "goto", "mode": "normal"},
    )
    x1 = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=(("h1", FINISHED), ("c0", FINISHED)),
        payload={"fault_id": "F"},
    )
    actions = [
        s0,
        _install_cache("s1", ("s0", FINISHED)),
        w1,
        _parked_probe(),
        h1,
        c0,
        x1,
        _probe("s2", ("x1", FINISHED)),
    ]
    actors = [
        ("nemesis", ["f1", "h1", "x1"]),
        ("a", ["c0"]),
        ("b", ["s0", "w1", "s1", "s2"]),
    ]
    return _spec(register, actions, actors)


def _checkout(action_id: str, revision: int, *requires: tuple[str, str]) -> Action:
    """Return a run-deadline checkout of one revision with requires edges."""
    return Action(
        id=action_id,
        kind="checkout",
        deadline=DEADLINE_RUN,
        requires=tuple(requires),
        payload={"revision": revision, "via": "goto", "mode": "normal"},
    )


def _await(action_id: str, fault_id: str, *requires: tuple[str, str]) -> Action:
    return Action(
        id=action_id,
        kind="fault_await",
        deadline=DEADLINE_OPERATION,
        requires=tuple(requires),
        payload={"fault_id": fault_id},
    )


def _green_spec(register: str) -> RunSpec:
    """Checkout parks at its block while peer traffic overlaps, then heals.

    The peer dirties DIRT (churn files are write-only to the model), then
    reads the register over both channels while the checkout is parked.
    """
    w1 = Action(
        id="w1",
        kind="write",
        deadline=DEADLINE_OPERATION,
        payload={"path": DIRT, "value_id": "fault-w1"},
        requires=(("h1", FINISHED),),
    )
    r1 = Action(
        id="r1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=(("w1", FINISHED),),
        payload={"path": register, "channel": "fuse"},
    )
    t1 = Action(
        id="t1",
        kind="read",
        deadline=DEADLINE_OPERATION,
        requires=(("w1", FINISHED),),
        payload={
            "path": register,
            "channel": "thrift",
            "sync_timeout_seconds": 60,
        },
    )
    x1 = Action(
        id="x1",
        kind="fault_remove",
        deadline=DEADLINE_OPERATION,
        requires=(("t1", FINISHED),),
        payload={"fault_id": "F"},
    )
    h1 = _await("h1", "F", ("c0", STARTED))
    actions = [_install("c0"), _checkout("c0", 2, ("f1", FINISHED)), h1, w1, r1]
    actions.extend([t1, x1])
    actors = [
        ("nemesis", ["f1", "h1", "x1"]),
        ("a", ["c0"]),
        ("b", ["w1", "r1", "t1"]),
    ]
    return _spec(register, actions, actors)


def _behavior_spec(register: str, site: str, behavior: str) -> RunSpec:
    """Return one pre-armed public-master fault and its target operation."""
    if site == CHECKOUT_BEGIN_SITE:
        target = Action(
            id="t1",
            kind="checkout",
            deadline=DEADLINE_OPERATION,
            requires=(("f1", FINISHED),),
            payload={"revision": 2, "via": "goto", "mode": "normal"},
        )
    elif site == STATUS_CACHE_FILL_SITE:
        target = _probe("t1", ("f1", FINISHED))
    elif site == PENDING_WRITES_SITE:
        target = _sync_read("t1", register, 60, ("f1", FINISHED))
    elif site == LOOKUP_SITE:
        target = Action(
            id="t1",
            kind="read",
            deadline=DEADLINE_OPERATION,
            requires=(("f1", FINISHED),),
            payload={"path": register, "channel": "fuse"},
        )
    else:
        raise AssertionError(f"unknown public-master site {site!r}")
    payload: dict[str, Any] = {
        "fault_id": "F",
        "target_action": "t1",
        "site": site,
        "behavior": behavior,
    }
    if behavior == "delay":
        payload["delay_ms"] = 250
    install = Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        payload=payload,
    )
    actions = [install, target]
    nemesis = ["f1"]
    actions.append(
        Action(
            id="x1",
            kind="fault_remove",
            deadline=DEADLINE_OPERATION,
            requires=(("t1", FINISHED),),
            payload={"fault_id": "F"},
        )
    )
    nemesis.append("x1")
    return _spec(register, actions, [("nemesis", nemesis), ("target", ["t1"])])


def _committed(spec: RunSpec, path: str, revision: int) -> str:
    """Return the hex committed bytes of one register at one revision."""
    return encode_committed(spec.seed, path, revision).hex()


def _process_argv(proc_root: str, pid: int) -> list[bytes] | None:
    """Read one process argument vector, failing closed if it vanished."""
    try:
        with open(os.path.join(proc_root, str(pid), "cmdline"), "rb") as handle:
            return [token for token in handle.read().split(b"\0") if token]
    except OSError:
        return None


def _process_parent(proc_root: str, pid: int) -> int | None:
    """Read one process parent from proc status, failing closed on bad data."""
    try:
        with open(os.path.join(proc_root, str(pid), "status")) as handle:
            for line in handle:
                label, separator, value = line.partition(":")
                if label == "PPid" and separator:
                    return int(value.strip())
    except (OSError, ValueError):
        return None
    return None


def _has_spec_ancestor(proc_root: str, pid: int, spec_path: str) -> bool:
    """Return whether an intact ancestor has the exact run-spec arguments."""
    expected = os.fsencode(spec_path)
    seen: set[int] = set()
    while pid > 1 and pid not in seen:
        seen.add(pid)
        argv = _process_argv(proc_root, pid)
        if argv is None:
            return False
        if any(
            option == b"--spec" and value == expected
            for option, value in zip(argv, argv[1:])
        ):
            return True
        parent = _process_parent(proc_root, pid)
        if parent is None:
            return False
        pid = parent
    return False


def _owned_sl_children(
    node: str, spec_path: str, *, proc_root: str = "/proc"
) -> list[int]:
    """Return exact-node sl updates descended from one contained run."""
    node_arg = node.encode()
    try:
        pids = [int(entry) for entry in os.listdir(proc_root) if entry.isdigit()]
    except OSError as error:
        raise AssertionError(f"{proc_root} unavailable: {error}") from error
    matches: list[int] = []
    for pid in pids:
        argv = _process_argv(proc_root, pid)
        if argv is None or b"goto" not in argv or node_arg not in argv:
            continue
        if _has_spec_ancestor(proc_root, pid, spec_path):
            matches.append(pid)
    return sorted(matches)


# Longer than a fixture run's runner deadline: the contained run ends first,
# and each wait then fails at once, so the guard only stops a hung test.
_RUN_HANG_GUARD = 900.0


def _wait_sl_child(
    node: str,
    spec_path: str,
    worker: threading.Thread,
    timeout_seconds: float = _RUN_HANG_GUARD,
) -> list[int]:
    """Block until this contained run owns an sl goto for the exact node."""
    deadline = time.monotonic() + timeout_seconds
    rounds = 0
    while time.monotonic() < deadline:
        rounds += 1
        if not worker.is_alive():
            raise AssertionError("run finished before any sl goto child appeared")
        matches = _owned_sl_children(node, spec_path)
        if matches:
            return matches
        time.sleep(0.05)
    raise AssertionError(
        f"sl goto child never appeared in {rounds} /proc scans; "
        f"run alive: {worker.is_alive()}"
    )


def _wait_marker(
    marker: str, worker: threading.Thread, timeout_seconds: float = _RUN_HANG_GUARD
) -> None:
    """Wait for a runner marker, failing if the contained run exits first."""
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        if os.path.isfile(marker):
            return
        if not worker.is_alive():
            raise AssertionError(f"run finished before marker {marker!r}")
        time.sleep(0.05)
    raise AssertionError(f"marker never appeared: {marker!r}")


def _wait_live_ready(
    artifact_dir: str,
    worker: threading.Thread,
    timeout_seconds: float = _RUN_HANG_GUARD,
) -> str:
    """Poll stderr.log for the live runner's ready line; return the rev-2 node.

    Fail when the runner died during setup, when the run finished without
    announcing readiness, or when setup never finished: the environment
    check has already passed.
    """
    path = os.path.join(artifact_dir, "stderr.log")
    deadline = time.monotonic() + timeout_seconds
    content = ""
    while time.monotonic() < deadline:
        try:
            with open(path) as handle:
                content = handle.read()
        except OSError:
            content = ""
        for line in content.splitlines():
            if line.startswith("live-ready commits="):
                return line.split("=", 1)[1].split()[2]
        if "setup-failed" in content or not worker.is_alive():
            break
        time.sleep(0.05)
    if "setup-failed" in content:
        raise AssertionError(f"live setup failed: {content.strip()}")
    if not worker.is_alive():
        raise AssertionError("run finished before the live runner was ready")
    raise AssertionError("live setup never finished")


def _status_code(result: Mapping[str, Any], path: str) -> str | None:
    """Return the code one status result's entry list reports for a path."""
    codes = [
        entry["code"] for entry in result.get("status", ()) if entry["path"] == path
    ]
    if len(codes) > 1:
        raise AssertionError(f"status lists {path!r} {len(codes)} times")
    return codes[0] if codes else None


def _unsatisfied(verdict: Verdict) -> list[str]:
    """Return every violated or not-evaluated obligation of a verdict, one line each."""
    lines: list[str] = []
    report = verdict.report
    if report is None:
        return [f"no report: {verdict.kind} {verdict.reason}"]
    lines.extend(f"gate: {problem}" for problem in report.gate)
    for result in report.results:
        if result.error is not None:
            lines.append(f"{result.property_id}: error {result.error}")
        for obligation in result.obligations:
            if obligation.status == ObligationStatus.SATISFIED:
                continue
            lines.append(
                f"{obligation.status} {obligation.rule_id} [{obligation.scope}] "
                f"{obligation.subject.token} "
                f"{obligation.attribution or obligation.not_evaluated_reason}: "
                f"{obligation.reason}"
            )
    return lines


def _assert_accept(test: unittest.TestCase, verdict: Verdict) -> None:
    """Assert an accept verdict, naming every unsatisfied obligation otherwise."""
    test.assertEqual(
        verdict.kind,
        ACCEPT,
        f"{verdict.kind} {verdict.reason}\n" + "\n".join(_unsatisfied(verdict)),
    )


def _violated_rules(verdict: Verdict) -> set[str]:
    """Return the rule IDs of every violated obligation of a verdict."""
    report = verdict.report
    if report is None:
        return set()
    return {
        obligation.rule_id
        for result in report.results
        for obligation in result.obligations
        if obligation.status == ObligationStatus.VIOLATED
    }
