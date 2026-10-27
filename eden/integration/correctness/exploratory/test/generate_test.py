# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Target samplers, core generator helpers, and composition: every sampled
input validates, composition is deterministic and validates for both
targets without retries, and planned strata, forced attempts, and the
no-fault baseline compose as declared.
"""

from __future__ import annotations

import dataclasses
import random
import unittest

from eden.integration.correctness.exploratory.core.actions import (
    compose_action_defs,
    CORE_ACTION_DEFS,
)
from eden.integration.correctness.exploratory.core.generate import (
    _producer_stratum,
    compose,
    derived_rng,
    FAULT_REPEATED_PRODUCERS,
    FAULT_REPEATED_SHARE,
    FRESH_PRODUCERS,
    generation_target,
    held_inputs,
    PRODUCER_STRATUM,
    REPEATED_PRODUCER_RATE,
    REPEATED_PRODUCERS,
    shape_signature as composed_shape_signature,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    Environment,
    RunSpec,
    SpecError,
    to_json,
    validate,
)
from eden.integration.correctness.exploratory.core.target import GenerationOptions
from eden.integration.correctness.exploratory.edenfs import (
    actions as edenfs_actions,
    sites as edenfs_sites,
)
from eden.integration.correctness.exploratory.edenfs.sites import (
    CHECKOUT_BEGIN_SITE,
    LOOKUP_SITE,
    registration_key,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    RECORDING_TARGET,
    sample_environment as sample_edenfs_environment,
)
from eden.integration.correctness.exploratory.fake.target import (
    FAKE,
    sample_environment as sample_fake_environment,
)
from eden.integration.correctness.exploratory.test.generation_fixtures import (
    action_owners,
    compose_sampled,
    edenfs_checkout_spec,
)


class TargetSamplerTest(unittest.TestCase):
    def test_target_environment_samples_are_owned_and_memory_limit_is_calibrated(
        self,
    ) -> None:
        samples = [
            sample_edenfs_environment(random.Random(seed)) for seed in range(500)
        ]
        sampled = samples[7]
        self.assertEqual(set(RECORDING_TARGET.environment_fields), set(sampled))
        self.assertEqual(
            {None, 1024, 1280, 1536},
            {sample["setup_profile"]["memory_max_mb"] for sample in samples},
        )
        for limit, multiplier in zip((1024, 1280, 1536), (2, 3, 4)):
            self.assertEqual(0, limit % 256)
            self.assertGreaterEqual(
                limit * 1024 * 1024,
                multiplier * 362_426_368,
            )
        clean = sum(sample["setup_profile"]["scatter_dirty"] == 0 for sample in samples)
        self.assertTrue(200 <= clean <= 300, clean)
        self.assertIn("warm_readdir", sampled["setup_profile"])
        self.assertIn("aged_overlay", sampled["setup_profile"])
        self.assertEqual({}, sample_fake_environment(random.Random(7)))

    def test_positive_action_samples_pass_their_validators(self) -> None:
        spec = edenfs_checkout_spec()
        context = {
            "environment": spec.environment.target,
            "action_id": "sample",
            "forced_kind": True,
        }
        for kind, action_def in RECORDING_TARGET.actions.items():
            if kind == "fault_install":
                continue
            payload = action_def.sample(random.Random(41), context)
            if payload is None:
                continue
            with self.subTest(target="edenfs", kind=kind):
                action_def.validate(
                    spec, Action("sample", kind, "operation", payload=payload)
                )

        fake_spec = dataclasses.replace(
            spec,
            target="fake",
            environment=Environment(target={}),
            actors=(Actor("a", ("sample",)),),
        )
        for kind, action_def in FAKE.actions.items():
            payload = action_def.sample(
                random.Random(41),
                {"action_id": "sample", "forced_kind": True},
            )
            if payload is None:
                continue
            with self.subTest(target="fake", kind=kind):
                action_def.validate(
                    fake_spec, Action("sample", kind, "operation", payload=payload)
                )

    def test_fault_sampler_replaces_only_sample_metadata(self) -> None:
        core = CORE_ACTION_DEFS["fault_install"]
        target = RECORDING_TARGET.actions["fault_install"]
        # The target wraps every validator with its whole-run checks.
        for field in dataclasses.fields(core):
            if field.name not in ("sample", "validate"):
                self.assertEqual(
                    getattr(core, field.name),
                    getattr(target, field.name),
                    field.name,
                )
        self.assertIsNot(core.sample, target.sample)

    def test_target_action_composition_still_rejects_duplicates(self) -> None:
        with self.assertRaisesRegex(SpecError, "action kind collision"):
            compose_action_defs(
                RECORDING_TARGET.actions, {"write": RECORDING_TARGET.actions["write"]}
            )

    def test_fault_sampler_covers_tree_checkout_keys_and_ordinals(self) -> None:
        spec = edenfs_checkout_spec()
        sample = RECORDING_TARGET.actions["fault_install"].sample(
            random.Random(7),
            {
                "site": "tree-checkout",
                "target_action": spec.action("checkout").id,
                "behavior": "block",
            },
        )
        assert sample is not None
        self.assertEqual("tree/a1", sample["key"])
        self.assertEqual(1, sample["ordinal"])


class CoreGeneratorHelperTest(unittest.TestCase):
    def test_derived_rng_is_deterministic_and_domain_separated(self) -> None:
        first = derived_rng(6201, "compose")
        second = derived_rng(6201, "compose")
        self.assertEqual(
            [first.randbytes(32) for _ in range(2)],
            [second.randbytes(32) for _ in range(2)],
        )
        self.assertNotEqual(
            derived_rng(6201, "compose").randbytes(32),
            derived_rng(6201, "campaign").randbytes(32),
        )

    def test_shape_signature_records_all_present_dimensions(self) -> None:
        actions = (
            Action(
                "i0",
                "checkout",
                "run",
                requires=(("n0", "finished"),),
                payload={"revision": 1, "via": "goto", "mode": "normal"},
            ),
            Action(
                "n0",
                "fault_install",
                "operation",
                payload={
                    "fault_id": "F",
                    "site": CHECKOUT_BEGIN_SITE,
                    "target_action": "i0",
                    "behavior": "block",
                },
            ),
            Action("o0", "blocked_faults", "operation"),
            Action(
                "n1",
                "fault_await",
                "operation",
                requires=(("i0", "started"),),
                payload={"fault_id": "F"},
            ),
            Action(
                "n2",
                "fault_remove",
                "operation",
                requires=(("n1", "finished"),),
                payload={"fault_id": "F"},
            ),
            Action(
                "p0",
                "status",
                "operation",
                requires=(("n2", "finished"),),
                payload={"channel": "sl"},
            ),
            Action(
                "k0",
                "kill_process",
                "operation",
                requires=(("p0", "started"),),
                payload={"target_action": "p0", "after_ms": 0},
            ),
            Action(
                "w0",
                "write",
                "operation",
                payload={"path": "tree/registers/r0.reg", "value_id": "a2-w0"},
            ),
            Action(
                "w1",
                "write",
                "operation",
                payload={"path": "tree/registers/r0.reg", "value_id": "a3-w1"},
            ),
        )
        spec = RunSpec(
            seed=6201,
            target="edenfs",
            environment=edenfs_checkout_spec().environment,
            budgets=Budgets(1, 1, 1),
            actors=(
                Actor("a0", ("n0", "o0", "n1", "n2", "k0")),
                Actor("a1", ("i0", "p0")),
                Actor("a2", ("w0",)),
                Actor("a3", ("w1",)),
            ),
            actions=actions,
        )
        validate(spec, RECORDING_TARGET)
        self.assertEqual(
            (
                CHECKOUT_BEGIN_SITE,
                "block",
                "process",
                4,
                True,
                (
                    ("a1", 0, "i0"),
                    ("a1", 1, "p0"),
                    ("a2", 0, "w0"),
                    ("a3", 0, "w1"),
                ),
                (("a0", 1, "o0"),),
            ),
            composed_shape_signature(spec, RECORDING_TARGET),
        )

    def test_shape_signature_records_absent_optional_dimensions(self) -> None:
        spec = edenfs_checkout_spec()
        validate(spec, RECORDING_TARGET)
        self.assertEqual(
            (None, None, "none", 1, False, (("a", 0, "checkout"),), ()),
            composed_shape_signature(spec, RECORDING_TARGET),
        )


class ComposerTest(unittest.TestCase):
    def test_background_load_and_setup_knobs_are_sampled(self) -> None:
        """Each generated load dimension appears in at least twenty percent."""
        background = 0
        warm = 0
        aged = 0
        memory = 0
        for seed in range(500):
            environment = sample_edenfs_environment(random.Random(seed))
            warm += environment["setup_profile"]["warm_readdir"]
            aged += environment["setup_profile"]["aged_overlay"] > 0
            memory += environment["setup_profile"]["memory_max_mb"] is not None
            spec = compose(RECORDING_TARGET, seed, environment)
            background += any(
                action.kind == "background_read" for action in spec.actions
            )
        for count in (background, warm, aged, memory):
            self.assertGreaterEqual(count, 100)

    def test_wide_dir_override_applied_when_stream_placed(self) -> None:
        """Decorated specs get the published wide directory; others keep their profile."""
        (held,) = held_inputs(RECORDING_TARGET)
        decorated = 0
        for seed in range(300):
            environment = sample_edenfs_environment(random.Random(seed))
            spec = compose(RECORDING_TARGET, seed, environment)
            profile = spec.environment.target["setup_profile"]
            if any(a.kind == held.stream for a in spec.actions):
                decorated += 1
                self.assertEqual(held.setup_profile["wide_dir"], profile["wide_dir"])
            else:
                self.assertNotIn("wide_dir", profile)
        self.assertGreater(decorated, 0)

    def test_lookup_generation_disables_setup_warming(self) -> None:
        """Generated lookup targets preserve the fault witness for the target read."""
        for forced in (False, True):
            for seed in range(50):
                environment = sample_edenfs_environment(random.Random(seed))
                environment["setup_profile"]["warm_readdir"] = True
                environment["setup_profile"]["aged_overlay"] = 3
                spec = compose(
                    RECORDING_TARGET,
                    seed,
                    environment,
                    stratum=(LOOKUP_SITE, "block", "none"),
                    forced=forced,
                )
                self.assertFalse(
                    spec.environment.target["setup_profile"]["warm_readdir"]
                )
                self.assertEqual(
                    0, spec.environment.target["setup_profile"]["aged_overlay"]
                )

    def test_no_lookup_generation_uses_aged_overlay(self) -> None:
        """Generated lookup parks require cold registers for their FUSE target."""
        lookup_with_aging = 0
        for seed in range(1000):
            spec = compose_sampled(RECORDING_TARGET, seed)
            install = next((a for a in spec.actions if a.kind == "fault_install"), None)
            if install is None or install.payload["site"] != LOOKUP_SITE:
                continue
            lookup_with_aging += (
                spec.environment.target["setup_profile"]["aged_overlay"] > 0
            )
            self.assertFalse(spec.environment.target["setup_profile"]["warm_readdir"])
        self.assertEqual(0, lookup_with_aging)

    def test_paired_read_observations_share_path_and_placement(self) -> None:
        """A read pair is one placement with both reads ordered before a pre-install."""
        placements = set()
        journals = 0
        for seed in range(500):
            spec = compose_sampled(RECORDING_TARGET, seed)
            first = spec.action("o0")
            if first.kind == "journal_window":
                self.assertEqual(0, first.payload["since_sequence"])
                journals += 1
            if first.kind != "read":
                continue
            second = spec.action("o1")
            self.assertEqual(
                ("read", "fuse", "thrift"),
                (second.kind, first.payload["channel"], second.payload["channel"]),
            )
            self.assertEqual(first.payload["path"], second.payload["path"])
            self.assertGreaterEqual(second.payload["sync_timeout_seconds"], 0)
            owner = next(a for a in spec.actors if first.id in a.actions)
            self.assertEqual(
                owner.actions.index(first.id) + 1, owner.actions.index(second.id)
            )
            install = spec.action("n0")
            if (first.id, "finished") in install.requires:
                self.assertIn((second.id, "finished"), install.requires)
                placements.add("pre")
            elif owner.id == action_owners(spec)[install.id]:
                placements.add("during")
            else:
                placements.add("post")
            validate(spec, RECORDING_TARGET)
        self.assertEqual({"pre", "during", "post"}, placements)
        self.assertGreater(journals, 0)

    def test_no_fault_composition_has_inputs_and_no_nemeses(self) -> None:
        """`no_fault` specs validate and carry no fault lifecycle or other nemesis."""
        for seed in range(500):
            environment = sample_edenfs_environment(random.Random(seed))
            spec = compose(RECORDING_TARGET, seed, environment, no_fault=True)
            validate(spec, RECORDING_TARGET)
            kinds = {action.kind for action in spec.actions}
            self.assertFalse(
                kinds & {"fault_install", "fault_await", "fault_remove"}, seed
            )
            self.assertFalse(
                any(RECORDING_TARGET.actions[k].axis == "nemesis" for k in kinds), seed
            )
            self.assertTrue(
                any(RECORDING_TARGET.actions[k].axis == "input" for k in kinds), seed
            )
            self.assertEqual(
                (None, None, "none"),
                tuple(composed_shape_signature(spec, RECORDING_TARGET)[:3]),
            )
        with self.assertRaises(ValueError):
            compose(
                RECORDING_TARGET,
                0,
                sample_edenfs_environment(random.Random(0)),
                stratum=(LOOKUP_SITE, "error", "none"),
                no_fault=True,
            )

    def test_1000_seeds_validate_for_both_targets_without_retries(self) -> None:
        for target in (RECORDING_TARGET, FAKE):
            for seed in range(1000):
                validate(compose_sampled(target, seed), target)

    def test_baseline_prefers_pathless_kind(self) -> None:
        """Baseline sampling avoids path access when a modeled pathless input exists."""
        for seed in range(50):
            spec = compose_sampled(RECORDING_TARGET, seed)
            baseline = spec.action("b0")
            self.assertEqual("status", baseline.kind)
            self.assertNotIn("path", baseline.payload)

    def test_output_is_deterministic(self) -> None:
        for target in (RECORDING_TARGET, FAKE):
            for seed in (0, 1, 6201, 99991):
                self.assertEqual(
                    to_json(compose_sampled(target, seed)),
                    to_json(compose_sampled(target, seed)),
                )

    def test_unforced_strata_of_generation_sites_validate_at_one_seed(self) -> None:
        for site_name, site in generation_target(RECORDING_TARGET).sites.items():
            for behavior in site.behaviors:
                for kill_mode in ("none", "process"):
                    with self.subTest(
                        site=site_name, behavior=behavior, kill_mode=kill_mode
                    ):
                        spec = compose_sampled(
                            RECORDING_TARGET,
                            41,
                            stratum=(site_name, behavior, kill_mode),
                        )
                        self.assertEqual(
                            (site_name, behavior, kill_mode),
                            composed_shape_signature(spec, RECORDING_TARGET)[:3],
                        )
                        validate(spec, RECORDING_TARGET)

    def test_forced_attempts_apply_target_owned_setup_and_binding(self) -> None:
        environment = dict(sample_edenfs_environment(random.Random(0)))
        environment["setup_profile"] = {
            "materialize": True,
            "scatter_dirty": environment["repository"]["file_count"],
        }

        checkout = compose(
            RECORDING_TARGET,
            41,
            environment,
            stratum=(CHECKOUT_BEGIN_SITE, "block", "none"),
            forced=True,
        )
        self.assertEqual(
            0, checkout.environment.target["setup_profile"]["scatter_dirty"]
        )

        lookup = compose(
            RECORDING_TARGET,
            41,
            environment,
            stratum=(LOOKUP_SITE, "block", "none"),
            forced=True,
        )
        lookup_install = lookup.action("n0")
        lookup_target = lookup.action(lookup_install.payload["target_action"])
        self.assertEqual("read", lookup_target.kind)
        self.assertEqual("fuse", lookup_target.payload["channel"])
        self.assertEqual(
            (
                "FuseDispatcherImpl::lookup",
                lookup_target.payload["path"].rsplit("/", 1)[-1],
            ),
            registration_key(lookup, lookup_install),
        )
        self.assertEqual(
            {
                "aged_overlay": 0,
                "materialize": False,
                "scatter_dirty": 0,
                "warm_readdir": False,
            },
            lookup.environment.target["setup_profile"],
        )

    def test_planned_strata_restore_generation_dropped_sites(self) -> None:
        """A planned stratum keeps its drawn site without swapping it."""
        for site_name, site in RECORDING_TARGET.sites.items():
            if edenfs_sites.observable_target_kinds(site_name, site):
                continue
            for behavior in sorted(site.behaviors):
                with self.subTest(site=site_name, behavior=behavior):
                    spec = compose_sampled(
                        RECORDING_TARGET,
                        41,
                        stratum=(site_name, behavior, "none"),
                        forced=True,
                    )
                    self.assertEqual(
                        (site_name, behavior, "none"),
                        composed_shape_signature(spec, RECORDING_TARGET)[:3],
                    )
                    validate(spec, RECORDING_TARGET)
        with self.assertRaisesRegex(ValueError, "is not admitted"):
            compose_sampled(
                RECORDING_TARGET,
                531,
                stratum=("no-such-site", "block", "none"),
                forced=True,
            )

    def test_site_validator_rejection_is_not_retried(self) -> None:
        site_name = sorted(RECORDING_TARGET.sites)[0]
        site = RECORDING_TARGET.sites[site_name]
        calls = 0

        def reject(*_args) -> None:
            nonlocal calls
            calls += 1
            raise SpecError("site validator rejected generated candidate")

        rejected = dataclasses.replace(
            RECORDING_TARGET,
            sites={
                **RECORDING_TARGET.sites,
                site_name: dataclasses.replace(site, validate=reject),
            },
        )
        with self.assertRaisesRegex(SpecError, "site validator rejected"):
            compose_sampled(
                rejected,
                41,
                stratum=(site_name, sorted(site.behaviors)[0], "none"),
            )
        self.assertEqual(1, calls)

    def test_no_sites_target_is_valid_and_fault_free(self) -> None:
        for seed in range(50):
            spec = compose_sampled(FAKE, seed)
            validate(spec, FAKE)
            self.assertEqual(
                (None, None, "none"),
                composed_shape_signature(spec, FAKE)[:3],
            )
            self.assertNotIn(
                "nemesis", {FAKE.actions[action.kind].axis for action in spec.actions}
            )

    def test_generation_target_preserves_handwritten_site_table(self) -> None:
        """Filtering composition leaves the target's full validator table intact."""
        generated = generation_target(RECORDING_TARGET)
        self.assertEqual(9, len(RECORDING_TARGET.sites))
        self.assertEqual(
            {
                CHECKOUT_BEGIN_SITE,
                LOOKUP_SITE,
                "overlay-write",
                "inodemap-load",
                "journal-record",
            },
            set(generated.sites),
        )
        self.assertEqual(frozenset({"read"}), generated.sites[LOOKUP_SITE].target_kinds)
        self.assertEqual(
            frozenset({"read"}), RECORDING_TARGET.sites[LOOKUP_SITE].target_kinds
        )


class NoFaultBaselineTest(unittest.TestCase):
    def test_1000_seeds_always_include_modeled_baseline_input(self) -> None:
        """No-fault composition supplies modeled evidence before random inputs."""
        for target, sample_environment in (
            (RECORDING_TARGET, sample_edenfs_environment),
            (FAKE, sample_fake_environment),
        ):
            for seed in range(1000):
                spec = compose(
                    target, seed, sample_environment(random.Random(seed)), no_fault=True
                )
                modeled = [
                    action
                    for action in spec.actions
                    if target.actions[action.kind].axis == "input"
                    and target.actions[action.kind].modeled
                ]
                self.assertGreaterEqual(len(modeled), 1, f"{target.name} seed {seed}")
                self.assertIn(
                    "b0",
                    [action.id for action in modeled],
                    f"{target.name} seed {seed}",
                )
                self.assertEqual("b0", spec.actors[0].actions[0])
                validate(spec, target)


class GenerationContractTest(unittest.TestCase):
    def test_generation_options_are_a_typed_target_field(self) -> None:
        """A target publishes its composition options as typed fields; a misspelled one is refused."""
        generation = RECORDING_TARGET.generation
        self.assertIs(edenfs_actions.forbid_overlap, generation.forbid_overlap)
        self.assertEqual(edenfs_actions.HELD_INPUTS, held_inputs(RECORDING_TARGET))
        self.assertEqual(frozenset({LOOKUP_SITE}), generation.uncontended_sites)
        self.assertEqual(GenerationOptions(), FAKE.generation)
        with self.assertRaises(TypeError):
            # pyrefly: ignore[unexpected-keyword]
            GenerationOptions(**{"forbid_overlaps": edenfs_actions.forbid_overlap})

    def test_compose_refuses_a_stratum_composition_does_not_admit(self) -> None:
        site_name = sorted(RECORDING_TARGET.sites)[0]
        behavior = sorted(RECORDING_TARGET.sites[site_name].behaviors)[0]
        for stratum in (
            (site_name, "no-such-behavior", "none"),
            (site_name, behavior, "sigkill"),
            ("no-such-site", behavior, "none"),
        ):
            with self.subTest(stratum=stratum):
                with self.assertRaisesRegex(ValueError, "is not admitted"):
                    compose_sampled(RECORDING_TARGET, 41, stratum=stratum)

    def test_a_planned_fault_window_tail_is_composed_or_refused(self) -> None:
        """A requested fault-window producer tail is never silently downgraded."""
        outcomes = set()
        for seed in range(12):
            try:
                spec = compose_sampled(
                    RECORDING_TARGET, seed, producers=FAULT_REPEATED_PRODUCERS
                )
            except ValueError as error:
                self.assertIn("declines a fault-window tail", str(error))
                outcomes.add("refused")
                continue
            self.assertEqual(FAULT_REPEATED_PRODUCERS, spec.metadata[PRODUCER_STRATUM])
            outcomes.add("composed")
        self.assertEqual({"composed", "refused"}, outcomes)

    def test_unplanned_producer_strata_follow_the_draw(self) -> None:
        """Below the rate a spec with a fault draws the fault-window tail for its share."""
        for seed in range(400):
            draw = derived_rng(seed, "exploratory-producers").random()
            expected = FRESH_PRODUCERS
            if draw < REPEATED_PRODUCER_RATE * FAULT_REPEATED_SHARE:
                expected = FAULT_REPEATED_PRODUCERS
            elif draw < REPEATED_PRODUCER_RATE:
                expected = REPEATED_PRODUCERS
            self.assertEqual(
                expected, _producer_stratum(RECORDING_TARGET, seed, None, True)
            )
