# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import json
import time
import unittest
from dataclasses import replace

from eden.integration.correctness.exploratory.core.checker import (
    check,
    CORE_INPUT_LEGALITY,
    CORE_RULES,
    Verdict,
)
from eden.integration.correctness.exploratory.core.facts import (
    FieldKind,
    FieldSpec,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.history import InMemorySink
from eden.integration.correctness.exploratory.core.identity import (
    canonical_errors,
    error_set_key,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    Finding,
    RuleDef,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import from_json, RunSpec
from eden.integration.correctness.exploratory.core.scheduler import run_actions
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.fake.target import BYTES_RULES, FAKE


def _rule(rule_id: str, roles: tuple[str, ...] = ("channel",)) -> RuleDef:
    return RuleDef(
        id=rule_id,
        statement="a read returns what a write produced",
        subject_kind=SubjectKind.PATH,
        identity_roles=roles,
    )


_RULES = {
    rule.id: rule
    for rule in (
        _rule("bytes.stale-read"),
        _rule("bytes.torn-read"),
        _rule("status.lost", ("channel", "errno")),
    )
}
_FILE = Subject(SubjectKind.PATH, "path:file")
_OTHER = Subject(SubjectKind.PATH, "path:other")


def _finding(
    rule_id: str = "bytes.stale-read",
    subject: Subject = _FILE,
    *witnesses: Witness,
    attribution: Attribution = Attribution.PRODUCT,
) -> Finding:
    return Finding(
        rule_id,
        attribution,
        f"{rule_id}: reason text is evidence",
        subject,
        (Witness("channel", "fuse"), *witnesses),
    )


def _keys(*findings: Finding) -> list[str]:
    return [error_set.key for error_set in canonical_errors(findings, _RULES).sets]


class CanonicalErrorsTest(unittest.TestCase):
    def test_errors_are_partitioned_by_subject_and_keyed_by_identity_only(
        self,
    ) -> None:
        result = canonical_errors(
            [
                _finding("bytes.stale-read", _FILE, Witness("read-action", "r1")),
                _finding("bytes.torn-read", _FILE),
                _finding("bytes.stale-read", _OTHER),
            ],
            _RULES,
        )
        file_set, other_set = result.sets
        self.assertEqual((_FILE, _OTHER), (file_set.subject, other_set.subject))
        self.assertEqual(
            (
                ("bytes.stale-read", "stale-read", (("channel", "fuse"),)),
                ("bytes.torn-read", "torn-read", (("channel", "fuse"),)),
            ),
            file_set.errors,
        )
        self.assertEqual(2, len(file_set.findings))
        self.assertEqual(error_set_key(file_set.errors), file_set.key)
        self.assertNotEqual(file_set.key, other_set.key)
        self.assertRegex(file_set.key, r"^sha256:[0-9a-f]{64}$")
        self.assertEqual((), result.unclassified)

    def test_key_is_invariant_under_instance_evidence(self) -> None:
        base = _keys(_finding())
        variants = {
            "action-ID renaming": _keys(
                _finding("bytes.stale-read", _FILE, Witness("read-action", "r99"))
            ),
            "witness changes": _keys(
                _finding(
                    "bytes.stale-read",
                    _FILE,
                    Witness("overwriter-action", "w3"),
                    Witness("proof", "linearization"),
                )
            ),
            "subject path change": _keys(_finding("bytes.stale-read", _OTHER)),
            "redundant observations": _keys(
                _finding("bytes.stale-read", _FILE, Witness("read-action", "r1")),
                _finding("bytes.stale-read", _FILE, Witness("read-action", "r2")),
            ),
            "reason text": _keys(replace(_finding(), reason="entirely different text")),
            "non-product companions": _keys(
                _finding(),
                _finding("bytes.torn-read", attribution=Attribution.HARNESS),
                _finding("bytes.torn-read", attribution=Attribution.ORACLE_SUSPECT),
                _finding("bytes.torn-read", attribution=Attribution.REFERENCE),
            ),
            "undetermined attribution": _keys(
                _finding(attribution=Attribution.UNDETERMINED)
            ),
        }
        for name, keys in variants.items():
            with self.subTest(variant=name):
                self.assertEqual(base, keys)

    def test_identity_changes_change_the_key(self) -> None:
        base = _keys(_finding())
        variants = {
            "identity role value": _keys(
                replace(_finding(), witnesses=(Witness("channel", "thrift"),))
            ),
            "rule": _keys(_finding("bytes.torn-read")),
            "superset": _keys(_finding(), _finding("bytes.torn-read")),
        }
        for name, keys in variants.items():
            with self.subTest(variant=name):
                self.assertEqual(1, len(keys))
                self.assertNotEqual(base, keys)

    def test_errno_and_input_kind_each_split_input_legality_keys(self) -> None:
        """Input-legality failures differing only in errno, or only in input kind, never merge."""

        def legality(kind: str, errno: str) -> list[str]:
            finding = Finding(
                CORE_INPUT_LEGALITY,
                Attribution.PRODUCT,
                "input failed illegally",
                _FILE,
                (Witness("input-kind", kind), Witness("errno", errno)),
            )
            return [s.key for s in canonical_errors([finding], CORE_RULES).sets]

        base = legality("write", "EIO")
        self.assertEqual(base, legality("write", "EIO"))
        variants = {
            "errno only": legality("write", "ENOENT"),
            "input kind only": legality("read", "EIO"),
        }
        for name, keys in variants.items():
            with self.subTest(variant=name):
                self.assertEqual(1, len(keys))
                self.assertNotEqual(base, keys)
        self.assertNotEqual(variants["errno only"], variants["input kind only"])

    def test_unclassifiable_findings_are_kept_but_never_keyed(self) -> None:
        missing = _finding("status.lost")
        conflicting = _finding("bytes.stale-read", _FILE, Witness("channel", "thrift"))
        unknown = _finding("bytes.gone")
        result = canonical_errors([missing, conflicting, unknown], _RULES)
        self.assertEqual((), result.sets)
        self.assertEqual((missing, conflicting, unknown), result.unclassified)
        harness = _finding(attribution=Attribution.HARNESS)
        self.assertEqual((), canonical_errors([harness], _RULES).unclassified)


_SPEC = json.dumps(
    {
        "schema": "edenfs-exploratory-run",
        "schema_version": 1,
        "seed": 7,
        "target": "fake",
        "environment": {"target": {}},
        "budgets": {
            "operation_deadline_seconds": 60,
            "cleanup_deadline_seconds": 10,
            "runner_deadline_seconds": 120,
        },
        "actors": [{"id": "a", "actions": ["w", "r"]}],
        "actions": {
            "w": {
                "kind": "write",
                "deadline": "operation",
                "path": "register",
                "value_id": "written",
            },
            "r": {"kind": "read", "deadline": "operation", "path": "register"},
        },
    }
)


def _torn(
    spec_json: str,
    target_name: str = "fake",
    profile: str = FAKE.semantics_profile,
    target: Target = FAKE,
) -> Verdict:
    target = replace(target, name=target_name, semantics_profile=profile)
    raw = json.loads(spec_json)
    raw["target"] = target_name
    spec: RunSpec = from_json(json.dumps(raw), target)
    environment = target.make_environment(spec, None)
    backend = target.make_backend(environment, spec, "torn-write")
    sink = InMemorySink()
    run_actions(spec, backend, sink, time.monotonic)
    return check(target, spec, sink.events)


def _verdict_keys(verdict: Verdict) -> list[str]:
    return list(_subject_keys(verdict).values())


def _subject_keys(verdict: Verdict) -> dict[str, str]:
    assert verdict.report is not None
    findings = [f for result in verdict.report.results for f in result.findings]
    return {
        s.subject.token: s.key for s in canonical_errors(findings, BYTES_RULES).sets
    }


def _with_actions(spec_json: str, extra: dict[str, dict[str, str]]) -> str:
    raw = json.loads(spec_json)
    raw["actions"].update(extra)
    raw["actors"][0]["actions"] += list(extra)
    return json.dumps(raw)


# The same fake target whose unmodeled getattr declares one more diagnostic
# field: its semantic digest differs and nothing it judges does.
_GETATTR = FAKE.actions["getattr"]
_REDECLARED = replace(
    FAKE,
    actions={
        **FAKE.actions,
        "getattr": replace(
            _GETATTR,
            result=ResultSchema(
                ok=(
                    *_GETATTR.result.ok,
                    FieldSpec("trace", FieldKind.JSON, required=False, consumed=False),
                )
            ),
        ),
    },
)


def _report_actions(verdict: Verdict) -> int:
    assert verdict.report is not None
    return verdict.report.work.phases["construction"]["obligations"]


class VerdictIdentityTest(unittest.TestCase):
    def test_target_profile_and_digest_changes_keep_the_key(self) -> None:
        base = _torn(_SPEC)
        renamed_path = _SPEC.replace('"register"', '"other-register"')
        renamed_actions = _SPEC.replace('"r"', '"read-9"')
        variants = {
            "target name": _torn(_SPEC, "fake-renamed"),
            "semantics profile": _torn(_SPEC, profile="fake-memory-v2"),
            "semantic digest": _torn(_SPEC, target=_REDECLARED),
            "concrete path": _torn(renamed_path),
            "action IDs": _torn(renamed_actions),
        }
        keys = _verdict_keys(base)
        self.assertEqual(1, len(keys))
        for name, verdict in variants.items():
            with self.subTest(variant=name):
                self.assertEqual(keys, _verdict_keys(verdict))
        assert base.report is not None
        profiled = variants["semantics profile"].report
        assert profiled is not None
        self.assertNotEqual(base.report.identity_digest, profiled.identity_digest)
        redeclared = variants["semantic digest"].report
        assert redeclared is not None
        self.assertNotEqual(base.report.bindings.semantic, redeclared.bindings.semantic)
        self.assertEqual(base.report.identity_digest, redeclared.identity_digest)

    def test_harmless_independent_actions_keep_the_key(self) -> None:
        """Unmodeled and legal reads elsewhere never change a subject's key."""
        base = _subject_keys(_torn(_SPEC))
        self.assertEqual(["path:register"], list(base))
        harmless = _with_actions(
            _SPEC,
            {
                "g": {"kind": "getattr", "deadline": "operation", "path": "register"},
                "r2": {"kind": "read", "deadline": "operation", "path": "untouched"},
                "r3": {"kind": "read", "deadline": "operation", "path": "untouched"},
            },
        )
        verdict = _torn(harmless)
        assert verdict.report is not None
        self.assertNotEqual(_report_actions(_torn(_SPEC)), _report_actions(verdict))
        self.assertEqual(base, _subject_keys(verdict))
