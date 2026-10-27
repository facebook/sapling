# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import errno
import hashlib
import re
import unittest
from dataclasses import replace
from typing import Any, cast

from eden.integration.correctness.exploratory.core.actions import CORE_ACTION_DEFS
from eden.integration.correctness.exploratory.core.differential import REFERENCE_RESULT
from eden.integration.correctness.exploratory.core.effects import no_footprint
from eden.integration.correctness.exploratory.core.facts import (
    ActionFacts,
    ContentDigest,
    decode_result,
    ErrnoUse,
    FieldKind,
    FieldSpec,
    HistoryFacts,
    MISSING_REFERENCE,
    NO_ERRNO,
    Outcome,
    paired,
    REFERENCE_SIDE,
    ResultProblem,
    ResultSchema,
    TypedResult,
    UNEXPECTED_REFERENCE,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    Event,
    FAIL,
    INFO,
    INVOKE,
    OK,
    SKIPPED,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    Environment,
    RunSpec,
)
from eden.integration.correctness.exploratory.core.target import (
    ActionDef,
    Binding,
    Grouping,
    Target,
)
from eden.integration.correctness.exploratory.core.test.testing import ev


def _digest(data: bytes) -> dict[str, Any]:
    return {
        "bytes_prefix": data[:128].hex(),
        "length": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


_MEMBER = (FieldSpec("name", FieldKind.TEXT), FieldSpec("size", FieldKind.INT, False))
_EVERY_KIND = ResultSchema(
    ok=(
        FieldSpec("text", FieldKind.TEXT),
        FieldSpec("count", FieldKind.INT),
        FieldSpec("flag", FieldKind.BOOL),
        FieldSpec("hex", FieldKind.HEX),
        FieldSpec("digest", FieldKind.DIGEST),
        FieldSpec("names", FieldKind.TEXT_LIST),
        FieldSpec("numbers", FieldKind.INT_LIST),
        FieldSpec("entry", FieldKind.OBJECT, schema=_MEMBER),
        FieldSpec("entries", FieldKind.OBJECT_LIST, schema=_MEMBER),
        FieldSpec("trace", FieldKind.JSON, required=False, consumed=False),
    ),
    fail=(FieldSpec("errno", FieldKind.INT),),
    errnos={"ENOENT": ErrnoUse.STATE, "EIO": ErrnoUse.FAULT},
)
_LEGAL = {
    "text": "a",
    "count": 3,
    "flag": False,
    "hex": "00ff",
    "digest": _digest(b"x" * 200),
    "names": ["a", "b"],
    "numbers": [1, -2],
    "entry": {"name": "a"},
    "entries": [{"name": "b", "size": 2}],
}


class FieldSpecTest(unittest.TestCase):
    def test_malformed_field_declarations_reject(self) -> None:
        cases: tuple[tuple[str, dict[str, Any]], ...] = (
            ("bad result field name", {"name": "Bytes"}),
            ("bad result field name", {"name": ""}),
            ("bad field kind", {"kind": "float"}),
            ("required and consumed are bools", {"required": 1}),
            (
                "a json field is diagnostic and never consumed",
                {"kind": FieldKind.JSON},
            ),
            ("an object field declares its members", {"kind": FieldKind.OBJECT}),
            (
                "an object field declares its members",
                {"kind": FieldKind.OBJECT_LIST, "schema": ()},
            ),
            ("only objects have members", {"schema": _MEMBER}),
            (
                "members repeat a field name",
                {"kind": FieldKind.OBJECT, "schema": (_MEMBER[0], _MEMBER[0])},
            ),
        )
        for message, overrides in cases:
            with self.subTest(message=message):
                fields: dict[str, Any] = {"name": "bytes", "kind": FieldKind.TEXT}
                with self.assertRaisesRegex(ValueError, re.escape(message)):
                    FieldSpec(**{**fields, **overrides})

    def test_malformed_schemas_reject(self) -> None:
        errno_field = (FieldSpec("errno", FieldKind.INT),)
        typed = (FieldSpec("error_type", FieldKind.TEXT),)
        cases: tuple[tuple[str, dict[str, Any]], ...] = (
            ("ok fields must be a tuple", {"ok": [FieldSpec("a", FieldKind.TEXT)]}),
            ("is not a FieldSpec", {"fail": ("errno",)}),
            (
                "ok fields repeat a field name",
                {"ok": (FieldSpec("a", FieldKind.TEXT), FieldSpec("a", FieldKind.INT))},
            ),
            (
                "info fields never declare the scheduler's reason",
                {"info": (FieldSpec("reason", FieldKind.TEXT),)},
            ),
            ("unknown errno", {"fail": errno_field, "errnos": {"ENOPE": "state"}}),
            ("bad errno use", {"fail": errno_field, "errnos": {"EIO": "maybe"}}),
            (
                "only ETIMEDOUT is a timeout errno",
                {"fail": errno_field, "errnos": {"EIO": ErrnoUse.TIMEOUT}},
            ),
            (
                "only ETIMEDOUT is a timeout errno",
                {"fail": errno_field, "errnos": {"ETIMEDOUT": ErrnoUse.FAULT}},
            ),
            ("declares a fail errno field", {"errnos": {"EIO": ErrnoUse.FAULT}}),
            (
                "declares a fail errno field",
                {"errnos": {NO_ERRNO: ErrnoUse.STATE, "EIO": ErrnoUse.FAULT}},
            ),
            (
                "only ETIMEDOUT is a timeout errno",
                {"errnos": {NO_ERRNO: ErrnoUse.TIMEOUT}},
            ),
            (
                "a fail errno field is an int",
                {"fail": (FieldSpec("errno", FieldKind.TEXT),)},
            ),
            (
                "bad error type 'busy'",
                {"fail": typed, "error_types": {"busy": "state"}},
            ),
            (
                "an error type is state or fault",
                {"fail": typed, "error_types": {"BUSY": ErrnoUse.TIMEOUT}},
            ),
            (
                "declares a fail error_type text field",
                {"error_types": {"BUSY": ErrnoUse.STATE}},
            ),
            (
                "declares a fail error_type text field",
                {
                    "fail": (FieldSpec("error_type", FieldKind.INT),),
                    "error_types": {"BUSY": ErrnoUse.STATE},
                },
            ),
        )
        for message, fields in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, re.escape(message)):
                    ResultSchema(**fields)

    def test_errnoless_failures_declare_the_no_errno_key(self) -> None:
        """A process or RPC failure carries no errno; `NO_ERRNO` declares it.

        It needs no errno field.
        """
        schema = ResultSchema(
            fail=(FieldSpec("returncode", FieldKind.INT),),
            errnos={NO_ERRNO: ErrnoUse.STATE},
        )
        self.assertEqual(ErrnoUse.STATE, schema.errnos[NO_ERRNO])
        self.assertEqual({NO_ERRNO: "state"}, schema.document()["errnos"])

    def test_error_types_classify_errnoless_failures(self) -> None:
        """A typed errno-less failure names its type; the core reads that field."""
        schema = ResultSchema(
            fail=(FieldSpec("error_type", FieldKind.TEXT, required=False),),
            errnos={NO_ERRNO: ErrnoUse.STATE},
            error_types=cast(dict[str, ErrnoUse], {"BUSY": "state", "LOST": "fault"}),
        )
        self.assertEqual(
            ({"BUSY": "state", "LOST": "fault"}, frozenset({"errno", "error_type"})),
            (schema.document()["error_types"], schema.failure_fields()),
        )
        plain = ResultSchema(errnos={NO_ERRNO: ErrnoUse.STATE})
        self.assertEqual(
            (False, frozenset({"errno"})),
            ("error_types" in plain.document(), plain.failure_fields()),
        )

    def test_schema_document_is_canonical(self) -> None:
        schema = ResultSchema(
            fail=(FieldSpec("errno", FieldKind.INT),),
            errnos=cast(dict[str, ErrnoUse], {"EIO": "fault", "ENOENT": "state"}),
        )
        self.assertEqual(ErrnoUse.FAULT, schema.errnos["EIO"])
        self.assertEqual(
            {
                "ok": [],
                "fail": [
                    {
                        "name": "errno",
                        "kind": "int",
                        "required": True,
                        "consumed": True,
                        "schema": None,
                    }
                ],
                "info": [],
                "errnos": {"EIO": "fault", "ENOENT": "state"},
            },
            schema.document(),
        )
        with self.assertRaises(TypeError):
            cast(dict[str, Any], schema.errnos)["EIO"] = ErrnoUse.STATE


class DecodeResultTest(unittest.TestCase):
    def test_every_kind_decodes_exactly(self) -> None:
        decoded = decode_result(_EVERY_KIND, OK, _LEGAL)
        assert isinstance(decoded, TypedResult)
        self.assertEqual(Outcome.OK, decoded.outcome)
        self.assertEqual(("a", "b"), decoded.get("names"))
        self.assertEqual({"name": "a"}, dict(decoded.get("entry")))
        self.assertEqual(2, decoded.get("entries")[0]["size"])
        digest = decoded.get("digest")
        self.assertEqual(
            ContentDigest(
                _LEGAL["digest"]["bytes_prefix"], 200, _LEGAL["digest"]["sha256"]
            ),
            digest,
        )
        self.assertEqual(hashlib.sha256(b"x" * 200).digest(), digest.key())
        short = ContentDigest(b"hi".hex(), 2, hashlib.sha256(b"hi").hexdigest())
        self.assertEqual(b"hi", short.key())
        self.assertIsNone(decoded.get("trace"))
        traced = decode_result(_EVERY_KIND, OK, {**_LEGAL, "trace": {"any": [1]}})
        assert isinstance(traced, TypedResult)
        self.assertEqual({"any": (1,)}, traced.get("trace"))
        with self.assertRaises(TypeError):
            cast(dict[str, Any], decoded.fields)["text"] = "b"

    def test_failures_and_info_decode_with_their_own_fields(self) -> None:
        failed = decode_result(_EVERY_KIND, FAIL, {"errno": errno.ENOENT})
        assert isinstance(failed, TypedResult)
        self.assertEqual(errno.ENOENT, failed.errno)
        info = decode_result(_EVERY_KIND, INFO, {"reason": "operation-timeout"})
        assert isinstance(info, TypedResult)
        self.assertIsNone(info.errno)
        self.assertEqual("operation-timeout", info.get("reason"))
        bare = decode_result(_EVERY_KIND, INFO, {})
        self.assertIsInstance(bare, TypedResult)

    def test_a_flat_digest_triple_is_checked_like_a_digest(self) -> None:
        """A recorded sha256 that is not its own bytes' digest is a
        recorder problem (`core.result-schema`), not evidence about state."""
        schema = ResultSchema(
            ok=(
                FieldSpec("bytes_prefix", FieldKind.HEX),
                FieldSpec("length", FieldKind.INT),
                FieldSpec("sha256", FieldKind.HEX),
            )
        )
        self.assertIsInstance(decode_result(schema, OK, _digest(b"ab")), TypedResult)
        self.assertIsInstance(
            decode_result(schema, OK, _digest(b"x" * 200)), TypedResult
        )
        bad = decode_result(schema, OK, {**_digest(b"ab"), "sha256": "ab" * 32})
        assert isinstance(bad, ResultProblem)
        self.assertEqual(
            ("result: digest sha256 does not match its whole bytes",), bad.problems
        )

    def test_every_mismatch_is_a_problem(self) -> None:
        cases: dict[str, tuple[str, dict[str, Any], str]] = {
            "unknown field": (
                OK,
                {**_LEGAL, "extra": 1},
                "undeclared fields ['extra']",
            ),
            "missing field": (
                OK,
                {k: v for k, v in _LEGAL.items() if k != "text"},
                "text: required field is missing",
            ),
            "int for text": (OK, {**_LEGAL, "text": 1}, "text: 1 is not text"),
            "bool for int": (OK, {**_LEGAL, "count": True}, "count: True is not int"),
            "int for bool": (OK, {**_LEGAL, "flag": 0}, "flag: 0 is not bool"),
            "odd hex": (OK, {**_LEGAL, "hex": "abc"}, "hex: 'abc' is not hex"),
            "upper hex": (OK, {**_LEGAL, "hex": "AB"}, "hex: 'AB' is not hex"),
            "digest keys": (
                OK,
                {**_LEGAL, "digest": {"length": 0}},
                "digest: digest must have exactly",
            ),
            "digest length": (
                OK,
                {**_LEGAL, "digest": {**_digest(b"ab"), "length": True}},
                "digest length True is not a count",
            ),
            "digest prefix": (
                OK,
                {**_LEGAL, "digest": {**_digest(b"ab"), "length": 3}},
                "prefix does not cover the first bytes",
            ),
            "digest sha": (
                OK,
                {**_LEGAL, "digest": {**_digest(b"ab"), "sha256": "0" * 63}},
                "sha256 is not 64 lowercase hex digits",
            ),
            "digest bytes": (
                OK,
                {**_LEGAL, "digest": {**_digest(b"ab"), "sha256": "0" * 64}},
                "sha256 does not match its whole bytes",
            ),
            "text list": (OK, {**_LEGAL, "names": ["a", 1]}, "names: <list>"),
            "int list": (OK, {**_LEGAL, "numbers": [True]}, "numbers: <list>"),
            "object member": (
                OK,
                {**_LEGAL, "entry": {"size": 1}},
                "entry.name: required field is missing",
            ),
            "object unknown member": (
                OK,
                {**_LEGAL, "entry": {"name": "a", "mode": 1}},
                "entry: undeclared fields ['mode']",
            ),
            "object list item": (
                OK,
                {**_LEGAL, "entries": [{"name": 1}]},
                "entries[0].name: 1 is not text",
            ),
            "object list": (OK, {**_LEGAL, "entries": {}}, "is not a list of objects"),
            "fail extra": (FAIL, {"errno": 2, "stderr": "x"}, "undeclared fields"),
            "info reason": (INFO, {"reason": 3}, "reason: 3 is not text"),
            "non-text keys": (
                OK,
                cast(dict[str, Any], {**_LEGAL, 1: "a"}),
                "undeclared fields [1]",
            ),
        }
        for name, (outcome, raw, message) in cases.items():
            with self.subTest(case=name):
                problem = decode_result(_EVERY_KIND, outcome, raw)
                assert isinstance(problem, ResultProblem)
                self.assertTrue(
                    any(message in item for item in problem.problems), problem.problems
                )
        unknown = decode_result(_EVERY_KIND, "skipped", {})
        self.assertIsInstance(unknown, ResultProblem)


def _definition(kind: str, result: ResultSchema, modeled: bool = True) -> ActionDef:
    return ActionDef(
        kind,
        "input",
        frozenset({"path"}),
        frozenset(),
        {"path": str},
        lambda _spec, _action: None,
        modeled,
        False,
        lambda _rng, _context: None,
        result=result,
        footprint=no_footprint,
        roles=frozenset(),
        binding=Binding.PATH,
        grouping=Grouping.NONE,
    )


_READ = ResultSchema(ok=(FieldSpec("size", FieldKind.INT),))
_TARGET = Target(
    name="facts",
    actions={**CORE_ACTION_DEFS, "read": _definition("read", _READ)},
    sites={},
    properties=(),
    install_witnessed=lambda *_: False,
    environment_fields={},
    make_environment=cast(Any, None),
    make_backend=cast(Any, None),
    red_controls=frozenset(),
)


def _spec(*actions: Action) -> RunSpec:
    return RunSpec(
        seed=1,
        target="facts",
        environment=Environment(target={}),
        budgets=Budgets(30.0, 5.0, 60.0),
        actors=(
            Actor(id="a", actions=tuple(a.id for a in actions[:2])),
            Actor(id="b", actions=tuple(a.id for a in actions[2:])),
        ),
        actions=actions,
    )


def _action(action_id: str, kind: str = "read", **payload: Any) -> Action:
    return Action(id=action_id, kind=kind, deadline=DEADLINE_OPERATION, payload=payload)


class HistoryFactsTest(unittest.TestCase):
    def test_cleanup_time_is_the_final_cleanup_record(self) -> None:
        """The early record of calls active at teardown does not end the run."""
        spec = _spec(_action("r1", path="a"))
        events = [
            Event(INVOKE, 1.0, {"action": "r1", "actor": "a"}),
            Event(INFO, 2.0, {"action": "r1", "actor": "a", "reason": "stopped"}),
            Event(CLEANUP, 3.0, {"active_at_teardown": ["r1"]}),
            Event(
                CLEANUP,
                5.0,
                {"close_error": None, "scheduler": {"status": "harness-failure"}},
            ),
        ]
        self.assertEqual(5.0, HistoryFacts.build(spec, events, _TARGET).cleanup_time)

    def test_build_decodes_every_action_in_spec_order(self) -> None:
        spec = _spec(
            _action("r1", path="a"),
            _action("r2", path="a"),
            _action("r3", path="b"),
            _action("r4", path="b"),
        )
        events = [
            Event(INVOKE, 1.0, {"action": "r1", "actor": "a"}),
            Event(OK, 2.0, {"action": "r1", "actor": "a", "size": 4}),
            Event(INVOKE, 3.0, {"action": "r2", "actor": "a"}),
            Event(OK, 4.0, {"action": "r2", "actor": "a", "size": "4"}),
            Event(INVOKE, 1.5, {"action": "r3", "actor": "b"}),
            Event(INFO, 2.5, {"reason": "daemon-exit", "returncode": 1}),
            Event(INFO, 4.5, {"action": "r3", "actor": "b", "reason": "stopped"}),
            Event(SKIPPED, 5.0, {"action": "r4", "actor": "b"}),
            Event(CLEANUP, 6.0, {"scheduler": {"status": "harness-failure"}}),
        ]
        facts = HistoryFacts.build(spec, events, _TARGET)
        self.assertEqual(["r1", "r2", "r3", "r4"], list(facts.actions))
        first, second, third, fourth = facts.actions.values()
        assert first.result is not None
        self.assertEqual(
            ("a", (1.0, 2.0), Outcome.OK, 4, ()),
            (
                first.actor,
                first.interval,
                first.outcome,
                first.result.get("size"),
                first.problems,
            ),
        )
        self.assertIsNone(second.result)
        self.assertEqual(("size: '4' is not int",), second.problems)
        self.assertEqual(
            ("b", 1.5, 4.5, Outcome.INFO, "primary", None),
            (
                third.actor,
                third.invoked,
                third.completed,
                third.outcome,
                third.side,
                third.reference,
            ),
        )
        self.assertEqual((None, None), (fourth.invoked, fourth.outcome))
        self.assertEqual([INFO, CLEANUP], [event.type for event in facts.run_events])
        self.assertEqual(6.0, facts.cleanup_time)
        self.assertEqual(4, len(facts.actions_of("read")))
        self.assertEqual((), facts.actions_of("write"))
        with self.assertRaises(TypeError):
            cast(dict[str, Any], first.payload)["path"] = "b"
        with self.assertRaises(TypeError):
            cast(dict[str, Any], facts.actions)["r9"] = first
        record = facts.run_events[1]
        self.assertEqual({"status": "harness-failure"}, dict(record.data["scheduler"]))
        with self.assertRaises(TypeError):
            cast(dict[str, Any], record.data["scheduler"])["status"] = "completed"

    def test_build_requires_a_valid_history(self) -> None:
        """HistoryFacts are only built from a history whose grammar holds."""
        spec = _spec(_action("r1", path="a"), _action("r2", path="a"))
        prefix = [Event(INVOKE, 1.0, {"action": "r1", "actor": "a"})]
        unresolved = HistoryFacts.build(spec, prefix, _TARGET).action("r1")
        self.assertEqual((1.0, None), (unresolved.invoked, unresolved.completed))
        cleanup = Event(CLEANUP, 6.0, {"scheduler": {"status": "harness-failure"}})
        cases = {
            "unresolved before cleanup": [*prefix, cleanup],
            "unknown action": [Event(INVOKE, 1.0, {"action": "zz"})],
            "completion without invoke": [Event(OK, 1.0, {"action": "r1"})],
        }
        for name, events in cases.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(ValueError, "need a valid history"):
                    HistoryFacts.build(spec, events, _TARGET)
        with self.assertRaisesRegex(ValueError, "need a valid history"):
            HistoryFacts.build(spec, prefix, _TARGET, execution_status="completed")

    def test_action_facts_sides_pair_only_primary_with_reference(self) -> None:
        primary = ActionFacts("r", "read", "a", {})
        reference = ActionFacts("r", "read", "a", {}, side="reference")
        self.assertIs(reference, replace(primary, reference=reference).reference)
        bad = {
            "unknown side": lambda: ActionFacts("r", "read", "a", {}, side="both"),
            "reference on primary side": lambda: replace(primary, reference=primary),
            "other action": lambda: replace(
                primary, reference=replace(reference, id="x")
            ),
            "reference of a reference": lambda: replace(reference, reference=reference),
        }
        for name, build in bad.items():
            with self.subTest(case=name):
                with self.assertRaises(ValueError):
                    build()

    def test_build_decodes_a_paired_reference_side(self) -> None:
        spec = _spec(_action("r"))
        target = replace(
            _TARGET,
            actions={**_TARGET.actions, "read": _definition("read", paired(_READ))},
            expects_reference=lambda _spec, _action: True,
        )

        def side(**changes: Any) -> dict[str, Any]:
            return {
                "schema_version": 1,
                "outcome": "ok",
                "started": 1.2,
                "completed": 1.8,
                "facts": {"size": 4},
                **changes,
            }

        def build(reference: Any) -> ActionFacts:
            events = [
                ev("invoke", "r", 1.0),
                ev("ok", "r", 2.0, size=3, reference_result=reference),
            ]
            recorded = HistoryFacts.build(spec, events, target).action("r")
            self.assertEqual((), recorded.problems)
            assert recorded.result is not None
            self.assertEqual({"size": 3}, dict(recorded.result.fields))
            assert recorded.reference is not None
            self.assertEqual("reference", recorded.reference.side)
            return recorded.reference

        decoded = build(side())
        assert decoded.result is not None
        self.assertEqual(
            (Outcome.OK, (1.2, 1.8), {"size": 4}, ()),
            (
                decoded.outcome,
                decoded.interval,
                dict(decoded.result.fields),
                decoded.problems,
            ),
        )
        unresolved = build(side(outcome="info", facts={"reason": "reference-timeout"}))
        self.assertEqual((Outcome.INFO, ()), (unresolved.outcome, unresolved.problems))
        problems = {
            "malformed": (side(schema_version=2), "reference result: "),
            "not an object": ("x", "reference result: "),
            "unhashable outcome": (side(outcome=[]), "reference result: "),
            "outside the span": (side(completed=2.5), "outside the primary span"),
            "off its schema": (side(facts={"size": "4"}), "size: '4' is not int"),
        }
        for name, (raw, problem) in problems.items():
            with self.subTest(case=name):
                reference = build(raw)
                self.assertIsNone(reference.result)
                self.assertEqual(1, len(reference.problems))
                self.assertIn(problem, reference.problems[0])
        plain = HistoryFacts.build(
            spec,
            [ev("invoke", "r", 1.0), ev("ok", "r", 2.0, size=3)],
            replace(target, expects_reference=None),
        ).action("r")
        self.assertIsNone(plain.reference)
        unpaired = HistoryFacts.build(
            spec,
            [ev("invoke", "r", 1.0), ev("ok", "r", 2.0, size=3, reference_result=1)],
            _TARGET,
        ).action("r")
        self.assertEqual(
            (None, None, ("result: undeclared fields ['reference_result']",)),
            (unpaired.reference, unpaired.result, unpaired.problems),
        )

    def test_reference_presence_follows_expects_reference(self) -> None:
        """A missing owed side and an unowed side are reference-side problems."""
        spec = _spec(_action("r"))
        side = {
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.2,
            "completed": 1.8,
            "facts": {"size": 4},
        }
        cases = {
            ("owed", "ok", False): (MISSING_REFERENCE,),
            ("owed", "fail", False): (MISSING_REFERENCE,),
            ("owed", "info", False): None,
            ("owed", "ok", True): (),
            ("unowed", "ok", True): (UNEXPECTED_REFERENCE,),
            ("unowed", "fail", True): (UNEXPECTED_REFERENCE,),
            ("unowed", "ok", False): None,
            ("no predicate", "ok", True): (UNEXPECTED_REFERENCE,),
        }
        predicates = {
            "owed": lambda _spec, _action: True,
            "unowed": lambda _spec, _action: False,
            "no predicate": None,
        }
        terminal = {"ok": {"size": 3}, "fail": {"errno": errno.ENOENT}, "info": {}}
        schema = paired(replace(_READ, fail=(FieldSpec("errno", FieldKind.INT),)))
        for (owed, outcome, carried), expected in cases.items():
            with self.subTest(owed=owed, outcome=outcome, carried=carried):
                target = replace(
                    _TARGET,
                    actions={**_TARGET.actions, "read": _definition("read", schema)},
                    expects_reference=predicates[owed],
                )
                facts: dict[str, Any] = dict(terminal[outcome])
                if carried:
                    facts[REFERENCE_RESULT] = {**side, "outcome": outcome}
                recorded = HistoryFacts.build(
                    spec,
                    [ev("invoke", "r", 1.0), ev(outcome, "r", 2.0, **facts)],
                    target,
                ).action("r")
                self.assertEqual((), recorded.problems)
                if expected is None:
                    self.assertIsNone(recorded.reference)
                    continue
                assert recorded.reference is not None
                self.assertEqual(
                    ("reference", expected),
                    (recorded.reference.side, recorded.reference.problems),
                )
                if expected:
                    self.assertIsNone(recorded.reference.result)

    def test_only_paired_declares_the_reference_side(self) -> None:
        schema = paired(_READ)
        self.assertEqual(
            (REFERENCE_SIDE, REFERENCE_SIDE, ()),
            (schema.ok[-1], schema.fail[-1], schema.info),
        )
        for name, build in {
            "judged": lambda: ResultSchema(
                ok=(FieldSpec(REFERENCE_RESULT, FieldKind.TEXT, False),)
            ),
            "on info": lambda: ResultSchema(info=(REFERENCE_SIDE,)),
        }.items():
            with self.subTest(case=name):
                with self.assertRaisesRegex(ValueError, "only by `paired`"):
                    build()

    def test_unknown_kinds_and_fault_windows(self) -> None:
        spec = _spec(
            _action(
                "f",
                "fault_install",
                fault_id="F",
                site="s",
                target_action="r",
                behavior="block",
            ),
            _action("r", path="a"),
            _action("x", "mystery"),
        )
        events = [
            Event(INVOKE, 1.0, {"action": "f"}),
            Event(OK, 2.0, {"action": "f", "fault_id": "F"}),
            Event(INVOKE, 3.0, {"action": "r"}),
            Event(OK, 4.0, {"action": "r", "size": 1}),
            Event(INVOKE, 3.0, {"action": "x"}),
            Event(OK, 4.0, {"action": "x"}),
        ]
        target = replace(
            _TARGET,
            actions={**_TARGET.actions, "mystery": _definition("mystery", _READ)},
        )
        facts = HistoryFacts.build(spec, events, target)
        ((install, window),) = facts.fault_windows()
        self.assertEqual(("f", (1.0, float("inf"))), (install.id, window))
        self.assertEqual(float("inf"), facts.cleanup_time)
        unknown = HistoryFacts.build(spec, events, _TARGET).action("x")
        self.assertEqual(("unknown action kind 'mystery'",), unknown.problems)


def _nested(levels: int) -> FieldSpec:
    """Return an object field nesting `levels` object levels."""
    spec = FieldSpec("leaf", FieldKind.INT)
    for _ in range(levels):
        spec = FieldSpec("node", FieldKind.OBJECT, schema=(spec,))
    return spec


def _nested_value(levels: int) -> Any:
    value: Any = 1
    for _ in range(levels):
        value = {"node" if not isinstance(value, int) else "leaf": value}
    return value


class SchemaRegressionTest(unittest.TestCase):
    """Schema and decode contracts."""

    def test_info_fields_are_never_required(self) -> None:
        with self.assertRaisesRegex(ValueError, "info fields are optional"):
            ResultSchema(info=(FieldSpec("partial", FieldKind.INT),))
        optional = ResultSchema(info=(FieldSpec("partial", FieldKind.INT, False),))
        stopped = decode_result(optional, INFO, {"reason": "operation-timeout"})
        assert isinstance(stopped, TypedResult)
        self.assertEqual({"reason": "operation-timeout"}, dict(stopped.fields))

    def test_json_fields_are_read_only_copies(self) -> None:
        schema = ResultSchema(
            ok=(FieldSpec("trace", FieldKind.JSON, required=False, consumed=False),)
        )
        raw: dict[str, Any] = {"trace": {"k": [1]}}
        decoded = decode_result(schema, OK, raw)
        assert isinstance(decoded, TypedResult)
        trace = decoded.get("trace")
        with self.assertRaises(TypeError):
            trace["k"] = 2
        with self.assertRaises(AttributeError):
            trace["k"].append(2)
        raw["trace"]["k"].append(3)
        self.assertEqual((1,), trace["k"])

    def test_decode_never_raises_on_any_outcome_or_raw(self) -> None:
        cases: dict[str, tuple[Any, Any]] = {
            "unhashable outcome": (["ok"], {}),
            "unknown outcome": ("done", {}),
            "non-text outcome": (1, {}),
            "none raw": (OK, None),
            "list raw": (OK, [("text", "a")]),
        }
        for name, (outcome, raw) in cases.items():
            with self.subTest(case=name):
                decoded = decode_result(_EVERY_KIND, outcome, raw)
                self.assertIsInstance(decoded, ResultProblem)

    def test_declared_nesting_is_bounded_so_every_schema_decodes(self) -> None:
        deepest = ResultSchema(ok=(_nested(8),))
        self.assertEqual(8, deepest.ok[0].depth)
        decoded = decode_result(deepest, OK, {"node": _nested_value(8)})
        self.assertIsInstance(decoded, TypedResult, decoded)
        too_deep = decode_result(deepest, OK, {"node": _nested_value(9)})
        self.assertIsInstance(too_deep, ResultProblem)
        with self.assertRaisesRegex(ValueError, "objects nest more than 8 levels"):
            _nested(9)
