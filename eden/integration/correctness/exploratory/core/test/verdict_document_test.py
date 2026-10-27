# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The verdict document: version 1 lists every finding once with its proof
references; the reader round-trips exactly what the writer produces and
rejects every other shape (non-canonical, repeated or non-finite bytes,
missing proofs, results that do not match the registry, a projection that does
not recompute); its digests bind each judged document; and a maximum shape is
counted and never truncated.
"""

from __future__ import annotations

import json
import unittest
from collections.abc import Callable
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core import (
    checker as checker_module,
    effects,
)
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    bind,
    check,
    CORE_ACTIVITY,
    decode_verdict,
    encode_verdict,
    INVALID_HISTORY,
    NO_EVALUATION,
    NO_PROPERTY_REASON,
    NOT_EVALUATED,
    PROPERTY_ERROR,
    read_verdict_document,
    registry,
    Verdict,
    verdict_document,
    VERDICT_OPERATIONAL_BYTES,
)
from eden.integration.correctness.exploratory.core.history import OK
from eden.integration.correctness.exploratory.core.jsonio import MAX_SCALAR_BYTES
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CheckInput,
    CollectorSink,
    identity_digest,
    Subject,
    SubjectKind,
    Witness,
)
from eden.integration.correctness.exploratory.core.test import checker_testing as t


def _record_findings(_input: CheckInput, sink: CollectorSink) -> None:
    sink.satisfied(sink.obligation("bytes.bad", "a", subject=t.PATH))
    t.violate(
        sink,
        "bytes.bad",
        "t4",
        Attribution.ORACLE_SUSPECT,
        "bytes/fuse≠model: hidden suspect",
        witnesses=(Witness("channel", "fuse"), Witness("read-action", "t4")),
    )
    t.violate(
        sink,
        "bytes.bad",
        "x",
        Attribution.UNDETERMINED,
        "bytes/fuse≠model: torn-read",
        Subject(SubjectKind.PATH, "path:other"),
        (Witness("channel", "fuse"),),
    )
    ref = sink.obligation("bytes.bad", "z", subject=t.PATH)
    sink.not_evaluated(ref, "bytes.no-input", "nothing")


def _findings_verdict() -> Verdict:
    return check(
        t.target(t.prop("bytes", _record_findings), t.quiet("status")),
        t.spec([t.action("r", "read")]),
        t.READ_OK,
    )


class VerdictDocumentTest(unittest.TestCase):
    def test_v1_document_shape_and_every_finding_once(self) -> None:
        verdict = _findings_verdict()
        document = verdict_document(verdict)
        self.assertEqual(
            [
                "bindings",
                "gate",
                "identity_digest",
                "kind",
                "lanes",
                "not_evaluated_cause",
                "proofs",
                "properties",
                "reason",
                "schema_version",
                "work",
            ],
            sorted(document),
        )
        self.assertEqual(1, document["schema_version"])
        self.assertEqual(
            ["execution", "history", "runspec", "semantic", "semantics_profile"],
            sorted(document["bindings"]),
        )
        self.assertEqual(
            ("reject", "bytes/fuse≠model: torn-read"),
            (document["kind"], document["reason"]),
        )
        self.assertEqual({"bytes": "reject", "status": "accept"}, document["lanes"])
        self.assertEqual([], document["proofs"])
        work = document["work"]
        self.assertEqual({}, work["components"])
        self.assertEqual(
            ["construction", "explanation", "propagation", "search"],
            sorted(work["phases"]),
        )
        construction = work["phases"]["construction"]
        self.assertEqual(
            (0, 0, 9, 0, 0),
            (
                construction["constraints"],
                construction["groups"],
                construction["obligations"],
                construction["observations"],
                construction["components"],
            ),
        )
        properties = {p["property_id"]: p for p in document["properties"]}
        self.assertEqual(
            [*t.CORE_IDS, "bytes", "status"],
            [p["property_id"] for p in document["properties"]],
        )
        path = {"kind": "path", "token": "path:file"}
        self.assertEqual(
            {
                "property_id": "bytes",
                "error": None,
                "coverage": {},
                "obligations": [
                    {
                        "rule_id": "bytes.bad",
                        "scope": "a",
                        "subject": path,
                        "status": "satisfied",
                        "witnesses": [],
                    },
                    {
                        "rule_id": "bytes.bad",
                        "scope": "t4",
                        "subject": path,
                        "status": "violated",
                        "attribution": "oracle-suspect",
                        "reason": "bytes/fuse≠model: hidden suspect",
                        "witnesses": [
                            {"role": "channel", "value": "fuse"},
                            {"role": "read-action", "value": "t4"},
                        ],
                    },
                    {
                        "rule_id": "bytes.bad",
                        "scope": "x",
                        "subject": {"kind": "path", "token": "path:other"},
                        "status": "violated",
                        "attribution": "undetermined",
                        "reason": "bytes/fuse≠model: torn-read",
                        "witnesses": [{"role": "channel", "value": "fuse"}],
                    },
                    {
                        "rule_id": "bytes.bad",
                        "scope": "z",
                        "subject": path,
                        "status": "not-evaluated",
                        "not_evaluated_reason": "bytes.no-input",
                        "reason": "nothing",
                        "witnesses": [],
                    },
                ],
            },
            properties["bytes"],
        )
        encoded = encode_verdict(verdict).decode()
        self.assertEqual(1, encoded.count("hidden suspect"))
        self.assertEqual(document, json.loads(encoded))

    def test_reader_round_trips_and_rejects_every_other_shape(self) -> None:
        verdict = _findings_verdict()
        document = verdict_document(verdict)
        self.assertEqual(
            verdict, read_verdict_document(json.loads(json.dumps(document)))
        )
        invalid = check(
            t.target(t.quiet()),
            t.spec([t.action("r", "read")]),
            [t.event(OK, "r", 1.0)],
        )
        self.assertEqual(invalid, read_verdict_document(verdict_document(invalid)))

        def mutated(change: Callable[[dict[str, Any]], None]) -> dict[str, Any]:
            copy = json.loads(json.dumps(document))
            change(copy)
            return copy

        def obligation(d: dict[str, Any], index: int) -> dict[str, Any]:
            return d["properties"][4]["obligations"][index]

        proof = {
            "id": "p1",
            "obligations": [["bytes.bad", "a"]],
            "observations": [],
            "groups": [],
            "constraints": [],
        }
        cases: dict[str, dict[str, Any]] = {
            "older version": mutated(lambda d: d.update(schema_version=0)),
            "newer version": mutated(lambda d: d.update(schema_version=2)),
            "bool version": mutated(lambda d: d.update(schema_version=True)),
            "missing version": mutated(lambda d: d.pop("schema_version")),
            "unknown key": mutated(lambda d: d.update(extra=1)),
            "old digest key": mutated(
                lambda d: d.update(registry_digest=d.pop("identity_digest"))
            ),
            "old checker binding": mutated(
                lambda d: d["bindings"].update(checker=d["bindings"].pop("semantic"))
            ),
            "bad profile": mutated(
                lambda d: d["bindings"].update(semantics_profile="Bad Profile")
            ),
            "bad digest": mutated(lambda d: d.update(identity_digest="sha256:00")),
            "cause on reject": mutated(
                lambda d: d.update(not_evaluated_cause="blocked")
            ),
            "bad lane": mutated(lambda d: d["lanes"].update(bytes="maybe")),
            "satisfied with a reason": mutated(
                lambda d: obligation(d, 0).update(reason="x")
            ),
            "violated without attribution": mutated(
                lambda d: obligation(d, 1).pop("attribution")
            ),
            "unknown attribution": mutated(
                lambda d: obligation(d, 1).update(attribution="model-suspect")
            ),
            "explicit null": mutated(lambda d: obligation(d, 1).update(proof=None)),
            "unknown obligation key": mutated(
                lambda d: obligation(d, 1).update(evidence="x")
            ),
            "missing subject": mutated(lambda d: obligation(d, 1).pop("subject")),
            "unsorted obligations": mutated(
                lambda d: d["properties"][4]["obligations"].reverse()
            ),
            "dangling proof": mutated(lambda d: obligation(d, 1).update(proof="p9")),
            "unreferenced proof": mutated(lambda d: d["proofs"].append(proof)),
            "negative work": mutated(
                lambda d: d["work"]["phases"]["construction"].update(groups=-1)
            ),
            "work without components": mutated(lambda d: d["work"].pop("components")),
            "gate without cause": mutated(lambda d: d.update(gate={"problems": ["x"]})),
            "recomputed reason": mutated(
                lambda d: d.update(reason="bytes/fuse≠model: hidden suspect")
            ),
            "recomputed kind": mutated(lambda d: d.update(kind="harness-suspect")),
            "recomputed lane": mutated(lambda d: d["lanes"].update(status="reject")),
            "missing lane": mutated(lambda d: d["lanes"].pop("status")),
            "core lane": mutated(
                lambda d: d["lanes"].update({CORE_ACTIVITY: "accept"})
            ),
            "gate with properties": mutated(
                lambda d: d.update(
                    kind="not-evaluated",
                    not_evaluated_cause="invalid-history",
                    gate={"problems": ["x"]},
                    reason="invalid-history/history≠spec: x",
                    lanes={},
                )
            ),
            "repeated property": mutated(
                lambda d: d["properties"].append(d["properties"][-1])
            ),
            "subject token": mutated(
                lambda d: obligation(d, 1)["subject"].update(token="/tmp/file")
            ),
            "witness role": mutated(
                lambda d: obligation(d, 1)["witnesses"][0].update(role="Channel")
            ),
            "attribution flip": mutated(
                lambda d: obligation(d, 2).update(attribution="harness")
            ),
            "error without bytes": mutated(
                lambda d: d["properties"][5].update(error={"type": "E", "message": "m"})
            ),
        }
        for name, raw in cases.items():
            with self.subTest(case=name):
                with self.assertRaises(ValueError):
                    read_verdict_document(raw)
        with self.assertRaisesRegex(ValueError, "needs a verdict produced by check"):
            verdict_document(Verdict(ACCEPT, "manual"))

    def test_proof_references_round_trip(self) -> None:
        verdict = _findings_verdict()
        report = t.report(verdict)
        bytes_result = report.results[4]
        proven = tuple(
            replace(o, proof="p1", component="c1") if o.scope == "x" else o
            for o in bytes_result.obligations
        )
        proof = effects.ProofCertificate(
            "p1",
            (effects.ObligationRef("bytes.bad", "x"),),
            ("o1",),
            ("g1",),
            ("before(g1,o1)",),
        )
        results = (
            *report.results[:4],
            replace(bytes_result, obligations=proven),
            *report.results[5:],
        )
        proven_verdict = replace(
            verdict, report=replace(report, results=results, proofs=(proof,))
        )
        self.assertEqual(proven_verdict, decode_verdict(encode_verdict(proven_verdict)))

    def test_reader_requires_proof_references_both_ways(self) -> None:
        """An obligation claiming a proof must be listed by that proof."""
        verdict = _findings_verdict()
        report = t.report(verdict)
        bytes_result = report.results[4]
        claimed = tuple(
            replace(o, proof="p1") if o.scope in ("t4", "x") else o
            for o in bytes_result.obligations
        )
        proof = effects.ProofCertificate(
            "p1", (effects.ObligationRef("bytes.bad", "t4"),), (), (), ()
        )
        results = (
            *report.results[:4],
            replace(bytes_result, obligations=claimed),
            *report.results[5:],
        )
        bad = replace(verdict, report=replace(report, results=results, proofs=(proof,)))
        with self.assertRaisesRegex(
            ValueError, "claims proof 'p1', which does not list it"
        ):
            decode_verdict(encode_verdict(bad))
        listed = replace(
            proof,
            obligations=(
                effects.ObligationRef("bytes.bad", "t4"),
                effects.ObligationRef("bytes.bad", "x"),
            ),
        )
        good = replace(bad, report=replace(t.report(bad), proofs=(listed,)))
        self.assertEqual(good, decode_verdict(encode_verdict(good)))

    def test_reader_requires_registry_shaped_results(self) -> None:
        """Rules stay in their property, obligations are unique, core comes first."""
        verdict = _findings_verdict()
        report = t.report(verdict)
        results = report.results
        activity = results[1]

        def reprojected(changed: tuple[Any, ...]) -> Verdict:
            kind, reason, cause = checker_module.project(changed)
            return replace(
                verdict,
                kind=kind,
                reason=reason,
                not_evaluated_cause=cause,
                lanes=checker_module.lanes(changed),
                report=replace(report, results=changed),
            )

        def under_bytes(*extra: Any) -> tuple[Any, ...]:
            merged = tuple(
                sorted(
                    (*results[4].obligations, *extra),
                    key=lambda o: (o.rule_id, o.scope),
                )
            )
            return (*results[:4], replace(results[4], obligations=merged), *results[5:])

        foreign = replace(activity.obligations[0], rule_id="status.bad", scope="q")
        cases = {
            "core obligation under a target property": (
                under_bytes(*activity.obligations),
                "rule 'core.activity' is reserved",
            ),
            "repeated obligation": (
                under_bytes(results[4].obligations[0]),
                r"appears twice",
            ),
            **{
                f"reserved property id {name}": (
                    (*results[:4], replace(results[4], property_id=name), *results[5:]),
                    f"bad property id '{name}'",
                )
                for name in ("core", "execution", "lifecycle")
            },
            "another target's rule": (under_bytes(foreign), "outside its property"),
            "core properties removed": (results[4:], "do not start with the core"),
            "core properties reordered": (
                (results[1], results[0], *results[2:]),
                "do not start with the core",
            ),
            "core property after target": (
                (*results, replace(results[0], property_id="core.extra")),
                "do not start with the core",
            ),
        }
        for name, (changed, message) in cases.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(ValueError, message):
                    decode_verdict(encode_verdict(reprojected(changed)))
        self.assertEqual(verdict, decode_verdict(encode_verdict(reprojected(results))))

    def test_digests_bind_each_judged_document(self) -> None:
        """Changing the RunSpec, history, profile, or execution changes its binding."""
        spec = t.spec([t.action("r", "read")])
        target = t.target(t.quiet())
        base = check(target, spec, t.READ_OK, execution={"status": "harness-failure"})
        assert base.report is not None
        bindings = base.report.bindings
        self.assertRegex(bindings.runspec, r"^sha256:[0-9a-f]{64}$")
        self.assertEqual(
            base.report.bindings,
            t.report(
                check(
                    target,
                    spec,
                    list(t.READ_OK),
                    execution={"status": "harness-failure"},
                )
            ).bindings,
        )
        changes = {
            "runspec": check(
                target,
                replace(spec, seed=8),
                t.READ_OK,
                execution={"status": "harness-failure"},
            ),
            "history": check(
                target,
                spec,
                [t.READ_OK[0], t.event(OK, "r", 2.5)],
                execution={"status": "harness-failure"},
            ),
            "execution": check(
                target, spec, t.READ_OK, execution={"status": "containment-timeout"}
            ),
        }
        names = ("runspec", "history", "execution", "semantic", "semantics_profile")
        for name, changed in changes.items():
            with self.subTest(document=name):
                assert changed.report is not None
                self.assertNotEqual(
                    getattr(bindings, name), getattr(changed.report.bindings, name)
                )
                for other in names:
                    if other != name:
                        self.assertEqual(
                            getattr(bindings, other),
                            getattr(changed.report.bindings, other),
                        )
        renamed = replace(target, semantics_profile="synthetic-v2")
        renamed_report = t.report(check(renamed, spec, t.READ_OK))
        self.assertNotEqual(base.report.identity_digest, renamed_report.identity_digest)
        self.assertEqual("synthetic-v2", renamed_report.bindings.semantics_profile)
        self.assertEqual(bindings.semantic, renamed_report.bindings.semantic)
        self.assertIsNone(t.report(check(target, spec, t.READ_OK)).bindings.execution)

    def test_maximum_shape_encoding_is_counted_and_never_truncated(self) -> None:
        """Maximal legal scalars encode whole; the byte count is exact and
        an oversize document still round-trips intact."""
        count = 600
        reason = "r" * MAX_SCALAR_BYTES
        value = "v" * MAX_SCALAR_BYTES

        def wide(_input: CheckInput, sink: CollectorSink) -> None:
            for index in range(count):
                t.violate(
                    sink,
                    "wide.bad",
                    f"s{index:04d}",
                    Attribution.PRODUCT,
                    reason,
                    witnesses=(Witness("w", value),),
                )

        verdict = check(
            t.target(t.prop("wide", wide)),
            t.spec([t.action("r", "read")]),
            t.READ_OK,
        )
        encoded = encode_verdict(verdict)
        self.assertGreater(len(encoded), VERDICT_OPERATIONAL_BYTES)
        # Every violated obligation plus the projected top-level reason.
        self.assertEqual(count + 1, encoded.count(reason.encode()))
        self.assertEqual(count, encoded.count(value.encode()))
        self.assertEqual(verdict, read_verdict_document(json.loads(encoded)))


class VerdictShapeTest(unittest.TestCase):
    _SPEC = t.spec([t.action("r", "read")])

    def test_decode_requires_canonical_unique_finite_bytes(self) -> None:
        verdict = _findings_verdict()
        encoded = encode_verdict(verdict)
        self.assertEqual(verdict, decode_verdict(encoded))
        self.assertIn(b"\\u2260", encoded)
        text = encoded.decode()
        duplicate = text.replace(
            '"schema_version": 1', '"schema_version": 1,\n  "schema_version": 1', 1
        )
        compact = json.dumps(json.loads(text), sort_keys=True) + "\n"
        cases = {
            "duplicate key": duplicate.encode(),
            "non-canonical spacing": compact.encode(),
            "missing newline": encoded[:-1],
            "raw utf-8": text.replace("\\u2260", "≠").encode(),
            "nan": text.replace('"groups": 0', '"groups": NaN', 1).encode(),
        }
        for name, data in cases.items():
            with self.subTest(case=name):
                with self.assertRaises(ValueError):
                    decode_verdict(data)

    def test_gate_and_zero_property_shapes(self) -> None:
        target = t.target(t.quiet())
        gated = check(target, self._SPEC, [t.event(OK, "r", 1.0)])
        self.assertEqual(gated, decode_verdict(encode_verdict(gated)))
        empty = t.target()
        fallback = check(empty, self._SPEC, t.READ_OK, execution={"status": "x"})
        self.assertEqual(
            (NOT_EVALUATED, NO_PROPERTY_REASON, NO_EVALUATION, {}),
            (
                fallback.kind,
                fallback.reason,
                fallback.not_evaluated_cause,
                fallback.lanes,
            ),
        )
        report = t.report(fallback)
        self.assertEqual(((), ()), (report.results, report.gate))
        self.assertEqual(
            identity_digest(registry(empty), empty.semantics_profile),
            report.identity_digest,
        )
        self.assertEqual(
            bind(self._SPEC, t.READ_OK, {"status": "x"}, target=empty),
            report.bindings,
        )
        self.assertEqual(fallback, decode_verdict(encode_verdict(fallback)))
        malformed = check(empty, self._SPEC, [t.event(OK, "r", 1.0)])
        self.assertEqual(INVALID_HISTORY, malformed.not_evaluated_cause)
        for bad in (
            replace(fallback, reason="all-properties-accept"),
            replace(fallback, reason="no-evaluation/properties≠target: t"),
            replace(fallback, not_evaluated_cause=PROPERTY_ERROR),
            replace(fallback, lanes={"quiet": NOT_EVALUATED}),
            replace(fallback, not_evaluated_cause=INVALID_HISTORY),
        ):
            with self.subTest(verdict=bad):
                with self.assertRaises(ValueError):
                    read_verdict_document(verdict_document(bad))
