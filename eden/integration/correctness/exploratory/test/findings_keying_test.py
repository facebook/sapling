# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""How findings key a task: the key is the digest of a canonical error-set document that
ignores action IDs, witnesses, concrete paths, harmless actions, and seeds, and
changes with declared roles; routing separates product, harness, and reference
findings; a subject stays unkeyed while a finding on it is unclassified; corpus
error sets key identically; and the findings report keys error sets and names
every run's replay.
"""

from __future__ import annotations

import dataclasses
import json
import tempfile
import unittest
from collections.abc import Mapping
from pathlib import Path
from typing import Any

from eden.integration.correctness.exploratory import findings
from eden.integration.correctness.exploratory.core.campaign import (
    CampaignReport,
    report_document,
)
from eden.integration.correctness.exploratory.core.checker import (
    check,
    CORE_PROPERTY_IDS,
    registry,
    semantic_digest,
    Verdict,
)
from eden.integration.correctness.exploratory.core.identity import (
    error_set_document,
    error_set_key,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    identity_digest,
    ObligationResult,
    ObligationStatus,
    PropertyDef,
    PropertyResult,
    RuleDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge as judge_edenfs,
)
from eden.integration.correctness.exploratory.fake.target import ACTIONS, FAKE, READ
from eden.integration.correctness.exploratory.findings import (
    keyed,
    read_findings,
    related_keys,
    route_of,
)
from eden.integration.correctness.exploratory.schemas import (
    admission_for,
    document_digest,
    RunPurpose,
    write_admission,
)
from eden.integration.correctness.exploratory.test.corpus import loader
from eden.integration.correctness.exploratory.test.corpus.author import (
    R0,
    Run,
    thrift_checkout,
    thrift_rejected,
    WRITE_OK,
)
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    COMPLETED_EXECUTION,
    findings_of,
    history_of,
    judge,
    keys_of,
    make_spec,
    Ob,
    obligations,
    register_actions,
    script,
    SCRIPTED,
    stale,
    status,
    subjects_of,
    summary_line,
    synthetic_verdict,
    torn,
    write_run,
)


TORN_FUSE = error_set_key([("bytes.torn-read", "torn-read", (("channel", "fuse"),))])


# The pinned canonical key of {bytes.torn-read[channel=fuse]}: any change to
# the key document's encoding changes this and every known task's key.
PINNED_TORN_FUSE = (
    "sha256:6bea56005b9fe4ef6aa6b3bbfe1c30620ad6196edd824b48d6fb49224331b36b"
)


# The pinned identity digest of the EdenFS rule registry. Task keys bind it, so a
# change is a deliberate identity change: update the pin in the same diff.
PINNED_EDENFS_IDENTITY = (
    "sha256:03a6c01c5f9c9effa2733527edcf5168a1bd5d8908081b051d969892898b6380"
)


class CanonicalKeyInvarianceTest(unittest.TestCase):
    """The key depends on the canonical error set and nothing else."""

    BASE = (torn(), status())

    def _key(self, *obligations: Ob, **kwargs: Any) -> list[str]:
        return keys_of(judge(script(*obligations), **kwargs))

    def _fake(self, actions: Mapping[str, Mapping[str, Any]], seed: int = 7) -> Verdict:
        """Judge a real fake-target torn-write run: collectors, engine, and keys."""
        spec = make_spec(target=FAKE, actions=actions, seed=seed)
        events = history_of(spec, FAKE, red_control="torn-write")
        return check(
            FAKE, spec, events, execution={**COMPLETED_EXECUTION, "events": len(events)}
        )

    def test_action_ids_renamed(self) -> None:
        base = self._fake(register_actions(r="register"))
        renamed = self._fake(
            {
                "first-writer": register_actions()["w"],
                "late-reader": register_actions(r="register")["r"],
            }
        )
        self.assertEqual(
            {"r"},
            {
                w.value
                for f in findings_of(base)
                for w in f.witnesses
                if w.role == "read-action"
            },
        )
        self.assertEqual(
            {"late-reader"},
            {
                w.value
                for f in findings_of(renamed)
                for w in f.witnesses
                if w.role == "read-action"
            },
        )
        keys = keys_of(base, FAKE)
        self.assertEqual(1, len(keys))
        self.assertEqual(keys, keys_of(renamed, FAKE))

    def test_witness_changes(self) -> None:
        noisy = (
            torn(evidence={"read-action": "r9", "proof": "splice"}, reason="other"),
            status(evidence={"probe-action": "s7"}, scope="elsewhere"),
        )
        self.assertEqual(self._key(*self.BASE), self._key(*noisy))

    def test_concrete_subject_path(self) -> None:
        moved = (torn("path:tree/r1.reg"), status("path:tree/r1.reg"))
        self.assertEqual(self._key(*self.BASE), self._key(*moved))
        both = judge(script(*self.BASE, *moved))
        self.assertEqual(
            {self._key(*self.BASE)[0]: ["path:r0", "path:tree/r1.reg"]},
            subjects_of(both),
        )

    def test_target_profile_and_digests(self) -> None:
        other = dataclasses.replace(
            SCRIPTED, name="other-target", semantics_profile="other-profile-v9"
        )
        verdict = judge(script(*self.BASE))
        moved = judge(script(*self.BASE), target=other)
        assert verdict.report is not None and moved.report is not None
        self.assertNotEqual(
            verdict.report.bindings.semantics_profile,
            moved.report.bindings.semantics_profile,
        )
        self.assertNotEqual(
            verdict.report.identity_digest, moved.report.identity_digest
        )
        self.assertEqual(keys_of(verdict), keys_of(moved, other))

    def test_redundant_observations(self) -> None:
        redundant = (*self.BASE, torn(scope="again"), torn(scope="third"))
        self.assertEqual(self._key(*self.BASE), self._key(*redundant))

    def test_harmless_independent_actions_and_seed(self) -> None:
        base = self._fake(register_actions(r="register"))
        harmless = self._fake(
            {
                **register_actions(r="register", r2="untouched", r3="untouched"),
                "g": {"kind": "getattr", "deadline": "operation", "path": "register"},
            },
            seed=99,
        )
        self.assertLess(len(list(obligations(base))), len(list(obligations(harmless))))
        self.assertEqual(
            {"path:register", "path:untouched"},
            {
                o.subject.token
                for o in obligations(harmless)
                if o.rule_id.startswith("bytes.")
            },
        )
        keys = keys_of(base, FAKE)
        self.assertEqual(1, len(keys))
        self.assertEqual(keys, keys_of(harmless, FAKE))
        self.assertEqual({keys[0]: ["path:register"]}, subjects_of(harmless, FAKE))

    def test_role_order_is_canonical_and_a_repeated_role_is_refused(self) -> None:
        roles = (("observed", "absent"), ("expected", "M"))
        error = ("status.model-live-stale-status", "model-live-stale-status", roles)
        sorted_error = (*error[:2], tuple(sorted(roles)))
        self.assertEqual(
            error_set_document([sorted_error]), error_set_document([error])
        )
        self.assertEqual(
            [["expected", "M"], ["observed", "absent"]],
            error_set_document([error])["errors"][0][2],
        )
        with self.assertRaises(ValueError):
            error_set_document(
                [("bytes.torn-read", "torn-read", (("channel", "a"), ("channel", "b")))]
            )

    def test_subset_and_superset_stay_distinct_and_related(self) -> None:
        (subset,) = self._key(torn())
        (superset,) = self._key(*self.BASE)
        self.assertNotEqual(subset, superset)
        verdict = judge(script(torn("path:a"), torn("path:b"), status("path:b")))
        documents = {
            o.task_key: o.document for o in keyed(verdict, SCRIPTED).occurrences
        }
        self.assertEqual({subset, superset}, set(documents))
        self.assertEqual(
            ((superset, "superset"),), related_keys(documents[subset], documents)
        )
        self.assertEqual(
            ((subset, "subset"),), related_keys(documents[superset], documents)
        )

    def test_declared_role_variation_re_keys_and_attribution_does_not(self) -> None:
        self.assertNotEqual(self._key(torn()), self._key(torn(channel="thrift")))
        self.assertEqual(self._key(torn()), self._key(torn(attribution="undetermined")))

    def test_unrelated_subjects_never_share_a_run_wide_key(self) -> None:
        verdict = judge(script(torn("path:a"), stale("path:b")))
        self.assertEqual(
            sorted([TORN_FUSE, self._key(stale())[0]]), sorted(keys_of(verdict))
        )

    def test_key_is_the_document_digest_and_pinned(self) -> None:
        (key,) = self._key(torn())
        document = error_set_document(
            [("bytes.torn-read", "torn-read", (("channel", "fuse"),))]
        )
        self.assertEqual(key, TORN_FUSE)
        self.assertEqual(key, document_digest(document))
        self.assertEqual(
            {
                "schema": "exploratory-error-set-v1",
                "errors": [["bytes.torn-read", "torn-read", [["channel", "fuse"]]]],
            },
            document,
        )
        self.assertEqual(PINNED_TORN_FUSE, key)

    def test_the_edenfs_registry_identity_digest_is_pinned(self) -> None:
        """Any rule ID, kind, identity role, or red-control change moves it."""
        self.assertEqual(
            PINNED_EDENFS_IDENTITY,
            identity_digest(registry(EDENFS_TARGET), EDENFS_TARGET.semantics_profile),
        )


class CheckoutSubjectTest(unittest.TestCase):
    """A checkout-shape finding keys on its own action's subject."""

    def _keys(self, *, count: bool, reject: bool) -> set[str]:
        run = Run("t", "checkout-subjects", "c1 misreports its count; c2 is refused")
        if count:
            run.act("a", "w0", "write", path=R0, value_id="w0")
            run.act("a", "c1", "checkout", revision=2, via="thrift", mode="normal")
        if reject:
            run.act("a", "c2", "checkout", revision=1, via="thrift", mode="normal")
        if count:
            run.call("w0", "ok", **WRITE_OK)
            run.call("c1", "ok", **thrift_checkout(2, conflicts=0))
        if reject:
            run.call("c2", "fail", **thrift_rejected())
        run.finish()
        verdict = judge_edenfs(run)
        return {item.task_key for item in keyed(verdict, EDENFS_TARGET).occurrences}

    def test_two_checkouts_findings_key_as_each_does_alone(self) -> None:
        count = self._keys(count=True, reject=False)
        reject = self._keys(count=False, reject=True)
        self.assertEqual((1, 1), (len(count), len(reject)))
        self.assertEqual(count | reject, self._keys(count=True, reject=True))


class RoutingTest(unittest.TestCase):
    """Product-actionable subjects key; non-product ones route, never key."""

    def test_mixed_product_and_suspect_stays_product_actionable(self) -> None:
        verdict = judge(
            script(
                torn(),
                status(attribution="oracle-suspect"),
                stale(attribution="harness"),
            )
        )
        run = keyed(verdict, SCRIPTED)
        (occurrence,) = run.occurrences
        self.assertEqual(TORN_FUSE, occurrence.task_key)
        self.assertEqual(
            ["harness", "oracle-suspect", "product"], sorted(occurrence.attributions)
        )
        self.assertEqual((), run.routed)

    def test_all_suspect_routes_to_the_harness_and_reference_to_the_reference(
        self,
    ) -> None:
        cases = {
            "oracle-suspect": ("oracle-suspect",),
            "harness": ("harness",),
            "reference": ("reference",),
            "harness and reference": ("harness", "reference"),
        }
        expected = {
            "oracle-suspect": "harness",
            "harness": "harness",
            "reference": "reference",
            "harness and reference": "harness",
        }
        for label, attributions in cases.items():
            with self.subTest(case=label):
                obligations = [
                    torn(attribution=a, scope=f"s{i}")
                    for i, a in enumerate(attributions)
                ]
                run = keyed(judge(script(*obligations)), SCRIPTED)
                self.assertEqual((), run.occurrences)
                (routed,) = run.routed
                self.assertEqual(expected[label], routed.route)
                self.assertEqual(expected[label], route_of(attributions))
        self.assertEqual("product", route_of(("undetermined", "reference")))

    def test_routed_digest_never_collides_with_a_task_key(self) -> None:
        run = keyed(judge(script(torn(attribution="oracle-suspect"))), SCRIPTED)
        (routed,) = run.routed
        self.assertNotEqual(TORN_FUSE, routed.digest)


FUSE_CHANNEL = (Witness("channel", "fuse"),)


class UnclassifiedTest(unittest.TestCase):
    def _results(self, *obligations: ObligationResult) -> Any:
        return synthetic_verdict(
            PropertyResult(
                "bytes",
                tuple(sorted(obligations, key=lambda o: (o.rule_id, o.scope))),
            )
        )

    def _violation(
        self,
        rule: str = "bytes.torn-read",
        witnesses: tuple[Witness, ...] = FUSE_CHANNEL,
        subject: Subject | None = None,
        scope: str = "s",
    ) -> ObligationResult:
        return ObligationResult(
            rule_id=rule,
            scope=scope,
            subject=subject or Subject(SubjectKind.PATH, "path:r0"),
            status=ObligationStatus.VIOLATED,
            witnesses=witnesses,
            attribution=Attribution.PRODUCT,
            reason="x",
        )

    def test_unclassified_causes_keep_the_subject_unkeyed(self) -> None:
        cases = {
            "missing-role": self._violation(witnesses=()),
            "duplicate-role": self._violation(
                witnesses=(Witness("channel", "fuse"), Witness("channel", "thrift"))
            ),
            "subject-kind-mismatch": self._violation(
                subject=Subject(SubjectKind.CHECKOUT, "checkout:all")
            ),
            "oversize-role": self._violation(
                witnesses=(Witness("channel", "x" * 300),)
            ),
            "unknown-rule": self._violation(rule="bytes.never-registered"),
        }
        for cause, obligation in cases.items():
            with self.subTest(cause=cause):
                run = keyed(self._results(obligation), SCRIPTED)
                self.assertEqual((), run.occurrences)
                (group,) = run.incomplete
                self.assertEqual([cause], [u.cause for u in group.unclassified])

    def test_an_unclassified_finding_never_keys_its_subject_but_others_key(
        self,
    ) -> None:
        verdict = self._results(
            self._violation(witnesses=(), scope="a"),
            self._violation(scope="b"),
            self._violation(subject=Subject(SubjectKind.PATH, "path:r1"), scope="c"),
        )
        run = keyed(verdict, SCRIPTED)
        self.assertEqual(["path:r1"], [s for o in run.occurrences for s in o.subjects])
        self.assertEqual(["path:r0"], [g.subject.token for g in run.incomplete])

    def test_a_foreign_rule_is_unclassified(self) -> None:
        verdict = synthetic_verdict(
            PropertyResult(
                "status",
                (self._violation(rule="status.model-live-stale-status", witnesses=()),),
            )
        )
        table = dict(findings.rule_table(SCRIPTED))
        table["status.model-live-stale-status"] = (
            "bytes",
            table["status.model-live-stale-status"][1],
        )
        run = keyed(verdict, SCRIPTED, table)
        self.assertEqual(
            ["foreign-rule"], [u.cause for g in run.incomplete for u in g.unclassified]
        )


def _corpus_target(rules: Mapping[str, Any]) -> Target:
    """A target owning every non-core corpus rule, by family, for keying only."""
    families: dict[str, dict[str, RuleDef]] = {}
    for rule_id, body in rules.items():
        family = rule_id.split(".", 1)[0]
        if family == "core":
            continue
        families.setdefault(family, {})[rule_id] = RuleDef(
            id=rule_id,
            statement=body["statement"],
            subject_kind=SubjectKind(body["subject_kind"]),
            identity_roles=tuple(body["identity_roles"]),
            red_controls=frozenset(body["red_controls"]),
        )
    properties = tuple(
        PropertyDef(
            id=family,
            collect=lambda _input, _sink: None,
            rules=table,
            consumes=frozenset({(READ, "bytes")}) if family == "bytes" else frozenset(),
        )
        for family, table in sorted(families.items())
    )
    return dataclasses.replace(
        FAKE,
        name="corpus",
        actions=ACTIONS,
        properties=properties,
        red_controls=frozenset(
            control
            for p in properties
            for r in p.rules.values()
            for control in r.red_controls
        ),
    )


def _subject(token: str) -> Subject:
    kind = token.split(":", 1)[0]
    return Subject(SubjectKind(kind), token)


class CorpusKeyTest(unittest.TestCase):
    """The findings pipeline reproduces every corpus case's declared canonical keys."""

    def test_declared_error_sets_key_identically(self) -> None:
        target = _corpus_target(loader.rules()["rules"])
        owner = {rule: p.id for p in target.properties for rule in p.rules}
        checked = 0
        for case in loader.cases():
            expectation = case.expectation
            by_property: dict[str, list[ObligationResult]] = {}
            for index, expected in enumerate(expectation.obligations):
                if expected.outcome != "violated":
                    continue
                pid = (
                    expected.rule_id
                    if expected.rule_id.startswith("core.")
                    else owner.get(expected.rule_id)
                )
                if pid is None:
                    continue
                for part, identity in enumerate(expected.identities):
                    witnesses = tuple(Witness(r, v) for r, v in identity.items())
                    by_property.setdefault(pid, []).append(
                        ObligationResult(
                            rule_id=expected.rule_id,
                            scope=f"{expected.anchor}#{index}.{part}",
                            subject=_subject(expected.subject),
                            status=ObligationStatus.VIOLATED,
                            witnesses=(*witnesses, Witness("anchor", expected.anchor)),
                            attribution=Attribution(expected.attribution),
                            reason=expected.reason,
                        )
                    )
            core = [
                PropertyResult(
                    pid,
                    tuple(
                        sorted(
                            by_property.pop(pid, []), key=lambda o: (o.rule_id, o.scope)
                        )
                    ),
                )
                for pid in CORE_PROPERTY_IDS
            ]
            rest = [
                PropertyResult(
                    p.id,
                    tuple(
                        sorted(
                            by_property.pop(p.id, []),
                            key=lambda o: (o.rule_id, o.scope),
                        )
                    ),
                )
                for p in target.properties
            ]
            verdict = synthetic_verdict(*rest)
            assert verdict.report is not None
            verdict = dataclasses.replace(
                verdict,
                report=dataclasses.replace(verdict.report, results=(*core, *rest)),
            )
            observed = {
                (subject, o.task_key)
                for o in keyed(verdict, target).occurrences
                for subject in o.subjects
            }
            declared = {(s.subject, s.key) for s in expectation.canonical_errors}
            with self.subTest(case=expectation.case):
                self.assertEqual(declared, observed)
            checked += bool(declared)
        self.assertGreater(checked, 20)


def scripted_campaign(root: Path, runs: Mapping[int, list[dict[str, Any]]]) -> Path:
    """Write a campaign of scripted explore runs: report, summary, run dirs."""
    artifacts = root / "artifacts"
    artifacts.mkdir(parents=True)
    rows = []
    for seed, entries in sorted(runs.items()):
        run_dir = artifacts / f"seed-{seed}"
        verdict = write_run(run_dir, entries, seed=seed)
        write_admission(run_dir, admission_for(RunPurpose.EXPLORE, "campaign-x"))
        rows.append(summary_line(seed, run_dir, verdict))
    (artifacts / "summary.jsonl").write_text(
        "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows)
    )
    report = report_document(
        CampaignReport(
            attempts=len(rows),
            completed=len(rows),
            harness_failures=0,
            environment_degraded=0,
            verdicts={},
            completed_strata={},
            missing_strata=(),
            rejects=(),
        ),
        {"campaign_id": "campaign-x"},
    )
    (root / "report.json").write_text(json.dumps(report))
    return artifacts


class ReportModeIdentityTest(unittest.TestCase):
    RUNS: Mapping[int, list[dict[str, Any]]] = {
        1: script(
            torn("path:a"),
            status("path:a", attribution="oracle-suspect"),
            Ob("readdir.duplicate-entry", "path:far", status="not-evaluated"),
        ),
        2: script(torn("path:b"), stale("path:c", attribution="harness")),
        3: script(torn("path:d", attribution="reference")),
    }

    def _report(self, root: Path, name: str) -> Mapping[str, Any]:
        findings.write_findings(
            root / "report.json", root / "artifacts", root / name, SCRIPTED
        )
        return read_findings(root / name / "findings.json")

    def test_the_report_keys_error_sets_and_routes_non_product_subjects(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripted_campaign(root, self.RUNS)
            first = self._report(root, "first")
            again = self._report(root, "again")
            body = (root / "first" / first["findings"][0]["body"]).read_text()
        self.assertEqual(
            json.dumps(first["findings"], sort_keys=True).replace("/first/", "/"),
            json.dumps(again["findings"], sort_keys=True).replace("/again/", "/"),
        )
        (entry,) = first["findings"]
        self.assertEqual(TORN_FUSE, entry["task_key"])
        self.assertEqual(
            [["bytes.torn-read", "torn-read", [["channel", "fuse"]]]], entry["errors"]
        )
        self.assertEqual(
            [(1, ["path:a"]), (2, ["path:b"])],
            [(run["seed"], run["subjects"]) for run in entry["runs"]],
        )
        self.assertEqual(
            [
                ("bytes.torn-read", "product", "path:a"),
                ("status.model-live-stale-status", "oracle-suspect", "path:a"),
            ],
            sorted(
                (o["rule_id"], o["attribution"], o["subject"])
                for o in entry["runs"][0]["obligations"]
            ),
        )
        self.assertIn(
            f"replay --target scripted --run {root}/artifacts/seed-1",
            entry["runs"][0]["replay"],
        )
        self.assertIn(TORN_FUSE, body)
        self.assertEqual(
            [("path:c", "harness"), ("path:d", "reference")],
            [(r["subject"], r["route"]) for r in first["routed"]],
        )
        self.assertEqual("scripted-v1", first["semantics_profile"])
        self.assertEqual(semantic_digest(SCRIPTED), first["semantic_digest"])
