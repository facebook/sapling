# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Tests for the recording-only EdenFS target: vocabulary, no evaluation, digests.

The checker target sharing its vocabulary is tested in `properties_test`.
"""

from __future__ import annotations

import ast
import json
import random
import unittest
from dataclasses import replace
from pathlib import Path
from types import ModuleType
from typing import Any

from eden.integration.correctness.exploratory.core.checker import (
    check,
    checker_sources,
    CheckerSourceUnavailable,
    content_digest,
    contract_effects,
    NO_EVALUATION,
    NO_PROPERTY_REASON,
    NOT_EVALUATED,
    semantic_digest,
)
from eden.integration.correctness.exploratory.core.effects import EffectSet
from eden.integration.correctness.exploratory.core.facts import (
    HistoryFacts,
    MISSING_REFERENCE,
    UNEXPECTED_REFERENCE,
)
from eden.integration.correctness.exploratory.core.history import (
    CLEANUP,
    CLEANUP_REMOVED,
    Event,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    DEADLINE_OPERATION,
    from_json,
    RunSpec,
    to_json,
)
from eden.integration.correctness.exploratory.core.sources import (
    ModuleSource,
    read_module_source,
    SourceReader,
)
from eden.integration.correctness.exploratory.core.test.testing import ev
from eden.integration.correctness.exploratory.edenfs import (
    backend_processes,
    backend_scm,
    backends,
    differential_backend,
    environment,
    faults,
    file_results,
    observation_bounds,
    reference,
    target,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    expects_reference,
    timeout_capable,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import EdenPinned
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.target import (
    install_witnessed,
    RECORDING_TARGET,
    sample_environment,
    SEMANTICS_PROFILE,
    with_reference_clone,
)

_PACKAGE = "eden.integration.correctness.exploratory"
# The layers above the live runtime, which it never imports: the composer,
# campaigns, CLI, findings, and advisory reports.
_LATER = (
    f"{_PACKAGE}.core.generate",
    f"{_PACKAGE}.core.campaign",
    f"{_PACKAGE}.cli",
    f"{_PACKAGE}.findings",
    f"{_PACKAGE}.advisory",
    f"{_PACKAGE}.test.fault_fixtures",
)
_OWNED: tuple[ModuleType, ...] = (
    backend_processes,
    backend_scm,
    backends,
    differential_backend,
    environment,
    faults,
    file_results,
    observation_bounds,
    reference,
    target,
)
_R0 = "tree/registers/r0.reg"


def _imported(module: ModuleType) -> set[str]:
    names: set[str] = set()
    tree = ast.parse(Path(str(module.__file__)).read_text())
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            names.update(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            names.add(node.module)
            names.update(f"{node.module}.{alias.name}" for alias in node.names)
    return names


def _spec(environment_target: dict[str, Any] | None = None) -> RunSpec:
    sampled = dict(sample_environment(random.Random(3)))
    sampled["case_sensitive"] = True
    body = {
        "schema": "edenfs-exploratory-run",
        "schema_version": 1,
        "seed": 9,
        "target": "edenfs",
        "environment": {"target": environment_target or sampled},
        "actors": [{"id": "a", "actions": ["w1", "r1"]}],
        "actions": {
            "w1": {
                "kind": "write",
                "deadline": "operation",
                "path": _R0,
                "value_id": "w1",
            },
            "r1": {
                "kind": "read",
                "deadline": "operation",
                "path": _R0,
                "channel": "fuse",
            },
        },
    }
    return from_json(json.dumps(body), RECORDING_TARGET)


def _history(spec: RunSpec) -> list[Event]:
    written = encode_value(spec.seed, "a", "w1")
    return [
        ev("invoke", "w1", 1.0),
        ev("ok", "w1", 1.1, actor="a", bytes_written=len(written)),
        ev("invoke", "r1", 1.2),
        ev("ok", "r1", 1.3, actor="a", **content_digest(written)),
    ]


class RecordingTargetTest(unittest.TestCase):
    def test_the_target_registers_no_property_and_no_control(self) -> None:
        self.assertEqual((), RECORDING_TARGET.properties)
        self.assertEqual(frozenset(), RECORDING_TARGET.red_controls)
        self.assertEqual("edenfs", RECORDING_TARGET.name)
        self.assertEqual(SEMANTICS_PROFILE, RECORDING_TARGET.semantics_profile)
        self.assertIs(timeout_capable, RECORDING_TARGET.timeout_capable)
        self.assertIs(install_witnessed, RECORDING_TARGET.install_witnessed)
        self.assertIsNotNone(RECORDING_TARGET.contracts)
        self.assertIsNotNone(RECORDING_TARGET.pinned)
        self.assertFalse(hasattr(RECORDING_TARGET, "model_allows_failure"))

    def test_a_sampled_spec_round_trips_and_its_budgets_default(self) -> None:
        spec = _spec()
        self.assertEqual(spec, from_json(to_json(spec), RECORDING_TARGET))
        self.assertEqual(600, spec.budgets.runner_deadline_seconds)
        self.assertEqual(120, spec.budgets.operation_deadline_seconds)
        self.assertEqual(30, spec.budgets.cleanup_deadline_seconds)

    def test_check_never_evaluates_a_captured_history(self) -> None:
        spec = _spec()
        cleanup = Event(
            CLEANUP, 2.0, {"scheduler": {"status": "completed", "reason": None}}
        )
        for execution, events in (
            (None, _history(spec)),
            ({"status": "completed"}, [*_history(spec), cleanup]),
        ):
            with self.subTest(execution=execution):
                verdict = check(RECORDING_TARGET, spec, events, execution=execution)
                self.assertEqual(NOT_EVALUATED, verdict.kind)
                self.assertEqual(NO_PROPERTY_REASON, verdict.reason)
                self.assertEqual(NO_EVALUATION, verdict.not_evaluated_cause)
                assert verdict.report is not None
                self.assertEqual((), verdict.report.results)
                self.assertEqual({}, dict(verdict.lanes))

    def test_captured_history_decodes_and_yields_contract_effects(self) -> None:
        spec = _spec()
        facts = HistoryFacts.build(spec, _history(spec), RECORDING_TARGET)
        self.assertEqual(
            {"w1": (), "r1": ()},
            {action.id: action.problems for action in facts.actions.values()},
        )
        derived = contract_effects(
            RECORDING_TARGET, spec, facts, ("core.input-legality",)
        )
        assert not isinstance(derived, Exception) and isinstance(derived, tuple)
        pinned, effects = derived
        self.assertIsInstance(pinned, EdenPinned)
        self.assertIsInstance(effects, EffectSet)
        self.assertEqual(["w1"], [group.action for group in effects.groups])

    def test_reference_presence_uses_the_runtimes_predicate(self) -> None:
        """A differential run's shared action owes a side; a plain run's owes none."""
        self.assertIs(expects_reference, RECORDING_TARGET.expects_reference)
        plain = _spec()
        sampled = dict(sample_environment(random.Random(3)))
        sampled["case_sensitive"] = True
        differential = _spec(with_reference_clone(sampled))
        side = {
            "schema_version": 1,
            "outcome": "ok",
            "started": 1.25,
            "completed": 1.28,
            "facts": {},
        }
        for spec, carried, expected in (
            (differential, False, (MISSING_REFERENCE,)),
            (plain, True, (UNEXPECTED_REFERENCE,)),
        ):
            with self.subTest(differential=spec is differential):
                events = _history(spec)
                if carried:
                    terminal = events[-1]
                    events[-1] = Event(
                        terminal.type,
                        terminal.time,
                        {**terminal.data, "reference_result": side},
                    )
                facts = HistoryFacts.build(spec, events, RECORDING_TARGET)
                read = facts.action("r1")
                self.assertEqual((), read.problems)
                assert read.reference is not None
                self.assertEqual(expected, read.reference.problems)

    def test_install_witnessed_reads_await_hits_and_cleanup_facts(self) -> None:
        """A recorded await hit witnesses the install even with no cleanup record.

        A run whose fault unwind outlives its slice keeps the await's
        `hit: true` but has no cleanup-removed record.
        """
        base = _spec()
        install = replace(
            base.actions[0], kind="fault_install", payload={"fault_id": "F"}
        )
        awaited = Action(
            "h1", "fault_await", DEADLINE_OPERATION, payload={"fault_id": "F"}
        )
        spec = replace(
            base,
            actors=(*base.actors, Actor(id="n", actions=("h1",))),
            actions=(*base.actions, awaited),
        )
        cases = {
            "hit": ({"hit": True, "miss": False}, None, True),
            "consumed": ({"hit": False, "miss": False}, None, True),
            "missed": ({"hit": False, "miss": True}, None, False),
            "absent": (None, None, False),
            "await hit": (None, True, True),
            "await miss": (None, False, False),
        }
        for name, (record, hit, expected) in cases.items():
            with self.subTest(name=name):
                events = _history(spec)
                if hit is not None:
                    events += [
                        ev("invoke", "h1", 1.4),
                        ev("ok", "h1", 1.5, actor="n", fault_id="F", hit=hit),
                    ]
                if record is not None:
                    events.append(
                        Event(CLEANUP_REMOVED, 2.0, {"fault_id": "F", **record})
                    )
                facts = HistoryFacts.build(spec, events, RECORDING_TARGET)
                self.assertIs(expected, install_witnessed(spec, facts, install))


def _changing(changed: str) -> SourceReader:
    """Return a source reader that appends a statement to one module."""

    def read(name: str) -> ModuleSource | None:
        source = read_module_source(name)
        if name != changed or source is None:
            return source
        return ModuleSource(source.data + b"\nCHANGED = 1\n", source.is_package)

    return read


class RecordingTargetSourcesTest(unittest.TestCase):
    def test_digest_binds_the_target_and_the_runtime_it_imports(self) -> None:
        digest = semantic_digest(RECORDING_TARGET)
        names = set(checker_sources(RECORDING_TARGET))
        for module in (
            f"{_PACKAGE}.core.actions",
            f"{_PACKAGE}.core.differential",
            f"{_PACKAGE}.edenfs.contracts.checkout",
            f"{_PACKAGE}.edenfs.encoding",
            f"{_PACKAGE}.edenfs.reference",
            "eden.integration.correctness.repo",
            f"{_PACKAGE}.edenfs.target",
        ):
            with self.subTest(module=module):
                self.assertIn(module, names)
                changed = semantic_digest(RECORDING_TARGET, reader=_changing(module))
                self.assertNotEqual(digest, changed)
        self.assertEqual(
            [],
            sorted(
                name
                for name in names
                if any(
                    name == later or name.startswith(later + ".") for later in _LATER
                )
            ),
        )

    def test_every_external_package_is_declared(self) -> None:
        sources = RECORDING_TARGET.sources
        assert sources is not None
        for external in sources.external:
            with self.subTest(external=external):
                narrowed = replace(
                    sources,
                    external=tuple(e for e in sources.external if e != external),
                )
                with self.assertRaisesRegex(CheckerSourceUnavailable, external):
                    checker_sources(replace(RECORDING_TARGET, sources=narrowed))


class RecordingTargetBoundaryTest(unittest.TestCase):
    def test_owned_modules_import_no_property_or_later_layer(self) -> None:
        """Only `target.py` registers properties; the runtime imports none."""
        properties = f"{_PACKAGE}.edenfs.properties"
        violations = {
            module.__name__: sorted(
                name
                for name in _imported(module)
                if any(
                    name == later or name.startswith(later + ".")
                    for later in (_LATER if module is target else (*_LATER, properties))
                )
            )
            for module in _OWNED
        }
        self.assertEqual(
            {}, {name: found for name, found in violations.items() if found}
        )
