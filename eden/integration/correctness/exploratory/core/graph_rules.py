# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Generic graph, ownership, and fault-lifetime rules for a RunSpec."""

from __future__ import annotations

from typing import Mapping

from .spec_types import (
    Action,
    DEADLINE_RUN,
    FINISHED,
    MAX_ACTIONS,
    RunSpec,
    SpecError,
    STARTED,
)
from .target import Target


def _validate_payload(spec: RunSpec, action: Action, target: Target) -> None:
    target.actions[action.kind].validate(spec, action)


def _expanded_edges(spec: RunSpec) -> dict[str, set[str]]:
    """Return successor edges: explicit requires plus implicit actor order."""
    edges: dict[str, set[str]] = {action.id: set() for action in spec.actions}
    for action in spec.actions:
        for dep_id, _event in action.requires:
            edges[dep_id].add(action.id)
    for actor in spec.actors:
        for previous, nxt in zip(actor.actions, actor.actions[1:]):
            edges[previous].add(nxt)
    return edges


def reachable(edges: dict[str, set[str]], source: str, goal: str) -> bool:
    """Return whether an expanded action graph orders source before goal."""
    seen = {source}
    stack = [source]
    while stack:
        node = stack.pop()
        for successor in edges[node]:
            if successor == goal:
                return True
            if successor not in seen:
                seen.add(successor)
                stack.append(successor)
    return False


def finish_before(spec: RunSpec, action_id: str) -> set[str]:
    """Return the actions that must finish before action_id starts."""
    predecessors: dict[str, set[str]] = {action.id: set() for action in spec.actions}
    for action in spec.actions:
        for dependency, event in action.requires:
            if event == FINISHED:
                predecessors[action.id].add(dependency)
    for actor in spec.actors:
        for previous, following in zip(actor.actions, actor.actions[1:]):
            predecessors[following].add(previous)
    seen: set[str] = set()
    stack = [action_id]
    while stack:
        node = stack.pop()
        for predecessor in predecessors[node]:
            if predecessor not in seen:
                seen.add(predecessor)
                stack.append(predecessor)
    return seen


def must_end_before(spec: RunSpec) -> dict[str, frozenset[str]]:
    """Return, per action, the actions that must finish before it starts.

    `a` ends before `b` starts when some path from `a` to `b` begins with a
    finished edge (a `finished` requirement or actor order) and continues
    over any edges: `a` finished before some `x` started, and `x` started
    no later than `b`. A `started` edge alone orders only starts.
    """
    finished: dict[str, set[str]] = {action.id: set() for action in spec.actions}
    started: dict[str, set[str]] = {action.id: set() for action in spec.actions}
    for action in spec.actions:
        for dependency, event in action.requires:
            started[action.id].add(dependency)
            if event == FINISHED:
                finished[action.id].add(dependency)
    for actor in spec.actors:
        for previous, following in zip(actor.actions, actor.actions[1:]):
            finished[following].add(previous)
            started[following].add(previous)
    result: dict[str, frozenset[str]] = {}
    for action in spec.actions:
        seen = {action.id}
        frontier = [action.id]
        while frontier:
            for dependency in started[frontier.pop()]:
                if dependency not in seen:
                    seen.add(dependency)
                    frontier.append(dependency)
        result[action.id] = frozenset().union(*(finished[node] for node in seen))
    return result


def may_overlap(
    spec: RunSpec, before: Mapping[str, frozenset[str]] | None = None
) -> tuple[tuple[str, str], ...]:
    """Return every action pair, sorted, neither of which must end before the other starts."""
    before = must_end_before(spec) if before is None else before
    ids = sorted(action.id for action in spec.actions)
    return tuple(
        (first, second)
        for index, first in enumerate(ids)
        for second in ids[index + 1 :]
        if first not in before[second] and second not in before[first]
    )


def _check_acyclic(spec: RunSpec) -> None:
    """Reject a dependency cycle using Kahn's topological sort, without recursion."""
    edges = _expanded_edges(spec)
    indegree = dict.fromkeys(edges, 0)
    for successors in edges.values():
        for successor in successors:
            indegree[successor] += 1
    ready = [node for node, count in indegree.items() if count == 0]
    sorted_count = 0
    while ready:
        node = ready.pop()
        sorted_count += 1
        for successor in edges[node]:
            indegree[successor] -= 1
            if indegree[successor] == 0:
                ready.append(successor)
    if sorted_count < len(edges):
        raise SpecError(f"dependency cycle: {' -> '.join(_cycle_path(spec, edges))}")


def _cycle_path(spec: RunSpec, edges: dict[str, set[str]]) -> list[str]:
    """Return the first cycle met by a depth-first walk from each action in spec order.

    Successors are visited in sorted order, so the path is deterministic. It
    runs only after Kahn's sort has found a cycle, and keeps an explicit
    stack instead of recursing.
    """
    finished: set[str] = set()
    for root in (action.id for action in spec.actions):
        if root in finished:
            continue
        path = [root]
        on_path = {root}
        pending = [iter(sorted(edges[root]))]
        while pending:
            successor = next(pending[-1], None)
            if successor is None:
                pending.pop()
                done = path.pop()
                on_path.remove(done)
                finished.add(done)
            elif successor in on_path:
                return [*path, successor]
            elif successor not in finished:
                path.append(successor)
                on_path.add(successor)
                pending.append(iter(sorted(edges[successor])))
    raise AssertionError("Kahn's sort found a cycle that the walk did not")


def _validate_sites(spec: RunSpec, target: Target, by_id: Mapping[str, Action]) -> None:
    """Run the target's site-local rule for every fault install."""
    for install in spec.actions:
        if install.kind != "fault_install":
            continue
        site_name = install.payload.get("site")
        site = target.sites.get(site_name) if isinstance(site_name, str) else None
        if site is None:
            raise SpecError(f"action {install.id!r} has unsupported site {site_name!r}")
        target_id = install.payload.get("target_action")
        if target_id not in by_id:
            raise SpecError(f"fault targets unknown action {target_id!r}")
        target_action = by_id[target_id]
        if target_action.kind not in site.target_kinds:
            raise SpecError(
                f"site {site_name!r} cannot target kind {target_action.kind!r}"
            )
        behavior = install.payload.get("behavior")
        if behavior not in site.behaviors:
            raise SpecError(
                f"site {site_name!r} does not support behavior {behavior!r}"
            )
        site.validate(spec, install, target_action)


def _fault_actions(
    spec: RunSpec,
) -> tuple[dict[str, str], dict[str, str], dict[str, str]]:
    """Collect one action ID per fault-lifecycle role and fault ID."""
    installs: dict[str, str] = {}
    awaits: dict[str, str] = {}
    removes: dict[str, str] = {}
    for action in spec.actions:
        fault_id = action.payload.get("fault_id")
        if action.kind == "fault_install":
            if fault_id is None:
                raise SpecError(f"install {action.id!r} needs fault_id")
            if fault_id in installs:
                raise SpecError(f"duplicate install for fault {fault_id!r}")
            installs[fault_id] = action.id
        elif action.kind == "fault_await":
            if fault_id is None:
                raise SpecError(f"wait {action.id!r} needs fault_id")
            if fault_id in awaits:
                raise SpecError(f"duplicate wait for fault {fault_id!r}")
            awaits[fault_id] = action.id
        elif action.kind == "fault_remove":
            if fault_id is None:
                raise SpecError(f"remove {action.id!r} needs fault_id")
            if fault_id in removes:
                raise SpecError(f"duplicate remove for fault {fault_id!r}")
            removes[fault_id] = action.id
    return installs, awaits, removes


def _validate_fault_pairs(
    by_id: Mapping[str, Action],
    installs: Mapping[str, str],
    awaits: Mapping[str, str],
    removes: Mapping[str, str],
) -> None:
    """Reject missing partners in an installed fault lifetime."""
    for fault_id, install_id in installs.items():
        behavior = by_id[install_id].payload.get("behavior")
        if fault_id not in removes:
            raise SpecError(f"fault {fault_id!r} has an install but no remove")
        if behavior == "block" and fault_id not in awaits:
            raise SpecError(f"block fault {fault_id!r} has an install but no wait")
    for fault_id in removes:
        if fault_id not in installs:
            raise SpecError(f"fault {fault_id!r} has a remove but no install")
    for fault_id in awaits:
        if fault_id not in installs:
            raise SpecError(f"fault {fault_id!r} has a wait but no install")


def _validate_fault_deadlines(
    spec: RunSpec,
    by_id: Mapping[str, Action],
    installs: Mapping[str, str],
) -> None:
    """Require run deadlines exactly on actions targeted by a block install."""
    blocked_targets = {
        by_id[install_id].payload.get("target_action")
        for install_id in installs.values()
        if by_id[install_id].payload.get("behavior") == "block"
        if by_id[install_id].payload.get("target_action") is not None
    }
    for target_id in blocked_targets:
        if target_id not in by_id:
            raise SpecError(f"block targets unknown action {target_id!r}")
    for action in spec.actions:
        if action.id in blocked_targets:
            if action.deadline != DEADLINE_RUN:
                raise SpecError(f"block target {action.id!r} must have deadline 'run'")
        elif action.deadline == DEADLINE_RUN:
            raise SpecError(
                f"action {action.id!r} has deadline 'run' but is not a block target"
            )


def _validate_fault_order(
    spec: RunSpec,
    by_id: Mapping[str, Action],
    installs: Mapping[str, str],
    awaits: Mapping[str, str],
    removes: Mapping[str, str],
) -> None:
    """Validate install timing, witness edges, and release ordering."""
    for fault_id, install_id in installs.items():
        install = by_id[install_id]
        target_id = _install_target(by_id, install)
        await_id = awaits.get(fault_id)
        if install.payload.get("behavior") == "block":
            assert await_id is not None
            _validate_block_order(spec, by_id, target_id, await_id, removes[fault_id])
            continue
        _validate_release_order(spec, target_id, await_id, removes.get(fault_id))


def _install_target(by_id: Mapping[str, Action], install: Action) -> str:
    """Return the install's target id after checking it exists and is pre-armed or mid-flight."""
    target_id = install.payload.get("target_action")
    if target_id is None:
        raise SpecError(f"install {install.id!r} needs target_action")
    if target_id not in by_id:
        raise SpecError(f"fault targets unknown action {target_id!r}")
    target_action = by_id[target_id]
    pre_armed = (install.id, FINISHED) in target_action.requires
    mid_flight = (target_id, STARTED) in install.requires
    if not pre_armed and not mid_flight:
        raise SpecError(
            f"fault install {install.id!r} must be pre-armed with "
            f"[{install.id!r}, 'finished'] in target {target_id!r} requires, "
            f"or mid-flight with [{target_id!r}, 'started'] in the install requires"
        )
    return target_id


def _validate_block_order(
    spec: RunSpec,
    by_id: Mapping[str, Action],
    target_id: str,
    await_id: str,
    remove_id: str,
) -> None:
    """A block fault's wait follows the target start and its remove follows the wait's finish."""
    if (target_id, STARTED) not in by_id[await_id].requires:
        raise SpecError(
            f"fault wait {await_id!r} must require [{target_id!r}, 'started']"
        )
    if await_id not in finish_before(spec, remove_id):
        raise SpecError(
            f"fault remove {remove_id!r} must start after wait {await_id!r} finished"
        )
    if target_id in finish_before(spec, remove_id):
        raise SpecError(
            f"fault remove {remove_id!r} must not wait on block target "
            f"{target_id!r} 'finished': an operation the fault blocks never "
            "finishes while the fault is installed"
        )


def _validate_release_order(
    spec: RunSpec, target_id: str, await_id: str | None, remove_id: str | None
) -> None:
    """A non-block fault's wait and remove both follow the target's completion."""
    if await_id is not None and target_id not in finish_before(spec, await_id):
        raise SpecError(
            f"non-block fault wait {await_id!r} must follow target "
            f"{target_id!r} completion"
        )
    if remove_id is not None and target_id not in finish_before(spec, remove_id):
        raise SpecError(
            f"non-block fault remove {remove_id!r} must follow target "
            f"{target_id!r} completion"
        )


def _validate_fault_rules(spec: RunSpec) -> None:
    """Validate the existing fault lifetime and deadline contract."""
    by_id = {action.id: action for action in spec.actions}
    installs, awaits, removes = _fault_actions(spec)
    _validate_fault_pairs(by_id, installs, awaits, removes)
    _validate_fault_deadlines(spec, by_id, installs)
    _validate_fault_order(spec, by_id, installs, awaits, removes)


def _validate_ownership(spec: RunSpec, by_id: Mapping[str, Action]) -> None:
    """Require every action to have exactly one actor owner."""
    owned: dict[str, str] = {}
    for actor in spec.actors:
        for action_id in actor.actions:
            if action_id not in by_id:
                raise SpecError(f"actor {actor.id!r} owns unknown action {action_id!r}")
            if action_id in owned:
                raise SpecError(
                    f"action {action_id!r} owned by both "
                    f"{owned[action_id]!r} and {actor.id!r}"
                )
            owned[action_id] = actor.id
    unowned = sorted(set(by_id) - set(owned))
    if unowned:
        raise SpecError(f"actions owned by no actor: {unowned}")


def _validate_dependencies(
    spec: RunSpec, target: Target, by_id: Mapping[str, Action]
) -> None:
    """Require known action kinds and dependency endpoints."""
    for action in spec.actions:
        if action.kind not in target.actions:
            raise SpecError(f"action {action.id!r} has unknown kind {action.kind!r}")
        for dependency, _event in action.requires:
            if dependency not in by_id:
                raise SpecError(
                    f"action {action.id!r} requires unknown action {dependency!r}"
                )
            if dependency == action.id:
                raise SpecError(f"action {action.id!r} requires itself")


def _validate_process_kills(
    spec: RunSpec, target: Target, by_id: Mapping[str, Action]
) -> None:
    """Bind each process kill to a started, process-spawning target action."""
    for action in spec.actions:
        if action.kind != "kill_process":
            continue
        target_id = action.payload["target_action"]
        target_action = by_id.get(target_id)
        if target_action is None:
            raise SpecError(f"kill_process targets unknown action {target_id!r}")
        if not target.actions[target_action.kind].spawns_process:
            raise SpecError(
                f"kill_process target {target_id!r} does not spawn a process"
            )
        if (target_id, STARTED) not in action.requires:
            raise SpecError(
                f"kill_process {action.id!r} must require [{target_id!r}, 'started']"
            )


def validate(spec: RunSpec, target: Target) -> None:
    """Enforce graph, fault-lifetime, deadline, and bound rules on a spec."""
    if not spec.actions:
        raise SpecError("actions must be non-empty")
    if len(spec.actions) > MAX_ACTIONS:
        raise SpecError(f"{len(spec.actions)} actions exceed the action limit")
    ids = [action.id for action in spec.actions]
    if len(set(ids)) != len(ids):
        raise SpecError("action ids must be unique")
    by_id = {action.id: action for action in spec.actions}
    _validate_ownership(spec, by_id)
    _validate_dependencies(spec, target, by_id)
    _check_acyclic(spec)
    for action in spec.actions:
        _validate_payload(spec, action, target)
    _validate_process_kills(spec, target, by_id)
    _validate_fault_rules(spec)
    _validate_sites(spec, target, by_id)
