# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import hashlib
import re
import unittest
from dataclasses import replace
from typing import Any, cast

from eden.integration.correctness.exploratory.core.effects import no_footprint
from eden.integration.correctness.exploratory.core.facts import (
    EMPTY_RESULT,
    ErrnoUse,
    FieldKind,
    FieldSpec,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.jsonio import MAX_SCALAR_BYTES
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    CHECKOUT_SUBJECT,
    control_rules,
    CoverageObligation,
    DAEMON_SUBJECT,
    Finding,
    identity_digest,
    identity_witnesses,
    ObligationResult,
    ObligationStatus,
    path_subject,
    property_error,
    PropertyDef,
    PropertyResult,
    RuleDef,
    SOLVER_UNAVAILABLE,
    Subject,
    SubjectKind,
    validate_registry,
    validate_result,
    validate_subject,
    Witness,
)
from eden.integration.correctness.exploratory.core.target import (
    ActionDef,
    Binding,
    DEFAULT_SEMANTICS_PROFILE,
    Grouping,
    Role,
    Target,
    TargetSources,
)


def _never(*_: Any) -> bool:
    return False


def _unused(*_: Any) -> Any:
    raise AssertionError("construction tests do not instantiate target resources")


class TargetTest(unittest.TestCase):
    def test_target_requires_install_witnessed(self) -> None:
        """Core calls `install_witnessed` directly, so a target supplies it."""
        fields: dict[str, Any] = {
            "name": "t",
            "actions": {},
            "sites": {},
            "properties": (),
            "environment_fields": {},
            "make_environment": _unused,
            "make_backend": _unused,
            "red_controls": frozenset(),
        }
        with self.assertRaisesRegex(TypeError, "install_witnessed"):
            Target(**fields)
        self.assertIs(
            _never, Target(**fields, install_witnessed=_never).install_witnessed
        )


def _collect(*_: Any) -> None:
    pass


_PATH = Subject(SubjectKind.PATH, "path:file")


def _rule(rule_id: str, **overrides: Any) -> RuleDef:
    fields: dict[str, Any] = {
        "id": rule_id,
        "statement": "an acknowledged write is visible to every later read",
        "subject_kind": SubjectKind.PATH,
    }
    return RuleDef(**{**fields, **overrides})


def _property(**overrides: Any) -> PropertyDef:
    fields: dict[str, Any] = {
        "id": "bytes",
        "collect": _collect,
        "rules": {
            "bytes.stale-read": _rule(
                "bytes.stale-read",
                identity_roles=("channel",),
                red_controls=frozenset({"torn-write"}),
            ),
            "bytes.torn-read": _rule("bytes.torn-read"),
        },
        "coverage": (CoverageObligation("bytes.write-observed", "a write completed"),),
        "not_evaluated_reasons": {"bytes.no-initial": "the initial bytes are unknown"},
        "consumes": frozenset({("read", "bytes")}),
    }
    return PropertyDef(**{**fields, **overrides})


def _obligation(**overrides: Any) -> ObligationResult:
    fields: dict[str, Any] = {
        "rule_id": "bytes.stale-read",
        "scope": "r",
        "subject": _PATH,
        "status": ObligationStatus.SATISFIED,
        "witnesses": (Witness("channel", "fuse"),),
    }
    return ObligationResult(**{**fields, **overrides})


def _violation(**overrides: Any) -> ObligationResult:
    fields: dict[str, Any] = {
        "status": ObligationStatus.VIOLATED,
        "attribution": Attribution.PRODUCT,
        "reason": "bytes/fuse≠model: stale-read: r returned overwritten bytes",
        "witnesses": (Witness("channel", "fuse"), Witness("read-action", "r")),
    }
    return _obligation(**{**fields, **overrides})


def _result(**overrides: Any) -> PropertyResult:
    fields: dict[str, Any] = {"property_id": "bytes", "obligations": (_obligation(),)}
    return PropertyResult(**{**fields, **overrides})


def _finding(rule_id: str = "bytes.stale-read", **overrides: Any) -> Finding:
    fields: dict[str, Any] = {
        "rule_id": rule_id,
        "attribution": Attribution.PRODUCT,
        "reason": "bytes/fuse≠model: stale-read: r returned overwritten bytes",
        "subject": _PATH,
        "witnesses": (Witness("channel", "fuse"), Witness("read-action", "r")),
    }
    return Finding(**{**fields, **overrides})


class RuleDefinitionTest(unittest.TestCase):
    def test_valid_rule_table_constructs(self) -> None:
        definition = _property()
        self.assertEqual(
            {"torn-write": frozenset({"bytes.stale-read"})}, control_rules([definition])
        )
        validate_registry([definition], frozenset({"torn-write"}))
        self.assertEqual(
            "stale-read", definition.rules["bytes.stale-read"].finding_kind
        )

    def test_malformed_declarations_reject(self) -> None:
        def alone(rule: RuleDef) -> dict[str, Any]:
            return {"rules": {rule.id: rule}}

        cases: tuple[tuple[str, dict[str, Any]], ...] = (
            ("bad property id", {"id": "Bytes"}),
            ("is reserved", {"id": "execution"}),
            ("is reserved", {"id": "lifecycle"}),
            ("core property 'core.' must be core.<name>", {"id": "core."}),
            ("declares no rules", {"rules": {}}),
            ("names 'bytes.other'", {"rules": {"bytes.stale": _rule("bytes.other")}}),
            ("is outside its property", alone(_rule("status.stale"))),
            ("rule 'bytes' is outside its property", alone(_rule("bytes"))),
            ("empty statement", alone(_rule("bytes.s", statement=" "))),
            (
                "exceeds 4096 UTF-8 bytes",
                alone(_rule("bytes.s", statement="é" * 2049)),
            ),
            ("bad rule id", alone(_rule("bytes." + "x" * 128))),
            ("bad subject kind", alone(_rule("bytes.s", subject_kind="file"))),
            (
                "repeated identity role",
                alone(_rule("bytes.s", identity_roles=("a", "a"))),
            ),
            (
                "identity roles are a tuple",
                alone(_rule("bytes.s", identity_roles=["a"])),
            ),
            (
                "bad bytes.s: identity role",
                alone(_rule("bytes.s", identity_roles=("A",))),
            ),
            (
                "bad bytes.s: red control",
                alone(_rule("bytes.s", red_controls=frozenset({"T"}))),
            ),
            (
                "repeated obligation",
                {
                    "coverage": (
                        CoverageObligation("bytes.write-observed", "a"),
                        CoverageObligation("bytes.write-observed", "b"),
                    )
                },
            ),
            ("must be bytes.<name>", {"coverage": (CoverageObligation("read", "a"),)}),
            (
                "not-evaluated reason 'no-initial'",
                {"not_evaluated_reasons": {"no-initial": "x"}},
            ),
            (
                "not-evaluated reason 'status.gone'",
                {"not_evaluated_reasons": {"status.gone": "x"}},
            ),
            (
                "rule 'core.other' is outside its property",
                {
                    "id": "core.activity",
                    "rules": {"core.other": _rule("core.other")},
                    "coverage": (),
                    "not_evaluated_reasons": {},
                },
            ),
            ("collect is not callable", {"collect": None}),
            ("consumes is a frozenset", {"consumes": {("read", "bytes")}}),
            ("bad consumed pair", {"consumes": frozenset({("read",)})}),
            ("bad consumed pair", {"consumes": frozenset({("read", "")})}),
        )
        for message, overrides in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, re.escape(message)):
                    _property(**overrides)

    def test_core_property_owns_its_own_rule(self) -> None:
        core = _property(
            id="core.activity",
            rules={"core.activity": _rule("core.activity")},
            coverage=(),
            not_evaluated_reasons={"core.activity.incomplete": "x"},
        )
        self.assertEqual(("core.activity",), tuple(core.rules))
        self.assertEqual("activity", core.rules["core.activity"].finding_kind)

    def test_registry_rejects_repeats_and_control_drift(self) -> None:
        definition = _property()
        with self.assertRaisesRegex(ValueError, "not the union"):
            validate_registry([definition], frozenset())
        with self.assertRaisesRegex(ValueError, "not the union"):
            validate_registry([definition], frozenset({"torn-write", "extra"}))
        with self.assertRaisesRegex(ValueError, "repeated property id"):
            validate_registry([definition, definition], frozenset({"torn-write"}))

    def test_control_union_is_many_to_one(self) -> None:
        """One control may trip rules in several properties; injectivity is not required."""
        status = _property(
            id="status",
            rules={
                "status.lost": _rule(
                    "status.lost", red_controls=frozenset({"torn-write"})
                )
            },
            coverage=(),
            not_evaluated_reasons={},
        )
        self.assertEqual(
            {"torn-write": frozenset({"bytes.stale-read", "status.lost"})},
            control_rules([_property(), status]),
        )


class IdentityDigestTest(unittest.TestCase):
    def test_digest_tracks_identity_fields_and_ignores_prose_order_and_code(
        self,
    ) -> None:
        base = _property()
        status = _property(
            id="status",
            rules={"status.lost": _rule("status.lost")},
            coverage=(),
            not_evaluated_reasons={},
        )
        digest = identity_digest([base, status], "p-v1")
        self.assertRegex(digest, r"^sha256:[0-9a-f]{64}$")
        self.assertEqual(digest, identity_digest([status, base], "p-v1"))
        recoded = replace(base, collect=lambda *_: None, coverage=())
        self.assertEqual(digest, identity_digest([recoded, status], "p-v1"))
        rule = base.rules["bytes.stale-read"]
        reworded = replace(rule, statement="a reworded guarantee")
        self.assertEqual(
            digest,
            identity_digest(
                [replace(base, rules={**base.rules, rule.id: reworded}), status], "p-v1"
            ),
            "prose is not identity",
        )
        self.assertNotEqual(digest, identity_digest([base, status], "p-v2"))
        changes: dict[str, Any] = {
            "subject_kind": SubjectKind.DESCRIPTOR,
            "identity_roles": ("channel", "errno"),
            "red_controls": frozenset({"torn-write", "status-loss"}),
        }
        for name, value in changes.items():
            with self.subTest(field=name):
                changed = replace(rule, **{name: value})
                rules = {**base.rules, rule.id: changed}
                self.assertNotEqual(
                    digest,
                    identity_digest([replace(base, rules=rules), status], "p-v1"),
                )
        renamed = replace(rule, id="bytes.stale")
        rules = {
            "bytes.stale": renamed,
            "bytes.torn-read": base.rules["bytes.torn-read"],
        }
        self.assertNotEqual(
            digest, identity_digest([replace(base, rules=rules), status], "p-v1")
        )


class SubjectAndWitnessTest(unittest.TestCase):
    def test_canonical_subjects(self) -> None:
        for subject in (
            _PATH,
            Subject(SubjectKind.PATH, "path:dir/file"),
            Subject(SubjectKind.DESCRIPTOR, "descriptor:fd0"),
            CHECKOUT_SUBJECT,
            Subject(SubjectKind.CHECKOUT, "checkout:c1"),
            DAEMON_SUBJECT,
        ):
            with self.subTest(subject=subject):
                validate_subject(subject)
        for subject, message in (
            (Subject(SubjectKind.PATH, "file"), "must be path:<alias>"),
            (Subject(SubjectKind.PATH, "descriptor:fd0"), "must be path:<alias>"),
            (Subject(SubjectKind.PATH, "path:/tmp/x"), "must be path:<alias>"),
            (Subject(SubjectKind.PATH, "path:a b"), "must be path:<alias>"),
            (Subject(SubjectKind.PATH, "path:"), "must be path:<alias>"),
            (Subject(SubjectKind.CHECKOUT, "path:c1"), "must be checkout:<alias>"),
            (Subject(SubjectKind.DAEMON, "daemon:1234"), "must be 'daemon:all'"),
            (Subject(cast(Any, "file"), "file:x"), "bad subject"),
        ):
            with self.subTest(subject=subject):
                with self.assertRaisesRegex(ValueError, re.escape(message)):
                    validate_subject(subject)

    def test_identity_witnesses_need_every_declared_role_once(self) -> None:
        rule = _rule("bytes.s", identity_roles=("errno", "channel"))
        finding = _finding(
            witnesses=(
                Witness("read-action", "r1"),
                Witness("errno", "EIO"),
                Witness("channel", "fuse"),
            )
        )
        self.assertEqual(
            (("channel", "fuse"), ("errno", "EIO")), identity_witnesses(rule, finding)
        )
        renamed = replace(
            finding, witnesses=(*finding.witnesses[1:], Witness("read-action", "r9"))
        )
        self.assertEqual(
            identity_witnesses(rule, finding), identity_witnesses(rule, renamed)
        )
        missing = replace(finding, witnesses=finding.witnesses[:2])
        conflicting = replace(
            finding, witnesses=(*finding.witnesses, Witness("channel", "thrift"))
        )
        self.assertIsNone(identity_witnesses(rule, missing))
        self.assertIsNone(identity_witnesses(rule, conflicting))
        self.assertEqual((), identity_witnesses(_rule("bytes.t"), finding))


class PropertyResultTest(unittest.TestCase):
    def test_findings_are_derived_from_violated_obligations(self) -> None:
        result = _result(
            obligations=(
                _obligation(scope="a"),
                _violation(scope="b"),
                _obligation(
                    scope="c",
                    status=ObligationStatus.NOT_EVALUATED,
                    not_evaluated_reason="bytes.no-initial",
                ),
            )
        )
        validate_result(_property(), result)
        (finding,) = result.findings
        self.assertEqual(
            ("bytes.stale-read", Attribution.PRODUCT, _PATH),
            (finding.rule_id, finding.attribution, finding.subject),
        )
        self.assertTrue(result.complete)
        self.assertFalse(_result(error=property_error("E", "m")).complete)
        with self.assertRaisesRegex(ValueError, "not a violation"):
            _obligation().finding()

    def test_legal_obligation_shapes(self) -> None:
        legal = (
            _obligation(),
            _violation(),
            _violation(attribution=Attribution.HARNESS, proof="p1", component="c1"),
            _obligation(component="c1"),
            _obligation(
                status=ObligationStatus.NOT_EVALUATED,
                not_evaluated_reason="bytes.no-initial",
                reason="no initial bytes for file",
            ),
            _obligation(
                status=ObligationStatus.NOT_EVALUATED,
                not_evaluated_reason=SOLVER_UNAVAILABLE,
            ),
        )
        for obligation in legal:
            with self.subTest(obligation=obligation):
                validate_result(_property(), _result(obligations=(obligation,)))

    def test_result_must_agree_with_its_property(self) -> None:
        definition = _property()
        not_evaluated = ObligationStatus.NOT_EVALUATED
        cases: tuple[tuple[str, PropertyResult], ...] = (
            ("result for 'status'", _result(property_id="status")),
            ("undeclared coverage", _result(coverage={"bytes.read-observed": 1})),
            ("bad coverage count", _result(coverage={"bytes.write-observed": -1})),
            (
                "unknown rule",
                _result(obligations=(_obligation(rule_id="bytes.gone"),)),
            ),
            ("bad obligation scope", _result(obligations=(_obligation(scope=""),))),
            (
                "bad obligation scope",
                _result(obligations=(_obligation(scope="x" * 129),)),
            ),
            (
                "exactly a violated obligation carries an attribution",
                _result(obligations=(_violation(attribution=None),)),
            ),
            (
                "exactly a violated obligation carries an attribution",
                _result(obligations=(_obligation(attribution=Attribution.PRODUCT),)),
            ),
            (
                "bad attribution",
                _result(obligations=(_violation(attribution="model-suspect"),)),
            ),
            (
                "reason must be a string",
                _result(obligations=(_violation(reason=None),)),
            ),
            (
                "exceeds 4096 UTF-8 bytes",
                _result(obligations=(_violation(reason="x" * 4097),)),
            ),
            (
                "a satisfied obligation carries no reason",
                _result(obligations=(_obligation(reason="x"),)),
            ),
            (
                "only a violation has a proof",
                _result(obligations=(_obligation(proof="p1"),)),
            ),
            (
                "exactly a not-evaluated obligation carries a reason ID",
                _result(obligations=(_obligation(status=not_evaluated),)),
            ),
            (
                "undeclared not-evaluated reason",
                _result(
                    obligations=(
                        _obligation(
                            status=not_evaluated,
                            not_evaluated_reason="bytes.unknown",
                        ),
                    )
                ),
            ),
            (
                "bad not-evaluated reason",
                _result(
                    obligations=(
                        _obligation(status=not_evaluated, not_evaluated_reason="Bad"),
                    )
                ),
            ),
            (
                "is not a path",
                _result(obligations=(_obligation(subject=DAEMON_SUBJECT),)),
            ),
            (
                "bad witness role",
                _result(
                    obligations=(_obligation(witnesses=(Witness("Channel", "x"),)),)
                ),
            ),
            (
                "unique and sorted",
                _result(obligations=(_obligation(scope="b"), _obligation(scope="a"))),
            ),
            (
                "unique and sorted",
                _result(obligations=(_obligation(), _violation())),
            ),
            ("bad status", _result(obligations=(_obligation(status="maybe"),))),
        )
        for message, result in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, re.escape(message)):
                    validate_result(definition, result)


def _action_def(**overrides: Any) -> ActionDef:
    fields: dict[str, Any] = {
        "kind": "write",
        "axis": "input",
        "fields": frozenset({"path"}),
        "required": frozenset({"path"}),
        "types": {"path": str},
        "validate": _never,
        "modeled": True,
        "spawns_process": False,
        "sample": _never,
        "result": EMPTY_RESULT,
        "footprint": no_footprint,
        "roles": frozenset({Role.PRODUCE}),
        "binding": Binding.PATH,
        "grouping": Grouping.SINGLE,
    }
    return ActionDef(**{**fields, **overrides})


class _DuckSchema:
    """Looks like a ResultSchema to duck typing, and is not one."""

    def fields(self, _outcome: str) -> tuple[()]:
        return ()

    def document(self) -> dict[str, Any]:
        return {}


class ActionDefTest(unittest.TestCase):
    def test_static_declarations_are_keyword_only_and_required(self) -> None:
        with self.assertRaisesRegex(TypeError, "result"):
            cast(Any, ActionDef)(
                "k", "input", frozenset(), frozenset(), {}, _never, False, False, _never
            )
        observe = _action_def(
            kind="read", roles=frozenset({Role.OBSERVE}), grouping=Grouping.NONE
        )
        self.assertEqual(Binding.PATH, observe.binding)
        mutate = _action_def(
            roles=frozenset({Role.TRANSFER, Role.CAN_SUPERSEDE}),
            grouping=Grouping.ATOMIC,
        )
        self.assertIn(Role.CAN_SUPERSEDE, mutate.roles)

    def test_contradictory_declarations_reject(self) -> None:
        cases: tuple[tuple[str, dict[str, Any]], ...] = (
            ("footprint is not callable", {"footprint": None}),
            ("roles must be a frozenset of Role", {"roles": {Role.PRODUCE}}),
            ("roles must be a frozenset of Role", {"roles": frozenset({"write"})}),
            ("bad binding", {"binding": "inode"}),
            ("bad grouping", {"grouping": "batched"}),
            (
                "groups its effects exactly when it mutates state",
                {"grouping": Grouping.NONE},
            ),
            (
                "groups its effects exactly when it mutates state",
                {"roles": frozenset({Role.OBSERVE})},
            ),
            (
                "only a mutating action can supersede",
                {
                    "roles": frozenset({Role.OBSERVE, Role.CAN_SUPERSEDE}),
                    "grouping": Grouping.NONE,
                },
            ),
            ("result is not a ResultSchema", {"result": object()}),
            ("result is not a ResultSchema", {"result": _DuckSchema()}),
            (
                "an action with roles binds its effects to something",
                {"binding": Binding.NONE},
            ),
            (
                "a single group binds one path or descriptor",
                {"binding": Binding.AGGREGATE},
            ),
            (
                "an aggregate group binds an aggregate",
                {"grouping": Grouping.AGGREGATE},
            ),
            ("input_subject is not callable", {"input_subject": "source"}),
            (
                "a modeled descriptor input declares input_subject",
                {"binding": Binding.DESCRIPTOR},
            ),
            (
                "a modeled none input declares input_subject",
                {
                    "binding": Binding.NONE,
                    "roles": frozenset(),
                    "grouping": Grouping.NONE,
                },
            ),
        )
        for message, overrides in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, re.escape(message)):
                    _action_def(**overrides)

    def test_only_path_or_aggregate_inputs_may_omit_input_subject(self) -> None:
        """A modeled input declares the path it is about unless its binding names it."""
        descriptor = {"binding": Binding.DESCRIPTOR}
        self.assertIsNotNone(
            _action_def(**descriptor, input_subject=_source).input_subject
        )
        for overrides in (
            {"modeled": False},
            {"axis": "observer"},
            {"binding": Binding.PATH},
            {"binding": Binding.AGGREGATE, "grouping": Grouping.ATOMIC},
        ):
            with self.subTest(overrides=overrides):
                self.assertIsNone(
                    _action_def(**{**descriptor, **overrides}).input_subject
                )


def _source(_spec: Any, action: Any) -> str:
    return action.payload["source"]


class PathSubjectTest(unittest.TestCase):
    def test_every_canonical_path_has_a_valid_distinct_alias(self) -> None:
        """Whitespace and '%' are percent-encoded, so no path breaks a subject."""
        cases = {
            "": "path:.",
            "a/b": "path:a/b",
            "dir with space/f": "path:dir%20with%20space/f",
            "dir%20with%20space/f": "path:dir%2520with%2520space/f",
            "tab\tand\nnewline": "path:tab%09and%0Anewline",
            "nbsp\u00a0x": "path:nbsp%C2%A0x",
            "100%": "path:100%25",
        }
        for path, token in cases.items():
            with self.subTest(path=path):
                subject = path_subject(path)
                self.assertEqual(Subject(SubjectKind.PATH, token), subject)
                validate_subject(subject)
        long = " " * MAX_SCALAR_BYTES
        digest = hashlib.sha256(long.encode()).hexdigest()
        self.assertEqual(f"path:%sha256:{digest}", path_subject(long).token)
        validate_subject(path_subject(long))
        with self.assertRaisesRegex(ValueError, "must be path:<alias>"):
            validate_subject(Subject(SubjectKind.PATH, "path:dir with space/f"))


class ConsumptionTest(unittest.TestCase):
    _FIELDS: dict[str, Any] = {
        "name": "t",
        "sites": {},
        "install_witnessed": _never,
        "environment_fields": {},
        "make_environment": _unused,
        "make_backend": _unused,
        "red_controls": frozenset({"torn-write"}),
    }

    def _target(self, read_result: ResultSchema, *properties: PropertyDef) -> Target:
        read = _action_def(
            kind="read",
            roles=frozenset({Role.OBSERVE}),
            grouping=Grouping.NONE,
            result=read_result,
        )
        return Target(
            **self._FIELDS,
            actions={"read": read},
            properties=properties or (_property(),),
        )

    def test_every_consumed_field_is_read(self) -> None:
        schema = ResultSchema(
            ok=(
                FieldSpec("bytes", FieldKind.HEX),
                FieldSpec("trace", FieldKind.JSON, consumed=False),
            ),
            fail=(FieldSpec("errno", FieldKind.INT),),
            errnos={"ENOENT": ErrnoUse.STATE},
        )
        self.assertIn("read", self._target(schema).actions)
        unread = ResultSchema(
            ok=(FieldSpec("bytes", FieldKind.HEX), FieldSpec("size", FieldKind.INT))
        )
        with self.assertRaisesRegex(ValueError, r"no property consumes .*'size'"):
            self._target(unread)
        missing = ResultSchema(ok=(FieldSpec("size", FieldKind.INT, consumed=False),))
        with self.assertRaisesRegex(ValueError, "consume undeclared result fields"):
            self._target(missing)

    def test_contract_reads_and_unported_fields_are_declared(self) -> None:
        """A field the contracts hook reads, or one reported unported, needs no property."""
        schema = ResultSchema(
            ok=(FieldSpec("bytes", FieldKind.HEX), FieldSpec("size", FieldKind.INT)),
            fail=(FieldSpec("errno", FieldKind.INT), FieldSpec("code", FieldKind.INT)),
        )
        read = _action_def(
            kind="read",
            roles=frozenset({Role.OBSERVE}),
            grouping=Grouping.NONE,
            result=schema,
        )

        def target(**fields: Any) -> Target:
            return Target(
                **self._FIELDS,
                actions={"read": read},
                properties=(_property(),),
                **fields,
            )

        with self.assertRaisesRegex(ValueError, r"no property consumes .*'code'"):
            target(unported_fields={("read", "size"): "ported later"})
        accepted = target(
            contract_consumes=frozenset({("read", "code")}),
            unported_fields={("read", "size"): "ported later"},
        )
        self.assertEqual({("read", "size"): "ported later"}, accepted.unported_fields)
        with self.assertRaisesRegex(ValueError, "read yet reported unported"):
            target(
                contract_consumes=frozenset({("read", "code")}),
                unported_fields={("read", "size"): "x", ("read", "bytes"): "x"},
            )
        with self.assertRaisesRegex(ValueError, "names no reason"):
            target(
                contract_consumes=frozenset({("read", "code")}),
                unported_fields={("read", "size"): " "},
            )
        with self.assertRaisesRegex(ValueError, "undeclared fields"):
            target(
                contract_consumes=frozenset({("read", "code"), ("read", "nope")}),
                unported_fields={("read", "size"): "x"},
            )

    def test_nested_members_are_consumed_by_dotted_path(self) -> None:
        """A consumed object's consumed members each need a consumer."""
        members = (
            FieldSpec("name", FieldKind.TEXT),
            FieldSpec("size", FieldKind.INT),
            FieldSpec("note", FieldKind.TEXT, consumed=False),
        )
        schema = ResultSchema(
            ok=(
                FieldSpec("bytes", FieldKind.HEX),
                FieldSpec("entries", FieldKind.OBJECT_LIST, schema=members),
            )
        )
        reads = {("read", "bytes"), ("read", "entries.name")}
        with self.assertRaisesRegex(
            ValueError, r"no property consumes .*entries\.size"
        ):
            self._target(schema, _property(consumes=frozenset(reads)))
        complete = _property(consumes=frozenset({*reads, ("read", "entries.size")}))
        self.assertIn("read", self._target(schema, complete).actions)
        with self.assertRaisesRegex(ValueError, "consume undeclared result fields"):
            self._target(
                schema,
                _property(
                    consumes=frozenset(
                        {*reads, ("read", "entries.size"), ("read", "entries.gone")}
                    )
                ),
            )
        diagnostic = ResultSchema(
            ok=(
                FieldSpec("bytes", FieldKind.HEX),
                FieldSpec(
                    "entries", FieldKind.OBJECT_LIST, consumed=False, schema=members
                ),
            )
        )
        self.assertIn("read", self._target(diagnostic).actions)

    def test_core_reads_only_a_modeled_inputs_fail_errno(self) -> None:
        """`errno` is exempt only on a modeled input's fail outcome."""
        errno_field = FieldSpec("errno", FieldKind.INT, required=False)
        fail_only = ResultSchema(
            ok=(FieldSpec("bytes", FieldKind.HEX),), fail=(errno_field,)
        )
        self.assertIn("read", self._target(fail_only).actions)
        on_ok = ResultSchema(
            ok=(FieldSpec("bytes", FieldKind.HEX), errno_field), fail=(errno_field,)
        )
        with self.assertRaisesRegex(ValueError, r"no property consumes .*'errno'"):
            self._target(on_ok)
        observer = _action_def(
            kind="read",
            axis="observer",
            roles=frozenset({Role.OBSERVE}),
            grouping=Grouping.NONE,
            result=fail_only,
        )
        with self.assertRaisesRegex(ValueError, r"no property consumes .*'errno'"):
            Target(
                **self._FIELDS, actions={"read": observer}, properties=(_property(),)
            )

    def test_recording_only_targets_skip_the_gate(self) -> None:
        unread = ResultSchema(ok=(FieldSpec("size", FieldKind.INT),))
        fields: dict[str, Any] = {**self._FIELDS, "red_controls": frozenset()}
        recording = Target(
            **fields,
            actions={"read": _action_def(kind="read", result=unread)},
            properties=(),
        )
        self.assertEqual((), recording.properties)

    def test_an_unmodeled_kinds_unread_field_fails_validation(self) -> None:
        """The gate covers every kind: an unmodeled one's fields are read or declared."""
        unread = ResultSchema(ok=(FieldSpec("size", FieldKind.INT),))
        quiet = _property(consumes=frozenset())

        def target(result: ResultSchema, **fields: Any) -> Target:
            action = _action_def(kind="read", modeled=False, result=result)
            return Target(
                **self._FIELDS, actions={"read": action}, properties=(quiet,), **fields
            )

        with self.assertRaisesRegex(ValueError, r"no property consumes .*'size'"):
            target(unread)
        self.assertFalse(
            target(unread, unported_fields={("read", "size"): "why"})
            .actions["read"]
            .modeled
        )
        diagnostic = ResultSchema(
            ok=(FieldSpec("size", FieldKind.INT, consumed=False),)
        )
        self.assertIn("read", target(diagnostic).actions)
        # The core classifies only a modeled input's failure.
        errno_only = ResultSchema(fail=(FieldSpec("errno", FieldKind.INT),))
        with self.assertRaisesRegex(ValueError, r"no property consumes .*'errno'"):
            target(errno_only)

    def test_actions_are_keyed_by_their_kind(self) -> None:
        with self.assertRaisesRegex(ValueError, "is not its ActionDef"):
            Target(
                **self._FIELDS,
                actions={"other": _action_def()},
                properties=(_property(consumes=frozenset()),),
            )


class ServingPathTest(unittest.TestCase):
    _PATHS = {"fuse": "kernel", "thrift": "daemon-api", "sl": "client", "cli": "client"}

    def test_target_declares_profile_and_serving_paths(self) -> None:
        fields: dict[str, Any] = {
            "name": "t",
            "actions": {},
            "sites": {},
            "properties": (),
            "install_witnessed": _never,
            "environment_fields": {},
            "make_environment": _unused,
            "make_backend": _unused,
            "red_controls": frozenset(),
        }
        target = Target(**fields)
        self.assertEqual(DEFAULT_SEMANTICS_PROFILE, target.semantics_profile)
        self.assertEqual({}, target.serving_paths)
        declared = Target(**fields, semantics_profile="t-v1", serving_paths=self._PATHS)
        self.assertEqual("client", declared.serving_paths["sl"])
        cases: tuple[tuple[dict[str, Any], str], ...] = (
            ({"semantics_profile": "T V1"}, "bad semantics profile"),
            ({"serving_paths": {"Fuse": "kernel"}}, "bad serving-path observer"),
            ({"serving_paths": {"fuse": ""}}, "bad serving-path group"),
        )
        for overrides, message in cases:
            with self.subTest(overrides=overrides):
                with self.assertRaisesRegex(ValueError, message):
                    Target(**fields, **overrides)


class TargetSourcesTest(unittest.TestCase):
    def test_well_formed_declarations_are_accepted(self) -> None:
        sources = TargetSources(
            implementation="eden.integration.correctness.exploratory.fake.target",
            modules=("eden.integration.correctness.exploratory.fake.tables",),
            packages=("helper_pkg",),
            external=("eden.integration.lib", "thrift"),
            dynamic=("eden.integration.correctness.exploratory.fake.plugin",),
        )
        self.assertEqual(("helper_pkg",), sources.packages)
        target = Target(
            name="t",
            actions={},
            sites={},
            properties=(),
            install_witnessed=_never,
            environment_fields={},
            make_environment=_unused,
            make_backend=_unused,
            red_controls=frozenset(),
            sources=sources,
        )
        self.assertIs(sources, target.sources)

    def test_malformed_declarations_are_rejected(self) -> None:
        implementation = "pkg.target"
        cases: dict[str, tuple[dict[str, Any], str]] = {
            "path not module": ({"implementation": "pkg/target.py"}, "bad source"),
            "empty": ({"implementation": ""}, "bad source module name"),
            "not text": ({"implementation": cast(Any, 3)}, "bad source module name"),
            "trailing dot": ({"modules": ("pkg.",)}, "bad source module name"),
            "too long": ({"modules": ("p" * 257,)}, "bad source module name"),
            "list": ({"modules": cast(Any, ["pkg.a"])}, "must be a tuple"),
            "duplicate": ({"packages": ("a", "a")}, "duplicate in TargetSources"),
            "repeats implementation": (
                {"modules": (implementation,)},
                "repeats the implementation",
            ),
            "external covers implementation": (
                {"external": ("pkg",)},
                "overlaps followed source",
            ),
            "external inside a followed package": (
                {"packages": ("lib",), "external": ("lib.client",)},
                "overlaps followed source",
            ),
            "dynamic path": ({"dynamic": ("pkg/plugin.py",)}, "bad source"),
            "dynamic relative": ({"dynamic": (".plugin",)}, "bad source"),
            "dynamic list": ({"dynamic": cast(Any, ["pkg.p"])}, "must be a tuple"),
            "dynamic duplicate": (
                {"dynamic": ("pkg.p", "pkg.p")},
                "duplicate in TargetSources",
            ),
            "external covers dynamic": (
                {"dynamic": ("lib.plugin",), "external": ("lib",)},
                "overlaps followed source",
            ),
            "too many": (
                {"modules": tuple(f"pkg.m{index}" for index in range(65))},
                "more than 64",
            ),
        }
        for name, (overrides, message) in cases.items():
            with self.subTest(case=name):
                fields: dict[str, Any] = {"implementation": implementation, **overrides}
                with self.assertRaisesRegex(ValueError, message):
                    TargetSources(**fields)


def _quiet() -> PropertyDef:
    return _property(consumes=frozenset())


class TargetRegistryTest(unittest.TestCase):
    _FIELDS: dict[str, Any] = {
        "name": "t",
        "actions": {},
        "sites": {},
        "install_witnessed": _never,
        "environment_fields": {},
        "make_environment": _unused,
        "make_backend": _unused,
    }

    def test_target_properties_carry_their_control_union(self) -> None:
        target = Target(
            **self._FIELDS,
            properties=(_quiet(),),
            red_controls=frozenset({"torn-write"}),
        )
        self.assertEqual("bytes", target.properties[0].id)
        self.assertIsNone(target.timeout_capable)
        self.assertIsNone(target.contracts)
        self.assertIsNone(target.pinned)

    def test_target_rejects_callables_core_repeats_and_control_drift(self) -> None:
        core = _property(
            id="core.activity",
            rules={"core.activity": _rule("core.activity")},
            coverage=(),
            not_evaluated_reasons={},
        )
        cases: tuple[tuple[tuple[Any, ...], frozenset[str], str], ...] = (
            ((_collect,), frozenset(), "is not a PropertyDef"),
            ((core,), frozenset(), "is implicit"),
            (
                (_quiet(), _quiet()),
                frozenset({"torn-write"}),
                "repeated property",
            ),
            ((_quiet(),), frozenset(), "not the union"),
            ((_quiet(),), frozenset({"torn-write", "x"}), "not the union"),
        )
        for properties, controls, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, message):
                    Target(**self._FIELDS, properties=properties, red_controls=controls)

    def test_effect_hooks_must_be_callable(self) -> None:
        for hook in ("contracts", "pinned"):
            with self.subTest(hook=hook):
                with self.assertRaisesRegex(ValueError, f"{hook} is not callable"):
                    Target(
                        **self._FIELDS,
                        properties=(),
                        red_controls=frozenset(),
                        **cast(dict[str, Any], {hook: 3}),
                    )
                target = Target(
                    **self._FIELDS,
                    properties=(),
                    red_controls=frozenset(),
                    **cast(dict[str, Any], {hook: _never}),
                )
                self.assertIs(_never, getattr(target, hook))


class PropertyErrorTest(unittest.TestCase):
    def test_message_is_kept_whole_with_digest_and_oversize_marker(self) -> None:
        message = "é" * 3000 + "\ud800"
        error = property_error("RuntimeError", message)
        full = message.encode("utf-8", errors="surrogatepass")
        self.assertEqual(message, error.message)
        self.assertEqual(len(full), error.message_bytes)
        self.assertEqual(
            "sha256:" + hashlib.sha256(full).hexdigest(), error.message_sha256
        )
        self.assertTrue(error.oversize)
        at_bound = property_error("KeyError", "x" * MAX_SCALAR_BYTES)
        self.assertFalse(at_bound.oversize)
        long_type = property_error("E" * 129, "m")
        self.assertEqual(("E" * 129, True), (long_type.type, long_type.oversize))

    def test_large_error_text_is_kept_whole_with_its_digest(self) -> None:
        """An error past 64 KiB keeps every byte of its type and message."""
        message = "".join(f"line {i:06d}: diagnostic detail\n" for i in range(7000))
        message = "a" + "é" * 70_000 + message
        full = message.encode("utf-8")
        error = property_error("T" * 70_010, message)
        self.assertEqual(("T" * 70_010, message), (error.type, error.message))
        self.assertEqual(
            (len(full), "sha256:" + hashlib.sha256(full).hexdigest(), True),
            (error.message_bytes, error.message_sha256, error.oversize),
        )
        validate_result(_property(), _result(error=error))
        cases = (
            (replace(error, message=error.message[:65_536]), "error message is"),
            (replace(error, message=error.message + "é"), "error message is"),
            (replace(error, message_sha256="md5"), "does not match"),
        )
        for bad, why in cases:
            with self.subTest(why=why):
                with self.assertRaisesRegex(ValueError, why):
                    validate_result(_property(), _result(error=bad))

    def test_error_validation(self) -> None:
        good = property_error("KeyError", "'p'")
        cases = (
            (replace(good, message_sha256="sha256:" + "0" * 64), "does not match"),
            (replace(good, message_bytes=1), "error message is 3 bytes, not 1"),
            (replace(good, message="'q'"), "does not match"),
            (replace(good, oversize=True), "oversize marker is True"),
            (
                replace(property_error("E", "x" * 4097), oversize=False),
                "oversize marker",
            ),
            (replace(good, type=""), "empty error type"),
            (replace(good, oversize=cast(Any, 0)), "bad error"),
        )
        for error, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(ValueError, message):
                    validate_result(_property(), _result(error=error))
