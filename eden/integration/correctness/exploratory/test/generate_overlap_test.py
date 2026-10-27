# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""How composition orders and places actions.

A fault's install, await, and remove keep a legal lifetime for every behavior
and install order; observations land before, during, and after the fault
lifetime; a nemesis targets another actor and a process kill a started action;
the modeled baseline input falls outside every fault window; a lookup is never
composed contended and an inode-map load unloads before its install; background
load reuses the nemesis actor in a safe order; and a checkout never
overlaps a namespace operation on a path it changes (`forbid_overlap`, enforced
by validation and by composition), while namespace samplers still reach their
interaction families.
"""

from __future__ import annotations

import dataclasses
import random
import unittest
from collections import Counter
from collections.abc import Callable, Mapping
from pathlib import PurePosixPath
from typing import Any

from eden.integration.correctness.exploratory.core.actions import CORE_ACTION_DEFS
from eden.integration.correctness.exploratory.core.generate import (
    _action,
    _add_fault,
    _add_kill_mode,
    _compose_fault,
    _context,
    _decorate_fault,
    _sample,
    _sample_axis,
    _sample_inputs,
    derived_rng,
    generation_target,
    may_overlap,
    must_end_before,
    order_forbidden_overlaps,
    shape_signature as composed_shape_signature,
    summarize_generation,
    WIDTHS,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    Environment,
    FINISHED,
    RunSpec,
    SpecError,
    STARTED,
    validate,
)
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs import sites as edenfs_sites
from eden.integration.correctness.exploratory.edenfs.actions import (
    forbid_overlap,
    WALK_DEADLOCK_TASK,
)
from eden.integration.correctness.exploratory.edenfs.sites import (
    CHECKOUT_BEGIN_SITE,
    INODEMAP_LOAD_SITE,
    LOOKUP_SITE,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    RECORDING_TARGET,
    RECORDING_TARGET as EDENFS_TARGET,
)
from eden.integration.correctness.exploratory.test.campaign_fixtures import (
    campaign_specs,
)
from eden.integration.correctness.exploratory.test.generation_fixtures import (
    action_owners,
    actor_spec,
    checkout_action,
    compose_sampled,
    edenfs_checkout_spec,
    generate_specs,
    REGISTERS,
)


class ComposerBuildingBlockTest(unittest.TestCase):
    def _environment(self):
        return edenfs_checkout_spec().environment.target

    def _validated_spec(
        self,
        actor_actions: dict[str, list[str]],
        actions: dict[str, Action],
        setup_override: Mapping[str, Any] | None = None,
    ) -> RunSpec:
        environment = dict(self._environment())
        environment["setup_profile"] = {
            **environment["setup_profile"],
            **(setup_override or {}),
        }
        spec = RunSpec(
            seed=41,
            target="edenfs",
            environment=Environment(environment),
            budgets=Budgets(120, 30, 600),
            actors=tuple(
                Actor(actor_id, tuple(action_ids))
                for actor_id, action_ids in actor_actions.items()
            ),
            actions=tuple(actions[action_id] for action_id in sorted(actions)),
        )
        validate(spec, EDENFS_TARGET)
        return spec

    def _fault_parts(
        self, behavior: str, seed: int = 41
    ) -> tuple[Action, list[Action], str, dict[str, object]]:
        target_action, lifecycle, terminal, context = _compose_fault(
            EDENFS_TARGET,
            random.Random(seed),
            self._environment(),
            (CHECKOUT_BEGIN_SITE, behavior, "none"),
            False,
        )
        return target_action, lifecycle, terminal, context

    def _validate_fault_parts(
        self, target_action: Action, lifecycle: list[Action]
    ) -> RunSpec:
        actions = {action.id: action for action in (target_action, *lifecycle)}
        return self._validated_spec(
            {"a0": [action.id for action in lifecycle], "a1": [target_action.id]},
            actions,
        )

    def test_sampling_primitives_preserve_context_and_validate_payloads(self) -> None:
        environment = self._environment()
        marker = object()
        context = _context(environment, "i0", forced_kind=True, opaque_marker=marker)
        self.assertIs(environment, context["environment"])
        self.assertIs(marker, context["opaque_marker"])
        sampled = _sample(EDENFS_TARGET.actions["checkout"], random.Random(7), context)
        self.assertIsNotNone(sampled)
        assert sampled is not None
        observation = _sample_axis(
            EDENFS_TARGET,
            "observation",
            random.Random(8),
            _context(environment, "o0"),
        )
        sleeper = _action("p0", "sleep", {"seconds": 1.0})
        spec = self._validated_spec(
            {"a0": [sampled.id, observation.id, sleeper.id]},
            {action.id: action for action in (sampled, observation, sleeper)},
        )
        self.assertEqual("operation", sleeper.deadline)
        self.assertEqual(sampled.payload, spec.action("i0").payload)
        self.assertIsNone(
            _sample(
                CORE_ACTION_DEFS["sleep"],
                random.Random(9),
                _context(environment, "unused"),
            )
        )

    def test_fault_lifecycles_validate_all_behaviors_and_install_orders(self) -> None:
        install_orders: set[str] = set()
        for behavior in ("block", "error", "delay"):
            for seed in range(8):
                target_action, lifecycle, _terminal, _context_values = (
                    self._fault_parts(behavior, seed)
                )
                install = lifecycle[0]
                mid_flight = (target_action.id, "started") in install.requires
                pre_armed = (install.id, "finished") in target_action.requires
                self.assertNotEqual(mid_flight, pre_armed)
                install_orders.add("mid" if mid_flight else "pre")
                expected = {
                    "block": ["fault_install", "fault_await", "fault_remove"],
                    "error": ["fault_install", "fault_remove"],
                    "delay": ["fault_install", "fault_remove"],
                }[behavior]
                self.assertEqual(expected, [action.kind for action in lifecycle])
                self.assertEqual(
                    "run" if behavior == "block" else "operation",
                    target_action.deadline,
                )
                self._validate_fault_parts(target_action, lifecycle)
            if behavior == "delay":
                self.assertEqual(100, lifecycle[0].payload["delay_ms"])
        self.assertEqual({"pre", "mid"}, install_orders)

    def test_process_kills_target_another_actor(self) -> None:
        for kill_mode in ("process",):
            target_action, lifecycle, terminal, _context_values = self._fault_parts(
                "error"
            )
            actor_actions = {
                "a0": [action.id for action in lifecycle],
                "a1": [target_action.id],
            }
            actions = {action.id: action for action in (target_action, *lifecycle)}
            _add_kill_mode(
                EDENFS_TARGET,
                random.Random(7),
                self._environment(),
                kill_mode,
                "a1",
                terminal,
                actor_actions,
                actions,
            )
            spec = self._validated_spec(actor_actions, actions)
            kill = spec.action("k0")
            target_id = kill.payload.get("target_action", kill.payload.get("while"))
            owners = {
                action_id: actor.id
                for actor in spec.actors
                for action_id in actor.actions
            }
            self.assertNotEqual(owners[kill.id], owners[target_id])
            self.assertIn((target_id, "started"), kill.requires)

    def test_fault_input_and_decoration_helpers_build_a_valid_graph(self) -> None:
        actor_actions = {"a0": [], "a1": [], "a2": []}
        actions: dict[str, Action] = {}
        rng = random.Random(19)
        input_actors, terminal, context = _add_fault(
            EDENFS_TARGET,
            rng,
            self._environment(),
            (CHECKOUT_BEGIN_SITE, "error", "none"),
            True,
            actor_actions,
            actions,
        )
        _sample_inputs(
            EDENFS_TARGET,
            rng,
            self._environment(),
            input_actors,
            actor_actions,
            actions,
            context,
            terminal,
        )
        override = _decorate_fault(
            EDENFS_TARGET,
            rng,
            self._environment(),
            input_actors,
            terminal,
            context,
            actor_actions,
            actions,
        )
        spec = self._validated_spec(actor_actions, actions, override)
        self.assertEqual(
            {"i1", "i2"},
            {a.id for a in spec.actions if a.id != "i0" and a.id.startswith("i")},
        )
        for action_id in ("i1", "i2"):
            self.assertIn((terminal, "finished"), spec.action(action_id).requires)
        self.assertIn("o0", {action.id for action in spec.actions})


class ComposedOrderingTest(unittest.TestCase):
    def test_background_load_uses_existing_nemesis_actor_and_safe_order(self) -> None:
        """Generated load brackets removal after the witness or target completion."""
        for behavior in ("block", "error", "delay"):
            for seed in range(500):
                spec = compose_sampled(
                    EDENFS_TARGET,
                    seed,
                    stratum=(CHECKOUT_BEGIN_SITE, behavior, "none"),
                )
                readers = [a for a in spec.actions if a.kind == "background_read"]
                if not readers:
                    continue
                (reader,) = readers
                expected_width = derived_rng(seed, "exploratory-compose").choice(WIDTHS)
                self.assertEqual(expected_width, len(spec.actors))
                nemesis = next(actor for actor in spec.actors if actor.id == "a0")
                remove = next(a for a in spec.actions if a.kind == "fault_remove")
                self.assertEqual(
                    nemesis.actions.index(reader.id) + 1,
                    nemesis.actions.index(remove.id),
                )
                dependency = "n1" if behavior == "block" else "i0"
                self.assertIn((dependency, "finished"), reader.requires)
                self.assertEqual("b0", nemesis.actions[0])
                validate(spec, EDENFS_TARGET)
                break
            else:
                self.fail(f"no background_read sampled for {behavior}")

    def test_every_fault_spec_has_baseline_outside_windows(self) -> None:
        """An acknowledged modeled input precedes every possible fault window."""
        for seed in range(500):
            with self.subTest(seed=seed):
                spec = compose_sampled(EDENFS_TARGET, seed)
                baseline = spec.action("b0")
                definition = EDENFS_TARGET.actions[baseline.kind]
                self.assertTrue(definition.modeled)
                self.assertEqual("input", definition.axis)
                self.assertEqual((), baseline.requires)
                owner = next(actor for actor in spec.actors if "b0" in actor.actions)
                self.assertEqual("b0", owner.actions[0])
                for action in spec.actions:
                    if action.kind == "fault_install":
                        self.assertIn(("b0", "finished"), action.requires)
                        target = spec.action(action.payload["target_action"])
                        if (target.id, "started") in action.requires:
                            self.assertIn(("b0", "finished"), target.requires)
                            self.assertNotIn((action.id, "finished"), target.requires)
                        else:
                            self.assertIn((action.id, "finished"), target.requires)

    def test_no_unobservable_pair_generated(self) -> None:
        """Historical fault misses are excluded from composition, not validation."""
        for seed in range(1000):
            spec = compose_sampled(EDENFS_TARGET, seed)
            for action in spec.actions:
                if action.kind == "fault_install":
                    target = spec.action(action.payload["target_action"])
                    self.assertNotIn(
                        (action.payload["site"], target.kind),
                        edenfs_sites.UNOBSERVABLE_PAIRS,
                    )

    def test_lookup_never_composes_contended(self) -> None:
        """Lookup parks are generated without a competing path owner."""
        for seed in range(1000):
            signature = composed_shape_signature(
                compose_sampled(EDENFS_TARGET, seed), EDENFS_TARGET
            )
            self.assertFalse(signature[0] == LOOKUP_SITE and signature[4])

    def test_inodemap_load_specs_unload_before_install(self) -> None:
        """Every generated inodemap-load install follows a covering unload."""
        for seed in range(1000):
            behavior = "block" if seed % 2 else "error"
            kill = ("none", "process")[seed % 2]
            spec = compose_sampled(
                EDENFS_TARGET,
                seed,
                stratum=(INODEMAP_LOAD_SITE, behavior, kill),
            )
            install = spec.action("n0")
            target = spec.action(install.payload["target_action"])
            unloads = [a for a in spec.actions if a.kind == "unload_inodes"]
            self.assertEqual(1, len(unloads))
            (unload,) = unloads
            self.assertIn((unload.id, FINISHED), install.requires)
            parent = str(PurePosixPath(target.payload["path"]).parent)
            self.assertEqual(parent, unload.payload["path"])

    def test_every_nemesis_targets_another_actor(self) -> None:
        for seed in range(500):
            spec = compose_sampled(EDENFS_TARGET, seed)
            owners = action_owners(spec)
            for action in spec.actions:
                target_id = action.payload.get("target_action")
                if target_id is not None:
                    self.assertNotEqual(owners[action.id], owners[target_id])

    def test_observations_have_pre_during_and_post_ordering(self) -> None:
        placements: set[str] = set()
        for seed in range(500):
            spec = compose_sampled(EDENFS_TARGET, seed)
            owners = action_owners(spec)
            install = spec.action("n0")
            observation = spec.action("o0")
            terminal = {"kill": "n0", "block": "n2"}.get(
                install.payload["behavior"], "n1"
            )
            if (observation.id, "finished") in install.requires:
                placements.add("pre")
            elif owners[observation.id] == owners[install.id]:
                placements.add("during")
                order = next(
                    actor.actions
                    for actor in spec.actors
                    if actor.id == owners[install.id]
                )
                self.assertLess(order.index("n0"), order.index("o0"))
                if terminal != "n0":
                    self.assertLess(order.index("o0"), order.index(terminal))
                self.assertIn(("n0", "finished"), observation.requires)
            else:
                placements.add("post")
                self.assertIn((terminal, "finished"), observation.requires)
        self.assertEqual({"pre", "during", "post"}, placements)

    def test_forced_attempts_use_prearmed_safe_post_observation_ordering(self) -> None:
        for site_name, site in generation_target(EDENFS_TARGET).sites.items():
            for behavior in site.behaviors:
                for kill_mode in ("none", "process"):
                    with self.subTest(
                        site=site_name, behavior=behavior, kill_mode=kill_mode
                    ):
                        spec = compose_sampled(
                            EDENFS_TARGET,
                            41,
                            stratum=(site_name, behavior, kill_mode),
                            forced=True,
                        )
                        install = spec.action("n0")
                        target = spec.action(install.payload["target_action"])
                        self.assertIn((install.id, "finished"), target.requires)
                        self.assertNotIn((target.id, "started"), install.requires)
                        terminal = {"kill": "n0", "block": "n2"}.get(behavior, "n1")
                        observation = spec.action("o0")
                        self.assertIn((terminal, "finished"), observation.requires)
                        owners = action_owners(spec)
                        self.assertNotEqual(owners[observation.id], owners[install.id])
                        for action in spec.actions:
                            if action.id.startswith("i") and action.id != target.id:
                                self.assertIn((terminal, "finished"), action.requires)
                        self.assertEqual(
                            (site_name, behavior, kill_mode),
                            composed_shape_signature(spec, EDENFS_TARGET)[:3],
                        )
                        validate(spec, EDENFS_TARGET)


# Kinds a checkout walk must not overlap.
_NAMESPACE_KINDS = ("unlink", "rename", "sl_rm", "sl_mv")


def _namespace_sampler(
    kind: str,
) -> Callable[[random.Random, Mapping[str, Any]], Mapping[str, Any]]:
    def sample(rng: random.Random, context: Mapping[str, Any]) -> Mapping[str, Any]:
        source = rng.choice(REGISTERS)
        if kind in ("rename", "sl_mv"):
            return {
                "source": source,
                "destination": f"tree/registers/moved-{context['action_id']}",
            }
        if kind == "mkdir":
            return {"path": f"tree/registers/dir-{context['action_id']}"}
        return {"path": source}

    return sample


def _with_namespace_samplers(target: Target, guarded: bool = True) -> Target:
    """Return the recording target with namespace samplers, optionally unguarded."""
    actions = {
        **target.actions,
        **{
            kind: dataclasses.replace(
                target.actions[kind], sample=_namespace_sampler(kind)
            )
            for kind in _NAMESPACE_KINDS
        },
    }
    generation = target.generation
    if not guarded:
        generation = dataclasses.replace(generation, forbid_overlap=None)
    return dataclasses.replace(target, actions=actions, generation=generation)


def _forced_namespace_overlaps(spec: RunSpec) -> list[tuple[str, str]]:
    return [
        (first, second)
        for first, second in may_overlap(spec)
        if forbid_overlap(spec, spec.action(first), spec.action(second))
    ]


def _namespace(action_id: str, kind: str) -> Action:
    return Action(
        action_id,
        kind,
        "operation",
        payload=_namespace_sampler(kind)(random.Random(0), {"action_id": action_id}),
    )


class ForcedCheckoutNamespaceGuardTest(unittest.TestCase):
    """A checkout, forced or not, overlapping an unlink deadlocks the mount."""

    def test_forced_checkout_and_remove_or_transfer_are_ordered(self) -> None:
        for kind, mode in [
            (k, m) for k in _NAMESPACE_KINDS for m in ("force", "normal")
        ]:
            with self.subTest(kind=kind, mode=mode):
                checkout = checkout_action("c0", 0, mode)
                spec = actor_spec({"a0": [checkout], "a1": [_namespace("u0", kind)]})
                self.assertEqual([("c0", "u0")], _forced_namespace_overlaps(spec))
                ordered = order_forbidden_overlaps(spec, RECORDING_TARGET)
                self.assertEqual([], _forced_namespace_overlaps(ordered))
                self.assertIn(("c0", FINISHED), ordered.action("u0").requires)
                self.assertEqual(spec.action("c0"), ordered.action("c0"))
                validate(ordered, RECORDING_TARGET)

    def test_a_namespace_op_that_started_first_is_waited_for(self) -> None:
        checkout = dataclasses.replace(
            checkout_action("c0", 0), requires=(("u0", STARTED),)
        )
        spec = actor_spec({"a0": [checkout], "a1": [_namespace("u0", "unlink")]})
        ordered = order_forbidden_overlaps(spec, RECORDING_TARGET)
        self.assertEqual(
            (("u0", STARTED), ("u0", FINISHED)), ordered.action("c0").requires
        )
        self.assertEqual((), ordered.action("u0").requires)
        validate(ordered, RECORDING_TARGET)

    def test_other_overlaps_are_left_alone(self) -> None:
        write = Action(
            "w0",
            "write",
            "operation",
            payload={"path": REGISTERS[0], "value_id": "w0"},
        )
        for name, actors in (
            ("force-write", {"a0": [checkout_action("c0", 0)], "a1": [write]}),
            (
                "force-force",
                {"a0": [checkout_action("c0", 0)], "a1": [checkout_action("c1", 1)]},
            ),
        ):
            with self.subTest(name=name):
                spec = actor_spec(actors)
                self.assertEqual(spec, order_forbidden_overlaps(spec, RECORDING_TARGET))
                self.assertEqual(1, len(may_overlap(spec)))

    def test_a_started_edge_path_orders_the_pair(self) -> None:
        """`u0` starts after `x0` starts, and `x0` waits for `c0` to finish."""
        gate = Action(
            "x0",
            "read",
            "operation",
            requires=(("c0", FINISHED),),
            payload={"path": REGISTERS[1], "channel": "fuse"},
        )
        unlink = dataclasses.replace(
            _namespace("u0", "unlink"), requires=(("x0", STARTED),)
        )
        spec = actor_spec(
            {"a0": [checkout_action("c0", 0)], "a1": [gate], "a2": [unlink]}
        )
        self.assertIn("c0", must_end_before(spec)["u0"])
        self.assertEqual([], _forced_namespace_overlaps(spec))
        self.assertEqual(spec, order_forbidden_overlaps(spec, RECORDING_TARGET))
        validate(spec, RECORDING_TARGET)

    def test_validation_rejects_an_unordered_forced_walk(self) -> None:
        """Hand-written, replayed, or shrunk specs cannot reintroduce the overlap."""
        for kind in _NAMESPACE_KINDS:
            with self.subTest(kind=kind):
                spec = actor_spec(
                    {"a0": [checkout_action("c0", 0)], "a1": [_namespace("u0", kind)]}
                )
                with self.assertRaisesRegex(SpecError, "T293953113"):
                    validate(spec, RECORDING_TARGET)
                declared = dataclasses.replace(
                    spec, metadata={"task": WALK_DEADLOCK_TASK}
                )
                validate(declared, RECORDING_TARGET)
                validate(
                    order_forbidden_overlaps(spec, RECORDING_TARGET), RECORDING_TARGET
                )
                shrunk = actor_spec(
                    {"a0": [checkout_action("c0", 0), _namespace("u0", kind)]}
                )
                validate(shrunk, RECORDING_TARGET)

    def test_a_mkdir_may_overlap_a_checkout_walk(self) -> None:
        """A mkdir inside the walk waits for the checkout and completes; it is not ordered."""
        for mode in ("force", "normal"):
            with self.subTest(mode=mode):
                validate(
                    actor_spec(
                        {
                            "a0": [checkout_action("c0", 0, mode=mode)],
                            "a1": [_namespace("u0", "mkdir")],
                        }
                    ),
                    RECORDING_TARGET,
                )

    def test_composition_never_overlaps_a_forced_checkout_with_namespace_ops(
        self,
    ) -> None:
        guarded = _with_namespace_samplers(RECORDING_TARGET)
        unguarded = _with_namespace_samplers(RECORDING_TARGET, guarded=False)
        both = rejected = 0
        for seed in range(400):
            for shape in (campaign_specs, generate_specs):
                (spec,) = shape(range(seed, seed + 1), guarded)
                kinds = {action.kind for action in spec.actions}
                both += "checkout" in kinds and bool(kinds & set(_NAMESPACE_KINDS))
                self.assertEqual([], _forced_namespace_overlaps(spec), seed)
                try:
                    (raw,) = shape(range(seed, seed + 1), unguarded)
                except SpecError as error:
                    self.assertIn("T293953113", str(error))
                    rejected += 1
                else:
                    self.assertEqual([], _forced_namespace_overlaps(raw), seed)
        self.assertGreater(both, 100)
        # Without the composer's ordering, validation rejects the overlaps.
        self.assertGreater(rejected, 20)


class NamespaceSamplerTest(unittest.TestCase):
    """The EdenFS samplers compose rename, unlink, mkdir, sl rm, and sl mv."""

    SEEDS = range(600)

    def test_namespace_kinds_reach_their_interaction_families(self) -> None:
        kinds: Counter[str] = Counter()
        specs = []
        for shape in (campaign_specs, generate_specs):
            for spec in shape(self.SEEDS):
                validate(spec, RECORDING_TARGET)
                self.assertEqual([], _forced_namespace_overlaps(spec))
                kinds.update(action.kind for action in spec.actions)
                specs.append(spec)
        summary = summarize_generation(specs, RECORDING_TARGET)
        for kind in (*_NAMESPACE_KINDS, "mkdir"):
            self.assertGreater(kinds[kind], 10, (kind, dict(kinds)))
        for family in (
            "family:observe~transfer",
            "family:observe~remove",
            "family:produce~transfer",
            "family:produce~remove",
            "family:remove~transfer",
        ):
            self.assertGreater(summary[family], 0, (family, summary))

    def test_samplers_name_registers_and_new_paths_beside_them(self) -> None:
        for spec in generate_specs(range(200)):
            registers = spec.environment.target["repository"]["registers"]
            for action in spec.actions:
                payload = action.payload
                if action.kind in ("unlink", "sl_rm"):
                    self.assertIn(payload["path"], registers)
                elif action.kind == "mkdir":
                    self.assertTrue(payload["path"].startswith("tree/registers/dir-"))
                elif action.kind in ("rename", "sl_mv"):
                    self.assertIn(payload["source"], registers)
                    self.assertNotEqual(payload["source"], payload["destination"])
                    if action.kind == "sl_mv":
                        self.assertNotIn(payload["destination"], registers)
