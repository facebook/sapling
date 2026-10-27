# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target-neutral seeded generation helpers for exploratory runs.

Composition reads only target-owned samplers, sites, and the static
`ActionDef` declarations (roles, binding, grouping, footprint); it never
imports checker, property, or contracts code. Producer strata,
interaction families, and producer ambiguity are computed from those
declarations alone.
"""

from __future__ import annotations

import hashlib
import random
from collections import Counter
from collections.abc import Iterable, Mapping
from dataclasses import replace
from typing import Any

from .effects import _under, Footprint, footprints_interact
from .runspec import (
    Action,
    Actor,
    DEADLINE_OPERATION,
    DEADLINE_RUN,
    DEFAULT_BUDGETS,
    Environment,
    FINISHED,
    may_overlap,
    must_end_before,
    RunSpec,
    STARTED,
    validate,
)
from .sites import DAEMON_LABELS, EngagePlan, PUBLIC_MASTER, Site
from .target import ActionDef, Binding, Grouping, HeldInput, Role, Target

KILL_MODES = ("none", "process")
WIDTHS = (2, 3, 4, 8)
Stratum = tuple[str, str, str]
FRESH_PRODUCERS = "fresh"
REPEATED_PRODUCERS = "repeated"
FAULT_REPEATED_PRODUCERS = "repeated-in-fault"
PRODUCER_STRATA = (FRESH_PRODUCERS, REPEATED_PRODUCERS, FAULT_REPEATED_PRODUCERS)
# The RunSpec metadata key that records the composed producer stratum.
PRODUCER_STRATUM = "producer_stratum"
# Unplanned composition draws a repeated-producer tail this often; in a
# spec with a fault, this share of those tails runs inside the fault window.
REPEATED_PRODUCER_RATE = 0.125
FAULT_REPEATED_SHARE = 0.5


def admitted_strata(target: Target, daemon_label: str) -> tuple[Stratum, ...]:
    """Return every site, behavior, and kill-mode triple composition admits.

    A site is admitted when its `daemon` label is `PUBLIC_MASTER` or equals
    `daemon_label`, with each behavior it supports and an optional process kill.
    """
    if daemon_label not in DAEMON_LABELS:
        raise ValueError(f"unsupported daemon label {daemon_label!r}")
    return tuple(
        (site_name, behavior, kill_mode)
        for site_name, site in sorted(target.sites.items())
        if site.daemon in (PUBLIC_MASTER, daemon_label)
        for behavior in sorted(site.behaviors)
        for kill_mode in KILL_MODES
    )


def generation_target(target: Target) -> Target:
    """Restrict sites for composition and campaign strata, not handwritten specs.

    Campaign library callers pass this view when planning generated attempts.
    The fault sampler supplies target-owned eligibility through its context.
    """
    observable = target.generation.observable_target_kinds
    if observable is None:
        return target
    return replace(
        target,
        sites={
            name: replace(site, target_kinds=kinds)
            for name, site in target.sites.items()
            if (kinds := observable(name, site))
        },
    )


def held_inputs(target: Target) -> tuple[HeldInput, ...]:
    """Return the input families the composer may hold across a fault target."""
    return target.generation.held_inputs


def lease_kinds(target: Target) -> tuple[str, ...]:
    """Return the process-spawning nemesis kinds the composer places as leases."""
    return tuple(
        sorted(
            kind
            for kind, definition in target.actions.items()
            if definition.axis == "nemesis" and definition.spawns_process
        )
    )


def derived_rng(seed: int, domain: str) -> random.Random:
    """Return a domain-separated deterministic random stream."""
    digest = hashlib.sha256(f"{seed}\0{domain}".encode()).digest()
    return random.Random(int.from_bytes(digest))


def _context(
    environment: Mapping[str, Any], action_id: str, **values: Any
) -> dict[str, Any]:
    """Build the target sampler context for one action."""
    return {"environment": environment, "action_id": action_id, **values}


def _sample(
    definition: ActionDef,
    rng: random.Random,
    context: Mapping[str, Any],
    *,
    requires: tuple[tuple[str, str], ...] = (),
    deadline: str = DEADLINE_OPERATION,
) -> Action | None:
    """Turn one target-owned payload sample into an action."""
    payload = definition.sample(rng, context)
    if payload is None:
        return None
    return Action(context["action_id"], definition.kind, deadline, requires, payload)


def _action(
    action_id: str,
    kind: str,
    payload: Mapping[str, Any],
    requires: tuple[tuple[str, str], ...] = (),
) -> Action:
    """Build a core action with the ordinary operation deadline."""
    return Action(action_id, kind, DEADLINE_OPERATION, requires, payload)


def _sample_axis(
    target: Target,
    axis: str,
    rng: random.Random,
    context: Mapping[str, Any],
    *,
    requires: tuple[tuple[str, str], ...] = (),
    spawning: bool = False,
) -> Action:
    """Choose uniformly among sampleable actions on one axis.

    Samplers see ``spawning`` in their context so a kind that has both a
    process-spawning and an in-daemon form picks the spawning one when a
    process kill will target the action.
    """
    context = {**context, "spawning": spawning}
    choices = [
        action
        for definition in target.actions.values()
        if definition.axis == axis and (not spawning or definition.spawns_process)
        if (action := _sample(definition, rng, context, requires=requires)) is not None
    ]
    if not choices:
        raise ValueError(f"target {target.name!r} has no sampleable {axis} action")
    return rng.choice(choices)


def _sample_baseline_input(
    target: Target, rng: random.Random, environment: Mapping[str, Any]
) -> Action:
    """Sample a modeled input, preferring kinds that cannot warm a path."""
    context = _context(environment, "b0", forced_kind=True)
    for pathless in (True, False):
        choices = [
            action
            for definition in target.actions.values()
            if definition.axis == "input" and definition.modeled
            if ("path" not in definition.fields) == pathless
            if (action := _sample(definition, rng, context)) is not None
        ]
        if choices:
            return rng.choice(choices)
    raise ValueError(f"target {target.name!r} has no sampleable modeled input")


def _fault_lifecycle(
    target: Target,
    rng: random.Random,
    context: Mapping[str, Any],
    target_action: Action,
    mid_flight: bool,
    install: Action | None = None,
) -> tuple[Action, list[Action], str]:
    """Build one install and its behavior-specific partners."""
    if install is None:
        install = _sample(
            target.actions["fault_install"],
            rng,
            context,
            requires=((target_action.id, STARTED),) if mid_flight else (),
        )
        if install is None:
            raise ValueError(f"target {target.name!r} cannot sample fault_install")
    elif mid_flight:
        install = replace(install, requires=((target_action.id, STARTED),))
    behavior = context["behavior"]
    if behavior != "block":
        remove = _action(
            "n1", "fault_remove", {"fault_id": "F"}, ((target_action.id, FINISHED),)
        )
        return install, [install, remove], remove.id
    wait = _action(
        "n1", "fault_await", {"fault_id": "F"}, ((target_action.id, STARTED),)
    )
    remove = _action("n2", "fault_remove", {"fault_id": "F"}, ((wait.id, FINISHED),))
    return install, [install, wait, remove], remove.id


def _sample_inputs(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    input_actors: list[str],
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
    context: Mapping[str, Any],
    after: str | None,
) -> None:
    """Give every input actor one independently sampled input action."""
    base_requires = ((after, FINISHED),) if after is not None else ()
    requires = base_requires
    for index, actor_id in enumerate(input_actors, 1):
        action_id = f"i{index}"
        action = _sample_axis(
            target,
            "input",
            rng,
            _context(
                environment, action_id, **context, input_index=index, actions=actions
            ),
            requires=requires,
        )
        actions[action_id] = action
        actor_actions[actor_id].append(action_id)
        if context.get("serialize_sibling_inputs"):
            requires = (*base_requires, (action.id, FINISHED))


def _place_observation(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    placement: str,
    target_owner: str,
    fault_end: str,
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> None:
    """Place an observation before, inside, or after the fault lifetime."""
    owner = "a0" if placement == "during" else target_owner
    dependency = "n0" if placement == "during" else fault_end
    observation = _sample_axis(
        target,
        "observation",
        rng,
        _context(environment, "o0"),
        requires=() if placement == "pre" else ((dependency, FINISHED),),
    )
    observations = _observation_pair(target, rng, environment, observation, actions)
    actions.update((action.id, action) for action in observations)
    ids = [action.id for action in observations]
    if placement == "pre":
        actor_actions[owner][0:0] = ids
        install = actions["n0"]
        actions[install.id] = replace(
            install, requires=(*install.requires, *((id, FINISHED) for id in ids))
        )
    elif placement == "during":
        index = actor_actions[owner].index(fault_end)
        index += fault_end == dependency
        actor_actions[owner][index:index] = ids
    else:
        actor_actions[owner].extend(ids)


def _observation_pair(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    observation: Action,
    actions: Mapping[str, Action],
) -> list[Action]:
    """Replace one sampled observation by the target's published pair when it offers one."""
    sample_pair = target.generation.sample_observation_pair
    pair = (
        sample_pair(
            rng, _context(environment, "o0", install=actions["n0"], actions=actions)
        )
        if sample_pair
        else None
    )
    if not pair:
        return [observation]
    return [
        Action(f"o{index}", kind, DEADLINE_OPERATION, observation.requires, payload)
        for index, (kind, payload) in enumerate(pair)
    ]


def _add_kill_mode(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    kill_mode: str,
    owner: str,
    fault_end: str,
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> None:
    """Add an independent process kill after the fault lifetime."""
    if kill_mode == "none":
        return
    if kill_mode == "process":
        process = _sample_axis(
            target,
            "input",
            rng,
            _context(environment, "p0", forced_kind=True, actions=actions),
            requires=((fault_end, FINISHED),),
            spawning=True,
        )
        kill = _action(
            "k0",
            "kill_process",
            {"target_action": process.id, "after_ms": 0},
            ((process.id, STARTED),),
        )
        actions.update({process.id: process, kill.id: kill})
        actor_actions[owner].append(process.id)
        actor_actions["a0"].append(kill.id)
        return
    raise ValueError(f"unknown kill mode {kill_mode!r}")


def _prepare_forced_fault(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    site_name: str,
    behavior: str,
    kill_mode: str,
    contended: bool,
) -> tuple[Action, Mapping[str, Any], dict[str, Any]]:
    """Collect target-owned forced choices while sampling the install once."""
    choices: dict[str, Any] = {}

    def configure_forced_attempt(**values: Any) -> None:
        choices.update(values)

    payload = target.actions["fault_install"].sample(
        rng,
        _context(
            environment,
            "n0",
            site=site_name,
            site_definition=target.sites[site_name],
            behavior=behavior,
            target_action="i0",
            kill_mode=kill_mode,
            contended=contended,
            configure_forced_attempt=configure_forced_attempt,
        ),
    )
    if payload is None:
        raise ValueError(f"target {target.name!r} cannot sample fault_install")
    install = Action("n0", "fault_install", DEADLINE_OPERATION, payload=payload)
    return install, choices.get("environment", environment), choices


def _apply_forced_choices(
    context: dict[str, Any],
    environment: Mapping[str, Any],
    choices: Mapping[str, Any],
) -> None:
    """Validate generic forced choices and add them to sampler context."""
    placement = choices.get("observation_placement", "post")
    if placement not in {"pre", "during", "post"}:
        raise ValueError(f"invalid forced observation placement {placement!r}")
    serialize = choices.get("serialize_sibling_inputs", False)
    if type(serialize) is not bool:
        raise ValueError("forced sibling serialization choice must be bool")
    context.update(
        composition_environment=environment,
        observation_placement=placement,
        serialize_sibling_inputs=serialize,
    )


def _compose_fault(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    stratum: Stratum,
    contended: bool,
    forced: bool = False,
    *,
    seed: int | None = None,
) -> tuple[Action, list[Action], str, dict[str, Any]]:
    """Sample one site-compatible target and fault lifecycle."""
    site_name, behavior, kill_mode = stratum
    site = target.sites[site_name]
    if site_name in target.generation.uncontended_sites:
        contended = False
    plan, environment = _engage_site(site, rng, seed, environment)
    sampled_install, environment, choices = _forced_choices(
        target, rng, environment, stratum, contended, forced
    )
    target_action = _compose_target_action(
        target, rng, environment, stratum, contended, forced, choices, plan
    )
    context: dict[str, Any] = {
        "site": site_name,
        "kill_mode": kill_mode,
        "contended": contended,
    }
    mid_flight = _mid_flight_choice(rng, choices)
    install, lifecycle, fault_end = _fault_lifecycle(
        target,
        rng,
        _context(
            environment,
            "n0",
            behavior=behavior,
            target_action=target_action.id,
            configure_composition=choices.update,
            **context,
        ),
        target_action,
        mid_flight,
        sampled_install,
    )
    target_action = _bind_target(target_action, install, mid_flight, behavior)
    lifecycle, pre_install = _attach_pre_install(plan, install, lifecycle)
    _finish_fault_context(context, forced, choices, environment, plan, pre_install)
    return target_action, lifecycle, fault_end, context


def _forced_choices(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    stratum: Stratum,
    contended: bool,
    forced: bool,
) -> tuple[Action | None, Mapping[str, Any], dict[str, Any]]:
    """Sample the forced install and its target-owned choices; nothing when unforced."""
    if not forced:
        return None, environment, {}
    site_name, behavior, kill_mode = stratum
    return _prepare_forced_fault(
        target, rng, environment, site_name, behavior, kill_mode, contended
    )


def _bind_target(
    target_action: Action, install: Action, mid_flight: bool, behavior: str
) -> Action:
    """Pre-arm the target behind its install and give a blocked target the run deadline."""
    if not mid_flight:
        target_action = replace(target_action, requires=((install.id, FINISHED),))
    if behavior == "block":
        target_action = replace(target_action, deadline=DEADLINE_RUN)
    return target_action


def _engage_site(
    site: Site, rng: random.Random, seed: int | None, environment: Mapping[str, Any]
) -> tuple[EngagePlan | None, Mapping[str, Any]]:
    """Ask the site for its engage plan and apply its setup-profile overrides."""
    if site.engage is None:
        return None, environment
    plan = site.engage(rng, {"seed": seed, "environment": environment})
    if plan is None or not plan.setup_profile_overrides:
        return plan, environment
    return plan, _override_setup_profile(environment, plan.setup_profile_overrides)


def _override_setup_profile(
    environment: Mapping[str, Any], overrides: Mapping[str, Any]
) -> dict[str, Any]:
    """Return the environment with ``overrides`` merged into its setup profile."""
    return {
        **environment,
        "setup_profile": {**environment["setup_profile"], **overrides},
    }


def _compose_target_action(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    stratum: Stratum,
    contended: bool,
    forced: bool,
    choices: Mapping[str, Any],
    plan: EngagePlan | None,
) -> Action:
    """Sample the fault's target action, honouring forced kind and plan payload."""
    site_name, behavior, _kill_mode = stratum
    site = target.sites[site_name]
    target_kind = choices.get("target_kind")
    if target_kind is None:
        target_kind = rng.choice(sorted(site.target_kinds))
    if target_kind not in site.target_kinds:
        raise ValueError(
            f"forced target kind {target_kind!r} is incompatible with {site_name!r}"
        )
    target_context = _context(
        environment,
        "i0",
        site=site_name,
        behavior=behavior,
        contended=contended,
        input_index=0,
        forced_kind=True,
    )
    if forced:
        target_context["forced_attempt"] = True
    target_action = _sample(
        target.actions[target_kind],
        rng,
        target_context,
    )
    if target_action is None:
        raise ValueError(f"action {target_kind!r} is hand-written only")
    if plan is not None and plan.target_payload:
        payload = {**target_action.payload, **plan.target_payload}
        target_action = replace(
            target_action,
            payload={k: v for k, v in payload.items() if v is not None},
        )
    return target_action


def _mid_flight_choice(rng: random.Random, choices: Mapping[str, Any]) -> bool:
    """Return the forced install ordering, or draw one."""
    mid_flight = choices.get("mid_flight")
    if mid_flight is None:
        mid_flight = rng.choice((True, False))
    if type(mid_flight) is not bool:
        raise ValueError("forced mid_flight choice must be bool")
    return mid_flight


def _attach_pre_install(
    plan: EngagePlan | None, install: Action, lifecycle: list[Action]
) -> tuple[list[Action], tuple[Action, ...]]:
    """Order the plan's pre-install actions before the install and after the baseline."""
    pre_install = tuple(
        _action(f"e{index}", kind, dict(payload), requires=(("b0", FINISHED),))
        for index, (kind, payload) in enumerate(
            plan.pre_install_actions if plan is not None else ()
        )
    )
    if not pre_install:
        return lifecycle, pre_install
    install = replace(
        install,
        requires=(
            *install.requires,
            *((action.id, FINISHED) for action in pre_install),
        ),
    )
    lifecycle = [install if action.id == install.id else action for action in lifecycle]
    return lifecycle, pre_install


def _finish_fault_context(
    context: dict[str, Any],
    forced: bool,
    choices: dict[str, Any],
    environment: Mapping[str, Any],
    plan: EngagePlan | None,
    pre_install: tuple[Action, ...],
) -> None:
    """Record the composition environment and pre-install actions on the fault context."""
    if forced:
        _apply_forced_choices(context, environment, choices)
    elif "environment" in choices:
        context["composition_environment"] = choices["environment"]
    if plan is not None and plan.setup_profile_overrides:
        context.setdefault("composition_environment", environment)
    context["pre_install"] = pre_install


def _add_fault(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    stratum: Stratum,
    contended: bool,
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
    forced: bool = False,
    *,
    seed: int | None = None,
) -> tuple[list[str], str, dict[str, Any]]:
    """Add one cross-actor fault and return remaining input actors."""
    input_actors = list(actor_actions)[1:]
    target_action, lifecycle, fault_end, context = _compose_fault(
        target, rng, environment, stratum, contended, forced, seed=seed
    )
    actions[target_action.id] = target_action
    actor_actions[input_actors[0]].append(target_action.id)
    owner = actor_actions[input_actors[0]]
    for index, action in enumerate(context.pop("pre_install", ())):
        actions[action.id] = action
        owner.insert(index, action.id)
    for action in lifecycle:
        actions[action.id] = action
        actor_actions["a0"].append(action.id)
    return input_actors, fault_end, context


def _hold_input(
    target: Target,
    rng: random.Random,
    owner: list[str],
    actions: dict[str, Action],
) -> Mapping[str, Any] | None:
    """Pause a published stream before i0 and resume it after, at the published rate."""
    partner = actions["i0"]
    for held in held_inputs(target):
        if partner.kind != held.partner or rng.random() >= held.probability:
            continue
        stream = _action("d0", held.stream, dict(held.stream_payload))
        resume = _action("d1", held.resume, {held.resume_field: stream.id})
        actions.update({stream.id: stream, resume.id: resume})
        actions[partner.id] = replace(
            partner, requires=(*partner.requires, (stream.id, FINISHED))
        )
        index = owner.index(partner.id)
        owner[index : index + 1] = [stream.id, partner.id, resume.id]
        return held.setup_profile
    return None


def _decorate_fault(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    input_actors: list[str],
    fault_end: str,
    context: Mapping[str, Any],
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> Mapping[str, Any] | None:
    """Add observation, lease, kill mode, and a held input; return its setup override."""
    owner = input_actors[-1]
    _place_observation(
        target,
        rng,
        environment,
        context.get("observation_placement") or rng.choice(("pre", "during", "post")),
        owner,
        fault_end,
        actor_actions,
        actions,
    )
    _place_lease(target, rng, environment, fault_end, actor_actions, actions)
    _add_kill_mode(
        target,
        rng,
        environment,
        context["kill_mode"],
        owner,
        fault_end,
        actor_actions,
        actions,
    )
    return _hold_input(target, rng, actor_actions[input_actors[0]], actions)


def _place_lease(
    target: Target,
    rng: random.Random,
    environment: Mapping[str, Any],
    fault_end: str,
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> None:
    """In half of the removable-fault specs, run one lease across the fault window."""
    remove = actions[fault_end]
    if remove.kind != "fault_remove" or rng.randrange(2) != 0:
        return
    install = actions["n0"]
    dependency = (
        "n1"
        if install.payload["behavior"] == "block"
        else install.payload["target_action"]
    )
    reader = _sample_axis(
        target,
        "nemesis",
        rng,
        _context(environment, "l0"),
        requires=((dependency, FINISHED),),
        spawning=True,
    )
    actions[reader.id] = reader
    remove_index = actor_actions["a0"].index(remove.id)
    actor_actions["a0"].insert(remove_index, reader.id)


def compose(
    target: Target,
    seed: int,
    environment_target: Mapping[str, Any],
    *,
    stratum: Stratum | None = None,
    forced: bool = False,
    daemon_label: str = PUBLIC_MASTER,
    no_fault: bool = False,
    producers: str | None = None,
    differential: bool = False,
) -> RunSpec:
    """Compose and validate one candidate without retrying rejection.

    When forced is true, the target sampler may constrain generic composition
    choices to improve the selected stratum's reachability. When no_fault is
    true, the fault stage is skipped: the spec has inputs only, with no fault
    lifecycle, kill, or fault decoration. `producers` picks the producer
    stratum (`PRODUCER_STRATA`); None draws a repeated tail at
    `REPEATED_PRODUCER_RATE` when the target can revisit a state, inside
    the fault window for `FAULT_REPEATED_SHARE` of the specs with a fault.
    The draw uses its own stream, so the rest of the spec does not depend
    on it. A fault-window tail the target declines for this fault falls
    back to the trailing tail; metadata records the stratum composed.
    Pairs the target forbids to overlap are ordered before validation.

    `differential` composes for a paired run: no fault stage, fresh
    producers unless planned, and every pair with conflicting footprints
    ordered by a finished edge (`order_comparable`), as the differential
    validator admits. The environment must already opt into
    the reference side, which samplers read to pick shared variants.
    """
    if differential:
        if stratum is not None or forced:
            raise ValueError("differential composition takes no stratum")
        no_fault = True
        producers = FRESH_PRODUCERS if producers is None else producers
    if forced and stratum is None:
        raise ValueError("forced composition requires a stratum")
    if no_fault and stratum is not None:
        raise ValueError("no-fault composition takes no stratum")
    if stratum is not None and not any(
        stratum in admitted_strata(target, label) for label in DAEMON_LABELS
    ):
        raise ValueError(f"stratum {stratum!r} is not admitted")
    planned = producers is not None
    target = _composition_target(target, stratum)
    producers = _producer_stratum(
        target, seed, producers, bool(target.sites) and not no_fault
    )
    rng = derived_rng(seed, "exploratory-compose")
    actor_actions = {f"a{index}": [] for index in range(rng.choice(WIDTHS))}
    actions: dict[str, Action] = {}
    contended = rng.choice((True, False))
    input_actors, fault_end, context, environment_target = _fault_stage(
        target,
        rng,
        environment_target,
        stratum,
        daemon_label,
        contended,
        actor_actions,
        actions,
        forced,
        seed,
        no_fault,
    )
    environment_target = _complete_actions(
        target,
        seed,
        rng,
        environment_target,
        input_actors,
        fault_end,
        context,
        actor_actions,
        actions,
    )
    if producers == FAULT_REPEATED_PRODUCERS and fault_end is not None:
        if not _add_fault_window_producer(
            target, seed, environment_target, fault_end, actor_actions, actions
        ):
            if planned:
                raise ValueError("the target declines a fault-window tail here")
            producers = REPEATED_PRODUCERS
    if producers == REPEATED_PRODUCERS:
        _add_repeated_producer(target, seed, environment_target, actor_actions, actions)
    held = (
        _hold_descriptor(
            target, seed, environment_target, contended, actor_actions, actions
        )
        if fault_end is None
        else None
    )
    if held is not None:
        spec = _finish(target, seed, environment_target, *held, producers, differential)
        if not _removed_before_open(target, spec):
            return spec
    return _finish(
        target,
        seed,
        environment_target,
        actor_actions,
        actions,
        producers,
        differential,
    )


def _finish(
    target: Target,
    seed: int,
    environment_target: Mapping[str, Any],
    actor_actions: Mapping[str, list[str]],
    actions: Mapping[str, Action],
    producers: str,
    differential: bool,
) -> RunSpec:
    """Assemble, order, and validate the composed actions."""
    spec = _build_spec(target, seed, environment_target, actor_actions, actions)
    spec = replace(spec, metadata={PRODUCER_STRATUM: producers})
    spec = order_forbidden_overlaps(spec, target)
    if differential:
        spec = order_comparable(spec, target)
    validate(spec, target)
    return spec


def _hold_descriptor(
    target: Target,
    seed: int,
    environment: Mapping[str, Any],
    contended: bool,
    actor_actions: Mapping[str, list[str]],
    actions: Mapping[str, Action],
) -> tuple[dict[str, list[str]], dict[str, Action]] | None:
    """Return the actors and actions with a held descriptor added (`HeldDescriptor`), or None.

    The draw uses its own stream, so a spec the draw passes over is
    exactly the spec composed without the family. The composer also
    passes over a held open that a path remover or mover may precede
    (`_removed_before_open`).
    """
    held = target.generation.held_descriptor
    if held is None:
        return None
    rng = derived_rng(seed, "exploratory-held-descriptor")
    if rng.random() >= held.probability:
        return None
    owner = rng.choice(sorted(actor_actions))
    opened = _sample(
        target.actions[held.open],
        rng,
        _context(environment, "h0", held_descriptor=True, contended=contended),
    )
    if opened is None:
        return None
    read = _action("h1", held.read, {held.field: opened.id})
    close = _action("h2", held.close, {held.field: opened.id})
    owned = {actor: list(ids) for actor, ids in actor_actions.items()}
    ids = owned[owner]
    owned[owner] = [*ids[:-1], opened.id, *ids[-1:], read.id, close.id]
    return owned, {**actions, **{a.id: a for a in (opened, read, close)}}


def _removed_before_open(target: Target, spec: RunSpec) -> bool:
    """Return whether a path remover or mover may run before a held open and touch its path.

    Such an open may fail, leaving its read and close nothing to judge.
    An action may run before the open unless the composed order (actor
    sequence and `finished` requirements) ends the open before it starts.
    A checkout binds an aggregate, not a path, and stays: holding a
    descriptor across one is the family's point.
    """
    held = target.generation.held_descriptor
    if held is None:
        return False
    ended = must_end_before(spec)
    for opened in spec.actions:
        if opened.kind == held.open and _may_be_removed_before(
            target, spec, opened, ended
        ):
            return True
    return False


def _may_be_removed_before(
    target: Target,
    spec: RunSpec,
    opened: Action,
    ended: Mapping[str, frozenset[str]],
) -> bool:
    reads = target.actions[opened.kind].footprint(spec, opened)
    for action in spec.actions:
        definition = target.actions.get(action.kind)
        if (
            action.id == opened.id
            or opened.id in ended[action.id]
            or definition is None
            or definition.binding != Binding.PATH
            or not definition.roles & {Role.REMOVE, Role.TRANSFER}
        ):
            continue
        if footprints_interact(definition.footprint(spec, action), reads):
            return True
    return False


def _revisit_kinds(target: Target) -> tuple[str, ...]:
    """Return the input kinds that produce a whole state progressively.

    Only such a producer can return the checkout to a state an earlier
    producer (or the initial state) already produced.
    """
    return tuple(
        sorted(
            kind
            for kind, definition in target.actions.items()
            if definition.axis == "input"
            and definition.grouping == Grouping.PROGRESSIVE
            and Role.PRODUCE in definition.roles
        )
    )


def _producer_stratum(
    target: Target, seed: int, producers: str | None, has_fault: bool
) -> str:
    """Validate a planned producer stratum, or draw one from its own stream."""
    if producers is not None:
        if producers not in PRODUCER_STRATA:
            raise ValueError(f"unknown producer stratum {producers!r}")
        if producers != FRESH_PRODUCERS and not _revisit_kinds(target):
            raise ValueError(f"target {target.name!r} cannot revisit a state")
        if producers == FAULT_REPEATED_PRODUCERS and not has_fault:
            raise ValueError("a fault-window producer tail needs a fault")
        return producers
    draw = derived_rng(seed, "exploratory-producers").random()
    if draw >= REPEATED_PRODUCER_RATE or not _revisit_kinds(target):
        return FRESH_PRODUCERS
    if has_fault and draw < REPEATED_PRODUCER_RATE * FAULT_REPEATED_SHARE:
        return FAULT_REPEATED_PRODUCERS
    return REPEATED_PRODUCERS


def _sample_stage(
    target: Target,
    kinds: Iterable[str],
    rng: random.Random,
    context: Mapping[str, Any],
    requires: tuple[tuple[str, str], ...],
) -> Action | None:
    """Sample one action among `kinds`; None when the target offers none."""
    choices = [
        action
        for kind in kinds
        if (action := _sample(target.actions[kind], rng, context, requires=requires))
        is not None
    ]
    return rng.choice(choices) if choices else None


def _sample_tail(
    target: Target,
    seed: int,
    environment: Mapping[str, Any],
    actions: Mapping[str, Action],
    prior: tuple[tuple[str, str], ...],
    fault: Mapping[str, Any] | None,
) -> tuple[Action, Action, Action, Action] | None:
    """Sample the depart, return, and two observers of a repeated-producer tail.

    `v0` produces a state other than the initial one, `v1` returns to the
    initial state, `v2` observes while both may run, and `v3` observes after
    the return; `v0` and `v2` wait for `prior`. Samplers see
    `producer_stratum` (`depart`, `return`, `observe`), the departure for
    the return, and `fault_window` (site, behavior, target action) when the
    tail runs inside a fault window. None when the target declines.
    """
    rng = derived_rng(seed, "exploratory-repeated-producer")
    window = {} if fault is None else {"fault_window": fault}
    depart = _sample_stage(
        target,
        _revisit_kinds(target),
        rng,
        _context(
            environment,
            "v0",
            forced_kind=True,
            producer_stratum="depart",
            actions=actions,
            **window,
        ),
        prior,
    )
    if depart is None:
        return None
    back = _sample_stage(
        target,
        (depart.kind,),
        rng,
        _context(
            environment,
            "v1",
            forced_kind=True,
            producer_stratum="return",
            departure=depart,
            actions={**actions, depart.id: depart},
            **window,
        ),
        (),
    )
    observers = sorted(
        kind
        for kind, definition in target.actions.items()
        if definition.axis == "input"
        and definition.roles == frozenset({Role.OBSERVE})
        and definition.binding == Binding.PATH
    )
    during, after = (
        _sample_stage(
            target,
            observers,
            rng,
            _context(
                environment,
                action_id,
                forced_kind=True,
                producer_stratum="observe",
                **window,
            ),
            requires,
        )
        for action_id, requires in (("v2", prior), ("v3", ()))
    )
    if back is None or during is None or after is None:
        return None
    return depart, back, during, after


def _add_repeated_producer(
    target: Target,
    seed: int,
    environment: Mapping[str, Any],
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> None:
    """Append a repeated-producer tail (`_sample_tail`) after every other action.

    `v0`, `v1`, and `v3` go on the last actor and `v2` on the previous one.
    """
    prior = tuple((action_id, FINISHED) for action_id in sorted(actions))
    tail = _sample_tail(target, seed, environment, actions, prior, None)
    if tail is None:
        raise ValueError(f"target {target.name!r} offers no repeated-producer tail")
    depart, back, during, after = tail
    owners = list(actor_actions)
    for owner, actor_tail in (
        (owners[-1], (depart, back, after)),
        (owners[-2], (during,)),
    ):
        for action in actor_tail:
            actions[action.id] = action
            actor_actions[owner].append(action.id)


def _add_fault_window_producer(
    target: Target,
    seed: int,
    environment: Mapping[str, Any],
    fault_end: str,
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> bool:
    """Run a repeated-producer tail on two new actors while the fault is armed.

    For a block fault the tail starts once the target is parked, and the
    removal waits only for `v0` and `v2` to start, so a tail action the
    fault parks cannot hold the removal. Otherwise the tail starts once the
    target finishes, and the removal waits for it to finish. False when the
    target declines a tail inside this fault's window.
    """
    install = actions["n0"]
    target_id = install.payload["target_action"]
    block = install.payload["behavior"] == "block"
    prior = (
        tuple(
            (action.id, FINISHED)
            for action in actions.values()
            if action.kind == "fault_await"
        )
        if block
        else ((install.id, FINISHED), (target_id, FINISHED))
    )
    fault = {
        "site": install.payload["site"],
        "behavior": install.payload["behavior"],
        "target_action": actions[target_id],
    }
    tail = _sample_tail(target, seed, environment, actions, prior, fault)
    if tail is None:
        return False
    depart, back, during, after = tail
    width = len(actor_actions)
    for index, actor_tail in enumerate(((depart, back, after), (during,))):
        actor_actions[f"a{width + index}"] = [action.id for action in actor_tail]
        actions.update((action.id, action) for action in actor_tail)
    waits = (
        ((depart.id, STARTED), (during.id, STARTED))
        if block
        else ((during.id, FINISHED), (after.id, FINISHED))
    )
    remove = actions[fault_end]
    actions[fault_end] = replace(remove, requires=(*remove.requires, *waits))
    return True


def _fault_stage(
    target: Target,
    rng: random.Random,
    environment_target: Mapping[str, Any],
    stratum: Stratum | None,
    daemon_label: str,
    contended: bool,
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
    forced: bool,
    seed: int,
    no_fault: bool = False,
) -> tuple[list[str], str | None, dict[str, Any], Mapping[str, Any]]:
    """Add the fault for a target with sites, drawing the stratum when none is planned."""
    if no_fault:
        baseline = _sample_baseline_input(
            target, derived_rng(seed, "exploratory-baseline"), environment_target
        )
        actions[baseline.id] = baseline
        actor_actions["a0"].append(baseline.id)
    if not target.sites or no_fault:
        return list(actor_actions), None, {"contended": contended}, environment_target
    if stratum is None:
        stratum = rng.choice(admitted_strata(target, daemon_label))
    input_actors, fault_end, context = _add_fault(
        target,
        rng,
        environment_target,
        stratum,
        contended,
        actor_actions,
        actions,
        forced,
        seed=seed,
    )
    environment = context.get("composition_environment", environment_target)
    return input_actors, fault_end, context, environment


def _build_spec(
    target: Target,
    seed: int,
    environment_target: Mapping[str, Any],
    actor_actions: Mapping[str, list[str]],
    actions: Mapping[str, Action],
) -> RunSpec:
    """Assemble the composed actors and actions into a RunSpec with fixed budgets."""
    return RunSpec(
        seed=seed,
        target=target.name,
        environment=Environment(dict(environment_target)),
        budgets=DEFAULT_BUDGETS,
        actors=tuple(
            Actor(actor_id, tuple(action_ids))
            for actor_id, action_ids in actor_actions.items()
        ),
        actions=tuple(actions[action_id] for action_id in sorted(actions)),
    )


def _composition_target(target: Target, stratum: Stratum | None) -> Target:
    """Restrict the target's sites for composition while keeping a planned stratum's site."""
    planned_site = target.sites.get(stratum[0]) if stratum is not None else None
    admitted = generation_target(target)
    if (
        planned_site is not None
        and stratum is not None
        and stratum[0] not in admitted.sites
    ):
        # A planned stratum keeps its drawn site: generation filtering
        # restricts unplanned composition, never a planned seed.
        admitted = replace(admitted, sites={**admitted.sites, stratum[0]: planned_site})
    return admitted


def _complete_actions(
    target: Target,
    seed: int,
    rng: random.Random,
    environment_target: Mapping[str, Any],
    input_actors: list[str],
    fault_end: str | None,
    context: Mapping[str, Any],
    actor_actions: dict[str, list[str]],
    actions: dict[str, Action],
) -> Mapping[str, Any]:
    """Sample inputs; for a fault spec also decorate and order installs after a baseline."""
    _sample_inputs(
        target,
        rng,
        environment_target,
        input_actors,
        actor_actions,
        actions,
        context,
        fault_end,
    )
    if fault_end is None:
        return environment_target
    setup_override = _decorate_fault(
        target,
        rng,
        environment_target,
        input_actors,
        fault_end,
        context,
        actor_actions,
        actions,
    )
    if setup_override:
        environment_target = _override_setup_profile(environment_target, setup_override)
    baseline = _sample_baseline_input(
        target, derived_rng(seed, "exploratory-baseline"), environment_target
    )
    actions[baseline.id] = baseline
    actor_actions["a0"].insert(0, baseline.id)
    _order_installs_after_baseline(actions, baseline)
    return environment_target


def _order_installs_after_baseline(
    actions: dict[str, Action], baseline: Action
) -> None:
    """Order every install, and every mid-flight target, after the baseline input."""
    for action_id, action in actions.items():
        if action.kind == "fault_install":
            actions[action_id] = replace(
                action, requires=(*action.requires, (baseline.id, FINISHED))
            )
            target_action = actions[action.payload["target_action"]]
            if (target_action.id, STARTED) in action.requires:
                actions[target_action.id] = replace(
                    target_action,
                    requires=(*target_action.requires, (baseline.id, FINISHED)),
                )


def _placements(
    spec: RunSpec, target: Target, axis: str
) -> tuple[tuple[str, int, str], ...]:
    """Return actor positions for actions on one target-defined axis."""
    return tuple(
        sorted(
            (actor.id, index, action_id)
            for actor in spec.actors
            for index, action_id in enumerate(actor.actions)
            if target.actions[spec.action(action_id).kind].axis == axis
        )
    )


def shape_signature(spec: RunSpec, target: Target) -> tuple[Any, ...]:
    """Return the seven structural dimensions used by campaign strata."""
    install = next((a for a in spec.actions if a.kind == "fault_install"), None)
    owners = {
        action_id: actor.id for actor in spec.actors for action_id in actor.actions
    }
    path_owners: dict[object, set[str]] = {}
    for action in spec.actions:
        definition = target.actions[action.kind]
        if {"path", "value_id"} <= definition.fields:
            path_owners.setdefault(action.payload["path"], set()).add(owners[action.id])
    kills = any(target.actions[a.kind].kills_process for a in spec.actions)
    kill_mode = "process" if kills else "none"
    return (
        install.payload["site"] if install else None,
        install.payload["behavior"] if install else None,
        kill_mode,
        len(spec.actors),
        any(len(users) > 1 for users in path_owners.values()),
        _placements(spec, target, "input"),
        _placements(spec, target, "observation"),
    )


def order_forbidden_overlaps(spec: RunSpec, target: Target) -> RunSpec:
    """Order every overlapping pair the target forbids, one finished edge at a time.

    The target publishes `forbid_overlap(spec, first, second)` in
    `target.generation`; without it the spec is returned unchanged. Of a
    forbidden pair, the action whose start already depends on the other
    waits for it to finish; otherwise the later ID waits. Each edge is a
    new `finished` requirement on a pair that may overlap, so the loop ends
    within one edge per pair.
    """
    forbid = target.generation.forbid_overlap
    if forbid is None:
        return spec
    for _edge in range(len(spec.actions) ** 2 + 1):
        pair = next(
            (
                (first, second)
                for first, second in may_overlap(spec)
                if forbid(spec, spec.action(first), spec.action(second))
            ),
            None,
        )
        if pair is None:
            return spec
        spec = _wait(spec, *pair)
    raise RuntimeError("ordering forbidden overlaps did not converge")


def _wait(spec: RunSpec, first: str, second: str) -> RunSpec:
    """Make one of two actions wait for the other to finish, never closing a cycle.

    The action whose start already depends on the other waits; otherwise
    the second one does.
    """
    waiter, waited = (
        (first, second) if _starts_after(spec, first, second) else (second, first)
    )
    action = spec.action(waiter)
    return replace(
        spec,
        actions=tuple(
            replace(action, requires=(*action.requires, (waited, FINISHED)))
            if candidate.id == waiter
            else candidate
            for candidate in spec.actions
        ),
    )


def order_comparable(spec: RunSpec, target: Target) -> RunSpec:
    """Order every two actions whose declared footprints interact, one finished edge at a time.

    In the result every pair with conflicting footprints is ordered by a
    finished edge: every observation runs before or after every mutation
    of its footprint, never beside it, which the
    differential validator requires (its conflict relation is contained in
    `footprints_interact`). Each edge is a new `finished` requirement on a
    pair that may overlap, so the loop ends.
    """
    while True:
        pair = next(
            (
                (first, second)
                for first, second in may_overlap(spec)
                if footprints_interact(
                    *(
                        target.actions[action.kind].footprint(spec, action)
                        for action in (spec.action(first), spec.action(second))
                    )
                )
            ),
            None,
        )
        if pair is None:
            return spec
        spec = _wait(spec, *pair)


def _starts_after(spec: RunSpec, action_id: str, other: str) -> bool:
    """Return whether `action_id` cannot start before `other` starts."""
    edges: dict[str, set[str]] = {action.id: set() for action in spec.actions}
    for action in spec.actions:
        edges[action.id].update(dependency for dependency, _event in action.requires)
    for actor in spec.actors:
        for previous, following in zip(actor.actions, actor.actions[1:]):
            edges[following].add(previous)
    seen = {action_id}
    frontier = [action_id]
    while frontier:
        for dependency in edges[frontier.pop()]:
            if dependency == other:
                return True
            if dependency not in seen:
                seen.add(dependency)
                frontier.append(dependency)
    return False


PROGRESSIVE = "progressive"
TRANSFER = "transfer"
REMOVE = "remove"
PRODUCE = "produce"
OBSERVE = "observe"
OBSERVE_AGGREGATE = "observe-aggregate"
MUTATOR_CLASSES = (PROGRESSIVE, TRANSFER, REMOVE, PRODUCE)
OBSERVER_CLASSES = (OBSERVE, OBSERVE_AGGREGATE)


def effect_class(definition: ActionDef) -> str | None:
    """Classify an action kind by its static declarations; None touches no state.

    A progressive group (a checkout) outranks a transfer, which outranks a
    remove, which outranks a produce; observers split by binding.
    """
    roles = definition.roles
    if definition.grouping == Grouping.PROGRESSIVE:
        return PROGRESSIVE
    for role, name in (
        (Role.TRANSFER, TRANSFER),
        (Role.REMOVE, REMOVE),
        (Role.PRODUCE, PRODUCE),
    ):
        if role in roles:
            return name
    if Role.OBSERVE in roles:
        return OBSERVE_AGGREGATE if definition.binding == Binding.AGGREGATE else OBSERVE
    return None


def family(first: str, second: str) -> str:
    """Return the canonical name of the interaction family of two effect classes."""
    return "~".join(sorted((first, second)))


def declared_families(target: Target) -> tuple[str, ...]:
    """Return every family two of the target's kinds can form with a mutator in it."""
    classes = sorted(
        {
            name
            for definition in target.actions.values()
            if (name := effect_class(definition)) is not None
        }
    )
    return tuple(
        sorted(
            {
                family(first, second)
                for first in classes
                for second in classes
                if first in MUTATOR_CLASSES or second in MUTATOR_CLASSES
            }
        )
    )


def generation_summary(spec: RunSpec, target: Target) -> dict[str, int]:
    """Return the deterministic interaction and producer counts of one spec.

    `family:<a>~<b>` counts overlapping pairs whose footprints interact, per
    declared family (zero when absent). `producers:<n>` counts observed
    paths with n candidate producers: the initial state unless a producer
    of the path must end before the observer starts, plus every producer
    whose footprint writes the path and that need not start after the
    observer ends. `producers:ambiguous` counts those with two or more,
    `producers:repeated` those where two candidates produce the same value
    by the target's published `produced_value(spec, action_or_initial,
    path)`, and `producers:revisits-initial` those where a candidate
    action reproduces the initial value, whether or not the initial state
    is still a candidate. `strata:repeated` is 1 when the spec's metadata
    records a repeated-producer stratum, and `strata:repeated-in-fault`
    when that tail runs inside the fault window.

    Producer counts are static upper bounds over candidates, not the
    producers a run can witness: a producer superseded by a later finished
    producer of the same path still counts, any action whose roles include
    PRODUCE or TRANSFER is a candidate (an `sl_rm` is, a REMOVE-only
    `unlink` is not), an aggregate observer sees only paths some candidate
    writes exactly, and a checkout produces its revision even when a run
    would abort it or keep a conflicting path.
    """
    before = must_end_before(spec)
    footprints = {
        action.id: target.actions[action.kind].footprint(spec, action)
        for action in spec.actions
    }
    classes = {
        action.id: effect_class(target.actions[action.kind]) for action in spec.actions
    }
    counts: Counter[str] = Counter(
        {f"family:{name}": 0 for name in declared_families(target)}
    )
    for first, second in may_overlap(spec, before):
        left, right = classes[first], classes[second]
        if left is None or right is None:
            continue
        if left in OBSERVER_CLASSES and right in OBSERVER_CLASSES:
            continue
        if footprints_interact(footprints[first], footprints[second]):
            counts[f"family:{family(left, right)}"] += 1
    counts.update(_producer_counts(spec, target, before, footprints, classes))
    stratum = spec.metadata.get(PRODUCER_STRATUM)
    counts["strata:repeated"] = int(
        stratum in (REPEATED_PRODUCERS, FAULT_REPEATED_PRODUCERS)
    )
    counts["strata:repeated-in-fault"] = int(stratum == FAULT_REPEATED_PRODUCERS)
    return dict(sorted(counts.items()))


def _observed_paths(footprint: Footprint, written: Iterable[str]) -> set[str]:
    """Return the exact paths one observer reads, including written paths in its subtrees."""
    paths = set(footprint.reads)
    for path in written:
        if any(_under(path, root) for root in footprint.subtrees_read):
            paths.add(path)
    return paths


def _covers_write(footprint: Footprint, path: str) -> bool:
    return path in footprint.writes or any(
        _under(path, root) for root in footprint.subtrees_written
    )


def _producer_counts(
    spec: RunSpec,
    target: Target,
    before: Mapping[str, frozenset[str]],
    footprints: Mapping[str, Footprint],
    classes: Mapping[str, str | None],
) -> Counter[str]:
    """Count candidate producers per observed path; see `generation_summary`."""
    produced_value = target.generation.produced_value
    producers = sorted(
        action.id
        for action in spec.actions
        if target.actions[action.kind].roles & {Role.PRODUCE, Role.TRANSFER}
    )
    written = {path for action_id in producers for path in footprints[action_id].writes}
    counts: Counter[str] = Counter(
        {
            "producers:ambiguous": 0,
            "producers:repeated": 0,
            "producers:revisits-initial": 0,
        }
    )
    for observer in sorted(spec.actions, key=lambda action: action.id):
        if classes[observer.id] not in OBSERVER_CLASSES:
            continue
        for path in sorted(_observed_paths(footprints[observer.id], written)):
            candidates = [
                action_id
                for action_id in producers
                if _covers_write(footprints[action_id], path)
                and observer.id not in before[action_id]
            ]
            initial = not any(
                action_id in before[observer.id] for action_id in candidates
            )
            total = len(candidates) + initial
            counts[f"producers:{total}"] += 1
            counts["producers:ambiguous"] += total >= 2
            if produced_value is None:
                continue
            values = [produced_value(spec, spec.action(a), path) for a in candidates]
            start = produced_value(spec, None, path)
            counts["producers:revisits-initial"] += (
                start is not None and start in values
            )
            if initial:
                values.append(start)
            known = [value for value in values if value is not None]
            counts["producers:repeated"] += len(set(known)) < len(known)
    return counts


def summarize_generation(specs: Iterable[RunSpec], target: Target) -> dict[str, int]:
    """Sum `generation_summary` over specs, with sorted keys; adds `specs`."""
    total: Counter[str] = Counter()
    count = 0
    for spec in specs:
        count += 1
        total.update(generation_summary(spec, target))
    for name in declared_families(target):
        total.setdefault(f"family:{name}", 0)
    total["specs"] = count
    return dict(sorted(total.items()))
