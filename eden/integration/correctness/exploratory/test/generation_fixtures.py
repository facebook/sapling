# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared builders for the generator tests.

`generate_specs` is the `explore generate` shape (32 files, an unplanned
stratum); `classify_checkouts` predicts each first checkout's outcome
against the pinned fixture; `compose_sampled` composes one seed over a sampled
environment of either target.
"""

from __future__ import annotations

import functools
import random
from collections import Counter
from collections.abc import Iterator, Mapping
from typing import Any

from eden.integration.correctness.exploratory.core.generate import (
    compose,
    derived_rng,
    FRESH_PRODUCERS,
    generation_summary,
    must_end_before,
    PRODUCER_STRATUM,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEFAULT_BUDGETS,
    Environment,
    RunSpec,
)
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs.fixture import GeneratedFixture
from eden.integration.correctness.exploratory.edenfs.target import (
    RECORDING_TARGET,
    sample_environment,
    sample_environment as sample_edenfs_environment,
)
from eden.integration.correctness.exploratory.fake.target import (
    sample_environment as sample_fake_environment,
)


REGISTERS = ("tree/registers/r0.reg", "tree/registers/r1.reg")


def sampled_environment(seed: int, files: int = 32) -> dict[str, Any]:
    environment = dict(sample_environment(derived_rng(seed, "exploratory-environment")))
    environment["repository"] = {**environment["repository"], "file_count": files}
    return environment


def generate_specs(
    seeds: range, target: Target = RECORDING_TARGET, **kwargs: Any
) -> Iterator[RunSpec]:
    """The `explore generate` shape: 32 files, an unplanned stratum."""
    for seed in seeds:
        yield compose(target, seed, sampled_environment(seed), **kwargs)


def classify_checkouts(spec: RunSpec) -> list[tuple[Action, str]]:
    """Statically predict each first checkout's outcome against the pinned fixture.

    Only checkouts that no other checkout precedes or may overlap are
    classified: `trivial` (the tip again), `force`, `clean` (nothing dirty
    at the start that the target revision changes), `conflict` (setup dirt
    or a finished write/chmod on a path the revision changes), or
    `maybe-conflict` (only an overlapping write/chmod could).
    """
    fixture = GeneratedFixture.for_spec(spec)
    tip = fixture.initial_revision
    before = must_end_before(spec)
    checkouts = [action for action in spec.actions if action.kind == "checkout"]
    layout = fixture.layout
    dirt = [
        layout.file_at(index, tip).path
        for index in range(min(fixture.scatter_dirty, layout.file_count))
    ]
    result = []
    for checkout in checkouts:
        if any(
            other.id != checkout.id and checkout.id not in before[other.id]
            for other in checkouts
        ):
            continue
        revision = checkout.payload["revision"]
        changed = functools.partial(_changed, fixture, tip, revision)
        if revision == tip:
            outcome = "trivial"
        elif checkout.payload["mode"] == "force":
            outcome = "force"
        else:
            certain = any(changed(path) for path in dirt)
            maybe = False
            for action in spec.actions:
                if action.kind not in ("write", "chmod"):
                    continue
                if action.kind == "write" and not changed(action.payload["path"]):
                    continue
                if action.id in before[checkout.id]:
                    certain = True
                elif checkout.id not in before[action.id]:
                    maybe = True
            outcome = "conflict" if certain else "maybe-conflict" if maybe else "clean"
        result.append((checkout, outcome))
    return result


def _changed(fixture: GeneratedFixture, tip: int, revision: int, path: str) -> bool:
    return fixture.committed(path, tip) != fixture.committed(path, revision)


def fault_window(spec: RunSpec, before: Mapping[str, frozenset[str]]) -> set[str]:
    """Return the non-fault actions neither ordered before the install nor after the removal."""
    install = next((a for a in spec.actions if a.kind == "fault_install"), None)
    if install is None:
        return set()
    removes = [a.id for a in spec.actions if a.kind == "fault_remove"]
    return {
        action.id
        for action in spec.actions
        if action.id not in before[install.id]
        and not any(remove in before[action.id] for remove in removes)
        and not action.kind.startswith("fault_")
    }


def repeated_coverage(specs: Iterator[RunSpec]) -> Counter[str]:
    """Count repeated producers outside the tails and revisits inside fault windows.

    `fresh-repeated` sums `producers:repeated` over specs with no tail;
    `window-revisits` counts checkouts to the tip or to another checkout's
    revision that start after the install and need not wait for the fault
    removal; `tail-window-revisits` counts the tails' share of those.
    """
    counts: Counter[str] = Counter()
    for spec in specs:
        counts["specs"] += 1
        if spec.metadata[PRODUCER_STRATUM] == FRESH_PRODUCERS:
            summary = generation_summary(spec, RECORDING_TARGET)
            counts["fresh-repeated"] += summary["producers:repeated"]
        window = fault_window(spec, must_end_before(spec))
        tip = spec.environment.target["repository"]["commit_count"] - 1
        checkouts = [a for a in spec.actions if a.kind == "checkout"]
        for checkout in checkouts:
            produced = {tip} | {
                other.payload["revision"]
                for other in checkouts
                if other.id != checkout.id
            }
            if checkout.id in window and checkout.payload["revision"] in produced:
                counts["window-revisits"] += 1
                counts["tail-window-revisits"] += checkout.id.startswith("v")
    return counts


def actor_spec(actors: Mapping[str, list[Action]]) -> RunSpec:
    environment = sampled_environment(0)
    environment["setup_profile"] = {
        **environment["setup_profile"],
        "scatter_dirty": 0,
    }
    return RunSpec(
        seed=7,
        target="edenfs",
        environment=Environment(environment),
        budgets=DEFAULT_BUDGETS,
        actors=tuple(
            Actor(actor, tuple(action.id for action in actions))
            for actor, actions in actors.items()
        ),
        actions=tuple(
            sorted(
                (action for actions in actors.values() for action in actions),
                key=lambda action: action.id,
            )
        ),
    )


def checkout_action(action_id: str, revision: int, mode: str = "force") -> Action:
    return Action(
        action_id,
        "checkout",
        "operation",
        payload={
            "revision": revision,
            "via": "thrift" if mode == "force" else "goto",
            "mode": mode,
        },
    )


def compose_sampled(
    target: Target,
    seed: int,
    *,
    stratum: tuple[str, str, str] | None = None,
    forced: bool = False,
    producers: str | None = None,
) -> RunSpec:
    """Compose one seed over a sampled environment of `target` (EdenFS or the fake)."""
    environment = sample_fake_environment(random.Random(0))
    if target.name == "edenfs":
        environment = sample_edenfs_environment(random.Random(0))
    return compose(
        target,
        seed,
        environment,
        stratum=stratum,
        forced=forced,
        producers=producers,
    )


def action_owners(spec: RunSpec) -> dict[str, str]:
    """Return each action's actor."""
    return {action_id: actor.id for actor in spec.actors for action_id in actor.actions}


def edenfs_checkout_spec() -> RunSpec:
    """Return a one-checkout EdenFS RunSpec over a 16-file repository."""
    checkout = Action(
        "checkout",
        "checkout",
        "operation",
        payload={"revision": 1, "via": "goto", "mode": "normal"},
    )
    return RunSpec(
        seed=7,
        target="edenfs",
        environment=Environment(
            target={
                "repository": {
                    "file_count": 16,
                    "commit_count": 4,
                    "registers": ["tree/registers/r0.reg"],
                },
                "channel": "fuse",
                "case_sensitive": True,
                "fixture_source": "backing",
                "setup_profile": {"materialize": False, "scatter_dirty": 0},
            }
        ),
        budgets=Budgets(3, 1, 3),
        actors=(Actor("a", (checkout.id,)),),
        actions=(checkout,),
    )
