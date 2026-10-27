# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Grammar and expectation-shape gates of the expected-results corpus.

These tests check that every corpus history is a valid recording under the
real EdenFS vocabulary and recording target (history grammar, strict
result decoding, per-channel result shapes, effect contracts), and that
every declared expectation is well-formed and internally consistent
(catalog rules and reasons, identity roles, canonical error-set keys,
verdict projection, task-key twin groups). The catalog agrees with every
registered rule's identity, subject kind, and red controls.
Shape gates keep a case from being emptied into an accept: a declared
satisfied (legal) or violated (illegal) target obligation per case, red
controls that trip their rules, and a named gap for any rule without both
outcomes.

They do NOT check that any expectation is semantically right:
`corpus_engine_test` compares the checker's obligations with these
expectations through `loader.mismatches`.
"""

from __future__ import annotations

import dataclasses
import json
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from unittest import mock

from eden.integration.correctness.exploratory.core import (
    actions as core_actions,
    checker,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import (
    event_line,
    history_problems,
)
from eden.integration.correctness.exploratory.core.identity import error_set_key
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.core.spec_types import SpecError
from eden.integration.correctness.exploratory.edenfs.actions import (
    channel_shape_problem,
    expects_reference,
)
from eden.integration.correctness.exploratory.edenfs.contracts import make_contracts
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
)
from eden.integration.correctness.exploratory.edenfs.encoding import (
    content_token,
    encode_committed,
    encode_value,
)
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    RECORDING_TARGET,
)
from eden.integration.correctness.exploratory.test.corpus import (
    export,
    families,
    loader,
)
from eden.integration.correctness.exploratory.test.corpus.catalog import (
    ATTRIBUTIONS,
    catalog,
    NOT_EVALUATED_REASONS,
    RULES,
)

COMPLETED = {"status": "completed"}
# Families the corpus requires, each with at least one legal and one
# illegal case (the NotEvaluated family pairs NotEvaluated with a finding).
REQUIRED_FAMILIES = (
    "bytes-status",
    "repeated-producers",
    "rename",
    "checkout",
    "windows",
    "aggregate-omission",
    "differential",
    "not-evaluated",
    "direct",
    "errno-twins",
    "task-key",
    "fd",
    "readdir",
    "journal",
    "cross-channel",
    "manifest",
    "existence",
    "attributes",
)
# Catalog rules with no declared satisfied or violated corpus obligation,
# and why. Adding the missing case means removing the entry.
COVERAGE_GAPS = {
    "core.daemon-exit": "core-owned; a daemon exit needs a run event the corpus "
    "does not record (core tests cover it)",
    "core.activity": "core-owned; every corpus case completes a modeled action "
    "(core tests cover the violation)",
    "core.result-schema": "core-owned; a corpus history must decode strictly, so it "
    "never carries a recorder problem (core tests cover it)",
}
# Cases whose legality rests on EdenFS behavior read from source or inferred
# rather than observed live; each states that in an `INFERRED:` basis entry.
INFERRED_CASES = (
    "checkout/interrupted-checkout-independent-paths",
    "checkout/checkout-rejected-without-overlap",
    "windows/etimedout-write-applied",
)
# Cases whose recorded result deliberately mixes channel field sets
# (`result-shape.channel-fields` violating twins).
SHAPE_VIOLATIONS = frozenset({"direct/getattr-mixed-thrift-fields"})
CONTRACT_TARGET = dataclasses.replace(
    RECORDING_TARGET, contracts=make_contracts(CONTRACT_RULES)
)


class GrammarTest(unittest.TestCase):
    def test_every_history_is_a_valid_recording(self) -> None:
        corpus = loader.cases()
        self.assertGreater(len(corpus), 0)
        for case in corpus:
            with self.subTest(case=case.name):
                spec = case.spec(RECORDING_TARGET)
                events = case.events()
                self.assertEqual(
                    case.history_bytes,
                    b"".join(event_line(event) for event in events),
                )
                self.assertEqual(
                    (), history_problems(spec, events, execution_status="completed")
                )
                facts = HistoryFacts.build(
                    spec, events, RECORDING_TARGET, execution_status="completed"
                )
                for action in facts.actions.values():
                    self.assertIsNotNone(action.outcome, action.id)
                    self.assertEqual((), action.problems, action.id)
                    self.assertEqual(
                        expects_reference(spec, spec.action(action.id))
                        and action.outcome is not None
                        and str(action.outcome) != "info",
                        action.reference is not None,
                        action.id,
                    )
                    if action.reference is not None:
                        self.assertEqual((), action.reference.problems, action.id)
                    assert action.result is not None, action.id
                    problem = channel_shape_problem(
                        spec.action(action.id),
                        str(action.outcome),
                        dict(action.result.fields),
                    )
                    if case.name in SHAPE_VIOLATIONS:
                        self.assertIsNotNone(problem)
                    else:
                        self.assertIsNone(problem)
                derived = checker.contract_effects(
                    CONTRACT_TARGET, spec, facts, CONTRACT_RULES
                )
                self.assertIsInstance(derived, tuple, derived)
                verdict = checker.check(
                    RECORDING_TARGET, spec, events, execution=COMPLETED
                )
                self.assertEqual("not-evaluated", verdict.kind)
                self.assertEqual("no-evaluation", verdict.not_evaluated_cause)

    def test_differential_specs_are_admitted_or_rejected_as_declared(self) -> None:
        specs = families.spec_cases()
        self.assertEqual({True, False}, {case.admitted for case in specs})
        for case in specs:
            with self.subTest(case=case.name):
                text = json.dumps(case.spec)
                if case.admitted:
                    self.assertEqual("", case.error)
                    spec = from_json(text, RECORDING_TARGET)
                    self.assertTrue(
                        all(expects_reference(spec, action) for action in spec.actions)
                    )
                    continue
                self.assertNotEqual("", case.error)
                with self.assertRaises(SpecError) as raised:
                    from_json(text, RECORDING_TARGET)
                self.assertIn(case.error, str(raised.exception))

    def test_two_cases_with_one_name_are_refused(self) -> None:
        (run, *_) = families.runs()
        loader.cases.cache_clear()
        try:
            with mock.patch.object(families, "runs", return_value=[run, run]):
                with self.assertRaisesRegex(loader.CorpusError, "two corpus cases"):
                    loader.cases()
        finally:
            loader.cases.cache_clear()

    def test_export_writes_the_rendered_case(self) -> None:
        case = loader.cases()[0]
        with tempfile.TemporaryDirectory() as temporary:
            export.export(case.name, Path(temporary))
            written = Path(temporary)
            self.assertEqual(case.spec_text, (written / "run-spec.json").read_text())
            self.assertEqual(
                case.history_bytes, (written / "history.jsonl").read_bytes()
            )


class ExpectationShapeTest(unittest.TestCase):
    def test_catalog_agrees_with_registered_code(self) -> None:
        core_rules = {
            rule.id: rule
            for definition in checker.core_properties(RECORDING_TARGET)
            for rule in definition.rules.values()
        }
        target_rules = {
            rule.id: rule
            for definition in EDENFS_TARGET.properties
            for rule in definition.rules.values()
        }
        stated = catalog()["rules"]
        for rule_id, entry in RULES.items():
            with self.subTest(rule=rule_id):
                rule = core_rules.get(rule_id) or target_rules.get(rule_id)
                if rule is None:
                    continue
                self.assertEqual(entry["identity_roles"], list(rule.identity_roles))
                self.assertEqual(entry["subject_kind"], str(rule.subject_kind))
                self.assertEqual(entry["red_controls"], sorted(rule.red_controls))
                self.assertEqual(rule.statement, stated[rule_id]["statement"])
        self.assertIn(checker.CORE_INPUT_STATE_UNOBSERVED, NOT_EVALUATED_REASONS)

    def test_every_registered_rule_is_catalogued(self) -> None:
        """Registry ⊆ catalog, so `test_every_rule_has_declared_outcomes_or_a_named_gap`
        and `test_red_controls_trip_their_rules` reach every registered rule:
        legal and violating cases, and a tripping red control for each it maps."""
        registered = {
            rule.id
            for definition in checker.registry(EDENFS_TARGET)
            for rule in definition.rules.values()
        }
        self.assertEqual(
            set(),
            registered - set(RULES),
            "catalogue each rule in test/corpus/catalog.py with legal and violating "
            "cases",
        )

    def test_catalog_entries_are_self_consistent(self) -> None:
        """Identity roles say how each is computed."""
        for rule_id, entry in RULES.items():
            with self.subTest(rule=rule_id):
                self.assertEqual(set(entry["identity_roles"]), set(entry["identity"]))

    def test_every_case_declares_what_it_asserts(self) -> None:
        """A case cannot be emptied into an accept: semantic-shape gates."""
        for case in loader.cases():
            expectation = case.expectation
            with self.subTest(case=case.name):
                outcomes = {
                    item.outcome
                    for item in expectation.obligations
                    if not item.outcome_in
                }
                if expectation.legal:
                    self.assertIn("satisfied", outcomes)
                else:
                    self.assertIn("violated", outcomes)
                if any(item.outcome_in for item in expectation.obligations):
                    self.assertEqual("reject", expectation.verdict)
                if case.name in INFERRED_CASES:
                    self.assertTrue(
                        any(
                            entry.startswith("INFERRED:") for entry in expectation.basis
                        )
                    )

    def test_fabricated_reads_match_no_candidate(self) -> None:
        """A read is fabricated only when no candidate exists:
        no committed (register, revision) and no write value of the run."""
        checked = 0
        for case in loader.cases():
            anchors = {
                item.anchor
                for item in case.expectation.obligations
                if item.rule_id in ("bytes.fabricated-value", "fd.fabricated-value")
                and item.outcome == "violated"
            }
            if not anchors:
                continue
            spec = case.spec(RECORDING_TARGET)
            repository = spec.environment.target["repository"]
            owner = {
                action_id: actor.id
                for actor in spec.actors
                for action_id in actor.actions
            }
            candidates = {
                content_token(encode_committed(spec.seed, path, revision))
                for path in repository["registers"]
                for revision in range(repository["commit_count"])
            } | {
                content_token(
                    encode_value(
                        spec.seed, owner[action.id], action.payload["value_id"]
                    )
                )
                for action in spec.actions
                if action.kind == "write"
            }
            facts = HistoryFacts.build(
                spec, case.events(), RECORDING_TARGET, execution_status="completed"
            )
            for anchor in sorted(anchors):
                with self.subTest(case=case.name, read=anchor):
                    result = facts.action(anchor).result
                    assert result is not None
                    self.assertNotIn(result.get("sha256"), candidates)
                    checked += 1
        self.assertGreater(checked, 0)

    def test_every_rule_has_declared_outcomes_or_a_named_gap(self) -> None:
        seen: dict[str, set[str]] = {rule_id: set() for rule_id in RULES}
        for case in loader.cases():
            for item in case.expectation.obligations:
                seen[item.rule_id].update(item.allowed)
        missing = {
            rule_id
            for rule_id, outcomes in seen.items()
            if not {"satisfied", "violated"} <= outcomes
        }
        self.assertEqual(set(COVERAGE_GAPS), missing)

    def test_red_controls_trip_their_rules(self) -> None:
        """Every red control a rule maps has a case of its shape violating that rule."""
        tripped: set[tuple[str, str]] = set()
        for case in loader.cases():
            control = case.expectation.red_control
            if control is None:
                continue
            violated = {
                item.rule_id
                for item in case.expectation.obligations
                if item.outcome == "violated"
            }
            owners = {
                rule_id
                for rule_id, entry in RULES.items()
                if control in entry["red_controls"]
            }
            with self.subTest(case=case.name):
                self.assertTrue(owners & violated)
            tripped |= {(rule_id, control) for rule_id in owners & violated}
        self.assertEqual(
            {
                (rule_id, control)
                for rule_id, entry in RULES.items()
                for control in entry["red_controls"]
            },
            tripped,
        )

    def test_every_expectation_is_well_formed(self) -> None:  # noqa: C901
        for case in loader.cases():
            expectation = case.expectation
            with self.subTest(case=case.name):
                spec = case.spec(RECORDING_TARGET)
                metadata = json.loads(case.spec_text)["metadata"]
                self.assertEqual(
                    {"corpus_case": case.name, "origin": "synthetic"},
                    {key: metadata[key] for key in ("corpus_case", "origin")},
                )
                self.assertEqual(COMPLETED, expectation.execution)
                self.assertIn(expectation.family, REQUIRED_FAMILIES)
                self.assertTrue(expectation.basis)
                keys = [item.key for item in expectation.obligations]
                self.assertEqual(sorted(keys), keys)
                self.assertEqual(len(set(keys)), len(keys))
                action_ids = {action.id for action in spec.actions}
                for item in expectation.obligations:
                    rule = RULES[item.rule_id]
                    self.assertIn(item.anchor, action_ids)
                    kind = item.subject.split(":", 1)[0]
                    self.assertEqual(rule["subject_kind"], kind, item.key)
                    if item.rule_id == "core.input-legality":
                        self.assertEqual(
                            checker.input_subject(
                                RECORDING_TARGET, spec, spec.action(item.anchor)
                            ).token,
                            item.subject,
                        )
                    if item.outcome == "violated":
                        self.assertIn(item.attribution, ATTRIBUTIONS)
                        for identity in item.identities:
                            self.assertEqual(set(rule["identity_roles"]), set(identity))
                    if item.outcome == "not-evaluated":
                        self.assertIn(item.not_evaluated_reason, NOT_EVALUATED_REASONS)
                self._check_errors(expectation)

    def _check_errors(self, expectation: loader.Expectation) -> None:
        grouped: dict[str, set[tuple[str, str, tuple[tuple[str, str], ...]]]] = {}
        for item in expectation.obligations:
            if item.outcome == "violated" and item.attribution in (
                "product",
                "undetermined",
            ):
                for identity in item.identities:
                    grouped.setdefault(item.subject, set()).add(
                        (
                            item.rule_id,
                            item.rule_id.rsplit(".", 1)[1],
                            tuple(sorted(identity.items())),
                        )
                    )
        declared = {
            error_set.subject: (set(error_set.errors), error_set.key)
            for error_set in expectation.canonical_errors
        }
        self.assertEqual(
            {
                subject: (errors, error_set_key(errors))
                for subject, errors in grouped.items()
            },
            declared,
        )
        self.assertEqual(not declared, expectation.legal)
        outcomes = [
            (item.outcome, item.attribution) for item in expectation.obligations
        ]
        if declared:
            verdict = "reject"
        elif any(outcome == "violated" for outcome, _ in outcomes):
            verdict = "harness-suspect"
        elif any(outcome == "not-evaluated" for outcome, _ in outcomes):
            verdict = "not-evaluated"
        else:
            verdict = "accept"
        self.assertEqual(verdict, expectation.verdict)

    def test_every_required_family_has_legal_and_illegal_cases(self) -> None:
        legal = Counter(
            case.expectation.family for case in loader.cases() if case.expectation.legal
        )
        illegal = Counter(
            case.expectation.family
            for case in loader.cases()
            if not case.expectation.legal
        )
        for family in REQUIRED_FAMILIES:
            with self.subTest(family=family):
                if family != "not-evaluated":
                    self.assertGreater(legal[family], 0)
                self.assertGreater(illegal[family], 0)

    def test_task_key_twins_share_or_split_their_key(self) -> None:
        by_name = {case.name: case.expectation for case in loader.cases()}
        groups = families.twins()["groups"]
        self.assertTrue(groups)
        for group in groups:
            (base,) = by_name[group["base"]].canonical_errors
            for name in group["same_key"]:
                with self.subTest(twin=name):
                    (error_set,) = by_name[name].canonical_errors
                    self.assertEqual(base.key, error_set.key)
            for name in group["different_key"]:
                with self.subTest(twin=name):
                    (error_set,) = by_name[name].canonical_errors
                    self.assertNotEqual(base.key, error_set.key)

    def test_mismatches_reports_disagreements(self) -> None:
        expectation = loader.case("task-key/stale-read-base").expectation
        observed = [
            loader.ObservedObligation(
                item.rule_id,
                item.anchor,
                item.subject,
                item.outcome,
                item.attribution,
                item.not_evaluated_reason,
                item.identity,
            )
            for item in expectation.obligations
        ]
        registered = set(RULES)
        self.assertEqual(
            (),
            loader.mismatches(expectation, observed, registered=registered),
        )
        flipped = [
            dataclasses.replace(item, outcome="satisfied", attribution=None)
            for item in observed
        ]
        self.assertEqual(
            2,
            len(loader.mismatches(expectation, flipped, registered=registered)),
        )
        self.assertEqual(
            (),
            loader.mismatches(expectation, flipped, registered={"core.input-legality"}),
        )
        for outcome, problems in (("satisfied", 0), ("not-evaluated", 1)):
            undeclared = dataclasses.replace(
                observed[0], anchor="undeclared", outcome=outcome, attribution=None
            )
            with self.subTest(undeclared=outcome):
                self.assertEqual(
                    problems,
                    len(
                        loader.mismatches(
                            expectation, [*observed, undeclared], registered=registered
                        )
                    ),
                )

    def test_mismatches_compares_the_verdict_kind(self) -> None:
        expectation = loader.case("errno-twins/enoent-rename-source-gone").expectation
        satisfied = [
            loader.ObservedObligation(
                item.rule_id, item.anchor, item.subject, "satisfied"
            )
            for item in expectation.obligations
        ]
        registered = {"core.input-legality"}
        self.assertEqual(
            (),
            loader.mismatches(expectation, satisfied, registered=registered),
        )
        self.assertEqual(
            ("verdict: expected accept, observed not-evaluated",),
            loader.mismatches(
                expectation,
                satisfied,
                registered=registered,
                verdict="not-evaluated",
            ),
        )
        self.assertEqual(
            (),
            loader.mismatches(
                expectation,
                satisfied,
                registered=registered,
                verdict="accept",
            ),
        )

    def test_mismatches_accepts_any_outcome_of_an_outcome_set(self) -> None:
        expectation = loader.case(
            "not-evaluated/budget-violation-same-component"
        ).expectation
        undecided = [item for item in expectation.obligations if item.outcome_in]
        self.assertEqual(12, len(undecided))

        def observed(outcome: str) -> list[loader.ObservedObligation]:
            return [
                loader.ObservedObligation(
                    item.rule_id,
                    item.anchor,
                    item.subject,
                    outcome if item.outcome_in else item.outcome,
                    item.attribution,
                    (
                        "core.solver.budget"
                        if item.outcome_in and outcome == "not-evaluated"
                        else item.not_evaluated_reason
                    ),
                    item.identity,
                )
                for item in expectation.obligations
            ]

        registered = set(RULES)
        for outcome in ("satisfied", "not-evaluated"):
            with self.subTest(outcome=outcome):
                self.assertEqual(
                    (),
                    loader.mismatches(
                        expectation,
                        observed(outcome),
                        registered=registered,
                    ),
                )
        self.assertEqual(
            12,
            sum(
                "expected ['satisfied', 'not-evaluated']" in problem
                for problem in loader.mismatches(
                    expectation,
                    observed("violated"),
                    registered=registered,
                )
            ),
        )


class KindCoverageTest(unittest.TestCase):
    """A new action kind needs a corpus case and a sampler, or a named exemption."""

    # Kinds no corpus case records.
    UNRECORDED = frozenset(
        {"background_read", "blocked_faults", "sleep", "status_counts", "unload_inodes"}
    )
    # Kinds the per-action sampler never draws, each with who places it.
    UNSAMPLED = {
        "blocked_faults": "authored only",
        "close": "the held-descriptor plan (`HELD_DESCRIPTOR`)",
        "fault_await": "the composer's fault plan",
        "fault_remove": "the composer's fault plan",
        "kill_process": "the composer's kill plan",
        "read_fd": "the held-descriptor plan (`HELD_DESCRIPTOR`)",
        "readdir_resume": "the held-input plan (`HELD_INPUTS`)",
        "readdir_stream": "the held-input plan (`HELD_INPUTS`)",
        "sleep": "authored only",
        "unload_inodes": "the inodemap-load site's setup (`sites.py`)",
    }

    def test_every_kind_is_recorded_and_sampled_or_named(self) -> None:
        recorded = {
            action.kind
            for case in loader.cases()
            for action in case.spec(RECORDING_TARGET).actions
        }
        kinds = set(EDENFS_TARGET.actions)
        self.assertEqual(self.UNRECORDED, kinds - recorded)
        self.assertEqual(
            set(self.UNSAMPLED),
            {
                kind
                for kind, definition in EDENFS_TARGET.actions.items()
                if definition.sample is core_actions._sample_none
            },
        )
