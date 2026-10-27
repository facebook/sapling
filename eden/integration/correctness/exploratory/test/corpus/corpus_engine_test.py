# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The expected-results corpus judged by `EDENFS_TARGET` through the real checker.

Every case matches its declared expectations (`loader.mismatches` is
empty). Every rule a contract input supports is registered, so the contracts
drops none. Verdict bytes, proofs, and canonical error-set keys are
identical across Python hash seeds.
"""

from __future__ import annotations

import contextlib
import dataclasses
import hashlib
import io
import json
import unittest
from collections.abc import Iterable

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.identity import canonical_errors
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.core.test.testing import (
    run_in_fresh_interpreter,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.test.corpus import loader
from eden.integration.correctness.exploratory.test.waiting import Waiting

COMPLETED = {"status": "completed"}
_IDENTITY_ROLES = {
    rule.id: rule.identity_roles
    for definition in checker.registry(EDENFS_TARGET)
    for rule in definition.rules.values()
}
REGISTERED = frozenset(_IDENTITY_ROLES)
_RULES = {
    rule.id: rule
    for definition in checker.registry(EDENFS_TARGET)
    for rule in definition.rules.values()
}


# Rules whose scopes are `<action>:<path>` (`common.path_scope`).
_PATH_SCOPED = ("status.", "journal.")
# Rules whose scopes are `<action>` or `<action>:<part>`.
_ACTION_PART_SCOPED = frozenset(
    {"existence.mkdir-type", "checkout.sl-abort", "attributes.type-mismatch"}
)


def _anchor(rule_id: str, scope: str) -> str:
    """Return the corpus anchor of an obligation: the action its scope names.

    Status and journal scopes are `<action>:<path>`; an `existence.mkdir-type`
    scope is `<getattr>` (FUSE) or `<getattr>:thrift`; a `checkout.sl-abort`
    scope is `<checkout>`, `<checkout>:listed`, or `<checkout>:blocked`
    (`contracts.checkout.abort_scope`); an `attributes.type-mismatch` scope
    is `<action>` or `<action>:thrift`.
    """
    if rule_id.startswith(_PATH_SCOPED) or rule_id in _ACTION_PART_SCOPED:
        return scope.split(":", 1)[0]
    return scope


def observed(
    verdict: checker.Verdict, skip: Iterable[str] = ()
) -> list[loader.ObservedObligation]:
    """Return a verdict's obligations in the corpus's anchor convention."""
    assert verdict.report is not None
    found = []
    for result in verdict.report.results:
        if result.property_id in skip:
            continue
        for item in result.obligations:
            roles = _IDENTITY_ROLES[item.rule_id]
            found.append(
                loader.ObservedObligation(
                    item.rule_id,
                    _anchor(item.rule_id, item.scope),
                    item.subject.token,
                    str(item.status),
                    None if item.attribution is None else str(item.attribution),
                    item.not_evaluated_reason,
                    {w.role: w.value for w in item.witnesses if w.role in roles},
                )
            )
    return found


def judge(target: Target, case: loader.CorpusCase) -> checker.Verdict:
    """Return the checker's verdict on one corpus case."""
    return checker.check(target, case.spec(target), case.events(), execution=COMPLETED)


def digest_document(cases: Iterable[loader.CorpusCase]) -> str:
    """Return the sha256 over every case's verdict bytes and canonical error-set keys."""
    digest = hashlib.sha256()
    for case in cases:
        verdict = judge(EDENFS_TARGET, case)
        digest.update(case.name.encode() + b"\0" + checker.encode_verdict(verdict))
        assert verdict.report is not None
        findings = [
            item.finding()
            for result in verdict.report.results
            for item in result.obligations
            if str(item.status) == "violated"
        ]
        errors = canonical_errors(findings, _RULES)
        keys = [[item.subject.token, item.key] for item in errors.sets]
        digest.update(json.dumps([keys, len(errors.unclassified)]).encode())
    return digest.hexdigest()


class CorpusEngineTest(unittest.TestCase):
    def _check(self, case: loader.CorpusCase, waiting: Waiting) -> None:
        with waiting.case(self, case.name, case.waiting_on):
            verdict = judge(EDENFS_TARGET, case)
            self.assertEqual(
                (),
                loader.mismatches(
                    case.expectation,
                    observed(verdict),
                    registered=REGISTERED,
                    verdict=verdict.kind,
                ),
            )

    def test_every_case_matches_its_expectations(self) -> None:
        waiting = Waiting()
        for case in loader.cases():
            with self.subTest(case=case.name):
                self._check(case, waiting)
        waiting.report()

    def test_a_waiting_case_is_skipped_until_it_matches(self) -> None:
        case = loader.cases()[0]
        spec = json.loads(case.spec_text)
        spec.setdefault("metadata", {})["waiting_on"] = "T1"
        waiting_case = dataclasses.replace(case, spec_text=json.dumps(spec))
        waiting = Waiting()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self._check(waiting_case, waiting)
        self.assertEqual(f"{case.name} now passes; remove the marker\n", out.getvalue())
        wrong = dataclasses.replace(
            waiting_case,
            expectation=dataclasses.replace(case.expectation, verdict="other"),
        )
        with self.assertRaisesRegex(unittest.SkipTest, "waiting on T1"):
            self._check(wrong, waiting)
        self.assertEqual({"T1": 1}, dict(waiting.counts))

    def test_every_contract_rule_is_registered(self) -> None:
        """No contract input is dropped: every rule it supports has a declaring property."""
        self.assertLessEqual(CONTRACT_RULES, REGISTERED)

    def test_verdicts_and_keys_are_identical_across_hash_seeds(self) -> None:
        """Verdict bytes, proofs, work, and task keys do not depend on PYTHONHASHSEED."""
        script = (
            "from eden.integration.correctness.exploratory.test.corpus import "
            "corpus_engine_test as t, loader\n"
            "print(t.digest_document(loader.cases()))\n"
        )
        digests = {
            run_in_fresh_interpreter(script, seed).strip().splitlines()[-1]
            for seed in ("0", "1", "4242")
        }
        self.assertEqual(1, len(digests), digests)
