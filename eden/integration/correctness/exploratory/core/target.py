# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The contract between the target-neutral core and one target.

A target supplies a `Target`: its actions (`ActionDef`) and fault sites,
how to run them (environment and backend), how to model effects from
their results, and its properties (`PropertyDef`, in `property`). This
module also defines the validators the checker applies to every target
declaration (`validate_consumption`, `validate_target_sources`).
"""

from __future__ import annotations

import os
import random
import re
import subprocess
import threading
import time
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass, field, KW_ONLY
from enum import StrEnum
from typing import Any, Protocol, TYPE_CHECKING, TypeAlias

from .jsonio import MAX_ID_BYTES, require as _require
from .property import _check_name, _utf8_len, is_core, PropertyDef, validate_registry
from .sites import Site

if TYPE_CHECKING:
    from .effects import EffectContracts, Footprint, PinnedFacts
    from .facts import HistoryFacts, ResultSchema
    from .spec_types import Action, RunSpec

    ActionPredicate: TypeAlias = Callable[[RunSpec, HistoryFacts, Action], bool]
    FootprintFn: TypeAlias = Callable[[RunSpec, Action], Footprint]
    InputSubjectFn: TypeAlias = Callable[[RunSpec, Action], str]
    SidePredicate: TypeAlias = Callable[[RunSpec, Action], bool]
    PinnedFn: TypeAlias = Callable[[RunSpec], PinnedFacts]
else:
    ActionPredicate: TypeAlias = Callable[..., bool]
    FootprintFn: TypeAlias = Callable[..., Any]
    InputSubjectFn: TypeAlias = Callable[..., str]
    PinnedFn: TypeAlias = Callable[..., Any]


DEFAULT_SEMANTICS_PROFILE = "unversioned"


_PROFILE = re.compile(r"[a-z0-9][a-z0-9.-]*")
_MODULE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*")
MAX_MODULE_NAME_BYTES = 256
MAX_SOURCE_MODULES = 64


@dataclass(frozen=True)
class OwnedProcess:
    """One action-owned process group and its creation time."""

    process: subprocess.Popen[Any]
    pgid: int
    spawned_at: float


class ProcessRegistry:
    """Track action-owned process groups within one environment."""

    def __init__(self) -> None:
        self._condition = threading.Condition()
        self._running: dict[str, OwnedProcess] = {}
        # Retired actions and the process-group ID each one had.
        self._finished: dict[str, int] = {}

    def publish(
        self,
        action_id: str,
        process: subprocess.Popen[Any],
        spawned_at: float,
    ) -> None:
        """Publish a newly created process group for one action.

        The process must lead its own group (pid == pgid); anything else is refused.
        """
        if os.getpgid(process.pid) != process.pid:
            raise RuntimeError(f"process action {action_id!r} does not lead its group")
        with self._condition:
            if action_id in self._running or action_id in self._finished:
                raise RuntimeError(f"process action {action_id!r} published twice")
            self._running[action_id] = OwnedProcess(process, process.pid, spawned_at)
            self._condition.notify_all()

    def retire(self, action_id: str, process: subprocess.Popen[Any]) -> None:
        """Retire one process only when its exact owner requests it."""
        with self._condition:
            owned = self._running.get(action_id)
            if owned is None or owned.process is not process:
                raise RuntimeError(f"process action {action_id!r} is not active")
            del self._running[action_id]
            self._finished[action_id] = owned.pgid
            self._condition.notify_all()

    def lookup(self, action_id: str) -> int | None:
        """Return the running action's exact process-group ID."""
        with self._condition:
            owned = self._running.get(action_id)
            return None if owned is None else owned.pgid

    def finished(self, action_id: str) -> int | None:
        """Return the process-group ID a retired action had, or None if it is not retired."""
        with self._condition:
            return self._finished.get(action_id)

    def wait(self, action_id: str, timeout: float) -> OwnedProcess | None:
        """Wait for process creation; return None after finish or timeout."""
        deadline = time.monotonic() + timeout
        with self._condition:
            while action_id not in self._running:
                if action_id in self._finished:
                    return None
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return None
                self._condition.wait(timeout=remaining)
            return self._running[action_id]


RUN_DIR_ENV = "EDEN_EXPLORATORY_RUN_DIR"  # supervisor-owned sandbox, set for the runner


class EnvironmentDegraded(RuntimeError):
    """A setup failure caused by exhausted external environment resources."""

    def __init__(self, reason: str, facts: Mapping[str, Any]) -> None:
        super().__init__(reason)
        self.reason = reason
        self.facts = dict(facts)


class Environment(Protocol):
    """Lifecycle and process controls supplied by a target."""

    def setup(self) -> None:
        """Create the target environment."""

    def close(self) -> str | None:
        """Close the target environment and return any cleanup error."""

    @property
    def processes(self) -> ProcessRegistry:
        """Return the target's owned process registry."""


class Backend(Protocol):
    """Action executor supplied by a target."""

    def __call__(self, action: Any) -> Any:
        """Execute one action."""

    def unwind(self, sink: Any, clock: Any) -> dict[str, Any]:
        """Release target state after execution."""


ActionValidator = Callable[[Any, Any], None]
ActionSampler = Callable[[random.Random, Mapping[str, Any]], Mapping[str, Any] | None]


class Role(StrEnum):
    """What an action can do to modeled state."""

    PRODUCE = "produce"
    REMOVE = "remove"
    TRANSFER = "transfer"
    OBSERVE = "observe"
    CAN_SUPERSEDE = "can-supersede"


class Binding(StrEnum):
    """What an action's effects bind to."""

    NONE = "none"
    PATH = "path"
    DESCRIPTOR = "descriptor"
    AGGREGATE = "aggregate"


class Grouping(StrEnum):
    """How an action's effects take effect together.

    `single` is one subject at one point; `progressive` is one group per
    subject; `atomic` is every subject at one point; `aggregate` covers a
    domain.
    """

    NONE = "none"
    SINGLE = "single"
    PROGRESSIVE = "progressive"
    ATOMIC = "atomic"
    AGGREGATE = "aggregate"


MUTATING_ROLES = frozenset({Role.PRODUCE, Role.REMOVE, Role.TRANSFER})


@dataclass(frozen=True)
class ActionDef:
    """Schema, behavior metadata, and static effect declarations for one action kind.

    `result` is the exact result schema per outcome; `footprint(spec,
    action)` names the modeled paths the action may touch; `roles`,
    `binding`, and `grouping` say what its effects can be.
    `input_subject(spec, action)`, when declared, returns the canonical
    path a modeled input is about (a rename's source, a descriptor's
    path); `core.input-legality` keys the input's obligation by it.
    Without it the key is the footprint's single path, or ROOT when the
    footprint names ROOT. `kills_process` declares a nemesis that kills
    another action's process; its window lasts from its invoke to its
    ok/fail completion, or forever. Generators may read these declarations;
    they never import checker code.
    """

    kind: str
    axis: str
    fields: frozenset[str]
    required: frozenset[str]
    types: Mapping[str, type]
    validate: ActionValidator
    modeled: bool
    spawns_process: bool
    sample: ActionSampler
    _: KW_ONLY
    result: ResultSchema
    footprint: FootprintFn
    roles: frozenset[Role]
    binding: Binding
    grouping: Grouping
    input_subject: InputSubjectFn | None = None
    kills_process: bool = False

    def __post_init__(self) -> None:
        validate_action_def(self)


def validate_action_def(definition: ActionDef) -> None:
    """Reject static declarations that are malformed or contradict each other.

    Roles, binding, and grouping are from their vocabularies; an action
    that mutates state groups its effects and one that only observes does
    not; an action with roles binds to something; a single group binds a
    path or descriptor and an aggregate group an aggregate; the result is
    a ResultSchema. A modeled input declares `input_subject` unless it
    binds a path or an aggregate, the bindings whose footprint names the
    input's subject.
    """
    kind = definition.kind
    _require(callable(definition.footprint), f"{kind}: footprint is not callable")
    _require(
        definition.input_subject is None or callable(definition.input_subject),
        f"{kind}: input_subject is not callable",
    )
    _require(
        type(definition.roles) is frozenset
        and all(role in frozenset(Role) for role in definition.roles),
        f"{kind}: roles must be a frozenset of Role",
    )
    _require(definition.binding in frozenset(Binding), f"{kind}: bad binding")
    _require(definition.grouping in frozenset(Grouping), f"{kind}: bad grouping")
    mutates = bool(definition.roles & MUTATING_ROLES)
    _require(
        mutates == (definition.grouping != Grouping.NONE),
        f"{kind}: an action groups its effects exactly when it mutates state",
    )
    _require(
        Role.CAN_SUPERSEDE not in definition.roles or mutates,
        f"{kind}: only a mutating action can supersede",
    )
    _require(
        definition.binding != Binding.NONE or not definition.roles,
        f"{kind}: an action with roles binds its effects to something",
    )
    _require(
        definition.grouping != Grouping.SINGLE
        or definition.binding in (Binding.PATH, Binding.DESCRIPTOR),
        f"{kind}: a single group binds one path or descriptor",
    )
    _require(
        (definition.grouping == Grouping.AGGREGATE)
        <= (definition.binding == Binding.AGGREGATE),
        f"{kind}: an aggregate group binds an aggregate",
    )
    _require(
        not (definition.modeled and definition.axis == "input")
        or definition.input_subject is not None
        or definition.binding in (Binding.PATH, Binding.AGGREGATE),
        f"{kind}: a modeled {definition.binding} input declares input_subject",
    )
    # facts imports history, which imports this module.
    from .facts import ResultSchema

    _require(
        isinstance(definition.result, ResultSchema),
        f"{kind}: result is not a ResultSchema",
    )


@dataclass(frozen=True)
class HeldInput:
    """A target-published input family: pause a stream around a partner input.

    The composer places `stream` before the fault target when the target's
    kind is `partner`, resumes it afterwards with `{resume_field: stream id}`,
    and merges `setup_profile` into the environment.
    """

    stream: str
    resume: str
    partner: str
    probability: float
    setup_profile: Mapping[str, Any]
    stream_payload: Mapping[str, Any]
    resume_field: str


@dataclass(frozen=True)
class HeldDescriptor:
    """A target-published input family: hold a descriptor across one actor's inputs.

    In a spec without a fault stage the composer, at `probability`, opens a
    path with `open` (its sampler sees `held_descriptor` in the context)
    just before one actor's last composed action, then reads through it
    with `read` and closes it with `close` (`{field: open id}`) after that
    action. It keeps the spec only when no path-bound remove or transfer
    outside that one action touches the opened path (such an open may
    fail).
    """

    open: str
    read: str
    close: str
    field: str
    probability: float


@dataclass(frozen=True)
class GenerationOptions:
    """What a target publishes for composition; the defaults publish nothing.

    `observable_target_kinds(site_name, site)` narrows a site's target kinds
    for generated attempts (an empty set drops the site; None keeps every
    declared kind). `sample_observation_pair(rng, context)` may replace a
    sampled observation by `(kind, payload)` pairs. `held_inputs` and
    `held_descriptor` are the held families. Faults at `uncontended_sites`
    are composed uncontended. `forbid_overlap(spec, first, second)` names
    pairs the composer orders. `produced_value(spec, producer, path)` is the
    value a producer (None: the initial state) leaves at a path.
    """

    observable_target_kinds: Callable[[str, Site], frozenset[str]] | None = None
    sample_observation_pair: (
        Callable[[random.Random, Mapping[str, Any]], Any] | None
    ) = None
    held_inputs: tuple[HeldInput, ...] = ()
    held_descriptor: HeldDescriptor | None = None
    uncontended_sites: frozenset[str] = frozenset()
    forbid_overlap: Callable[[Any, Any, Any], bool] | None = None
    produced_value: Callable[[Any, Any, str], object] | None = None


@dataclass(frozen=True, kw_only=True)
class TargetSources:
    """The source-dependency contract a target declares for the checker digest.

    `implementation` is the module that defines the target: its action
    table, predicates, and property registry. `modules` are further
    behavior dependencies no static import reaches. `dynamic` names the
    followed modules some followed module loads with a direct
    ``importlib.import_module``/``__import__`` call naming that module
    literally; every other dynamic load fails the digest closed, and each
    declared module is digested. The digest follows static imports from these, from
    each property callable's module, and from the core checker through
    the checker's namespace and `packages`. `external` names packages
    those imports may reach that are runtime environment rather than
    verdict logic (the build attests them); any other import outside the
    followed packages and the standard library fails the digest closed.
    Every entry is an absolute dotted name, never a path, so a digest is
    the same in every checkout.
    """

    implementation: str
    modules: tuple[str, ...] = ()
    packages: tuple[str, ...] = ()
    external: tuple[str, ...] = ()
    dynamic: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        validate_target_sources(self)


def _covers(package: str, name: str) -> bool:
    return name == package or name.startswith(package + ".")


def validate_target_sources(sources: TargetSources) -> None:
    """Reject a malformed source-dependency declaration."""
    _require(
        isinstance(sources, TargetSources),
        f"{type(sources).__name__} is not a TargetSources",
    )
    groups = {
        "modules": sources.modules,
        "packages": sources.packages,
        "external": sources.external,
        "dynamic": sources.dynamic,
    }
    for group, names in groups.items():
        _require(type(names) is tuple, f"TargetSources.{group} must be a tuple")
        _require(
            len(names) <= MAX_SOURCE_MODULES,
            f"TargetSources.{group} declares more than {MAX_SOURCE_MODULES} names",
        )
        _require(len(set(names)) == len(names), f"duplicate in TargetSources.{group}")
    for name in (
        sources.implementation,
        *sources.modules,
        *sources.packages,
        *sources.external,
        *sources.dynamic,
    ):
        _require(
            type(name) is str
            and _MODULE.fullmatch(name) is not None
            and _utf8_len(name) <= MAX_MODULE_NAME_BYTES,
            f"bad source module name {name!r:.300}",
        )
    _require(
        sources.implementation not in sources.modules,
        "TargetSources.modules repeats the implementation",
    )
    for external in sources.external:
        for name in (
            sources.implementation,
            *sources.modules,
            *sources.packages,
            *sources.dynamic,
        ):
            _require(
                not _covers(external, name) and not _covers(name, external),
                f"external package {external!r} overlaps followed source {name!r}",
            )


@dataclass(frozen=True)
class Target:
    """All target-owned behavior and schemas used by the core harness.

    `install_witnessed(spec, facts, install)` says whether an `ok` fault
    install was seen to fire; `timeout_capable(spec, facts, action)` says
    whether the action ran on a channel documented to fail a request
    parked past its timeout with ETIMEDOUT (None means no channel is).
    Both are pure functions over the history facts. `properties` are the
    target's properties in evaluation order; the core properties are
    implicit, and a target without properties is recording-only.
    `contracts` turns facts and pinned fixture facts into effect groups;
    `pinned(spec)` returns the fixture's indexed facts. `semantics_profile`
    names the guarantee set the rule table encodes. `serving_paths` maps
    each observation channel to its serving-path independence group, which
    attributes engine proofs (`checker._attribute_proof`). `sources` declares the modules its verdicts
    depend on; a target without it has no semantic digest.
    `expects_reference(spec, action)` says whether the action owes a
    differential reference side in its `ok` or `fail` terminal event: the
    one predicate the runtime that runs the side and the checker that
    judges its presence share. None means no action owes one.
    `contract_consumes` names the result fields the `contracts` hook reads
    (what its contracts make of them is judged through the obligations it
    supports); `unported_fields` names each consumed result field no
    property of this target reads yet, with the reason (`validate_consumption`).
    `generation` is what the target publishes for composition.
    """

    name: str
    actions: Mapping[str, ActionDef]
    sites: Mapping[str, Site]
    properties: tuple[PropertyDef, ...]
    install_witnessed: ActionPredicate
    environment_fields: Mapping[str, type]
    make_environment: Callable[[Any, Any], Environment]
    make_backend: Callable[[Environment, Any, str | None], Backend]
    red_controls: frozenset[str]
    semantics_profile: str = DEFAULT_SEMANTICS_PROFILE
    serving_paths: Mapping[str, str] = field(default_factory=dict)
    timeout_capable: ActionPredicate | None = None
    sources: TargetSources | None = None
    contracts: EffectContracts | None = None
    pinned: PinnedFn | None = None
    expects_reference: SidePredicate | None = None
    contract_consumes: frozenset[tuple[str, str]] = frozenset()
    unported_fields: Mapping[tuple[str, str], str] = field(default_factory=dict)
    generation: GenerationOptions = field(default_factory=GenerationOptions)

    def __post_init__(self) -> None:
        validate_target_metadata(self.semantics_profile, self.serving_paths)
        if self.sources is not None:
            validate_target_sources(self.sources)
        for kind, definition in self.actions.items():
            _require(
                isinstance(definition, ActionDef) and definition.kind == kind,
                f"{self.name}: action {kind!r} is not its ActionDef",
            )
        for definition in self.properties:
            _require(
                isinstance(definition, PropertyDef),
                f"{self.name}: property {definition!r:.200} is not a PropertyDef",
            )
            _require(
                not is_core(definition.id),
                f"{self.name}: core property {definition.id!r} is implicit",
            )
        for hook in ("contracts", "pinned", "expects_reference"):
            value = getattr(self, hook)
            _require(
                value is None or callable(value), f"{self.name}: {hook} is not callable"
            )
        validate_registry(self.properties, self.red_controls)
        if self.properties:
            validate_consumption(
                self.actions,
                self.properties,
                contract_consumes=self.contract_consumes,
                unported=self.unported_fields,
            )


_OUTCOME_NAMES = ("ok", "fail", "info")


def _core_consumed(definition: ActionDef, outcome: str, path: str) -> bool:
    """Return whether the core reads a result field: a modeled input's fail classifier.

    That is its `errno`, plus its `error_type` when the schema declares
    error types.
    """
    return (
        definition.modeled
        and definition.axis == "input"
        and outcome == "fail"
        and path in definition.result.failure_fields()
    )


def _field_paths(fields: Iterable[Any], prefix: str = "") -> Iterable[str]:
    """Yield the dotted path of every declared field and nested member."""
    for spec in fields:
        path = f"{prefix}{spec.name}"
        yield path
        if spec.schema is not None:
            yield from _field_paths(spec.schema, f"{path}.")


def _consumption_units(fields: Iterable[Any], prefix: str = "") -> Iterable[str]:
    """Yield the dotted paths some property must consume.

    A field with `consumed=False` makes its whole subtree diagnostic. A
    consumed object field is consumed through its consumed members, each
    by its own dotted path; one with no consumed member is its own unit.
    """
    for spec in fields:
        if not spec.consumed:
            continue
        path = f"{prefix}{spec.name}"
        members = [] if spec.schema is None else list(spec.schema)
        units = list(_consumption_units(members, f"{path}."))
        yield from units if units else (path,)


def validate_consumption(
    actions: Mapping[str, ActionDef],
    properties: Iterable[PropertyDef],
    *,
    contract_consumes: frozenset[tuple[str, str]] = frozenset(),
    unported: Mapping[tuple[str, str], str] | None = None,
) -> None:
    """Reject a consumed result field nobody reads or reports, or a read of no field.

    Every consumption unit (`_consumption_units`: a consumed scalar field,
    or `field.member` for each consumed member of a consumed object) of
    every action's result schema, modeled or not, is read by some property
    or by the contracts hook (`contract_consumes`), or is reported in `unported`
    with a reason. The core itself reads a modeled input's failure
    classifier: its top-level `errno`, and its `error_type` when the
    schema declares error types (`_core_consumed`). Every pair a property
    or the contracts hook consumes, and every unported pair, names a declared
    field or dotted member of a declared action, and a field something
    reads is never also reported unported.
    """
    unported = {} if unported is None else unported
    consumed = {pair for definition in properties for pair in definition.consumes}
    declared = {
        (kind, path)
        for kind, definition in actions.items()
        for outcome in _OUTCOME_NAMES
        for path in _field_paths(definition.result.fields(outcome))
    }
    unknown = sorted(consumed - declared)
    _require(not unknown, f"properties consume undeclared result fields {unknown}")
    unknown = sorted((set(contract_consumes) | set(unported)) - declared)
    _require(not unknown, f"contracts or unported names undeclared fields {unknown}")
    read = consumed | set(contract_consumes)
    stale = sorted(set(unported) & read)
    _require(not stale, f"result fields {stale} are read yet reported unported")
    for pair, reason in sorted(unported.items()):
        _require(
            type(reason) is str and reason.strip() != "",
            f"unported field {pair} names no reason",
        )
    unread = sorted(
        (kind, path)
        for kind, definition in actions.items()
        for outcome in _OUTCOME_NAMES
        for path in _consumption_units(definition.result.fields(outcome))
        if not _core_consumed(definition, outcome, path)
        and (kind, path) not in read
        and (kind, path) not in unported
    )
    _require(not unread, f"no property consumes the result fields {unread}")


def validate_target_metadata(
    semantics_profile: str, serving_paths: Mapping[str, str]
) -> None:
    """Reject a malformed semantics profile or serving-path declaration."""
    _require(
        type(semantics_profile) is str
        and _PROFILE.fullmatch(semantics_profile) is not None
        and _utf8_len(semantics_profile) <= MAX_ID_BYTES,
        f"bad semantics profile {semantics_profile!r}",
    )
    for observer, group in serving_paths.items():
        _check_name(observer, "serving-path observer")
        _check_name(group, f"serving-path group for {observer!r}")
