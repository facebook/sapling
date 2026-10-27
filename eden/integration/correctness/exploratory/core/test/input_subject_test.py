# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Input-legality findings are grouped by the failing input's declared subject: one
error set per path, two-path and no-path inputs kept apart, and a state errno is
supported only by an observation of that same input. A target whose input has
no declared subject, or whose contracts record an input for a core obligation it did not
declare, is a contract error. The task key is subject-free: error sets of the
same symptom on different subjects share it by design.
"""

from __future__ import annotations

import errno
import unittest
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core import effects
from eden.integration.correctness.exploratory.core.checker import (
    check,
    CORE_INPUT_LEGALITY,
    PROPERTY_ERROR,
    registry,
)
from eden.integration.correctness.exploratory.core.effects import ABSENT, Expect, Window
from eden.integration.correctness.exploratory.core.facts import ErrnoUse, HistoryFacts
from eden.integration.correctness.exploratory.core.history import (
    Event,
    FAIL,
    INVOKE,
    OK,
)
from eden.integration.correctness.exploratory.core.identity import canonical_errors
from eden.integration.correctness.exploratory.core.property import (
    ObligationStatus,
    SOLVER_UNKNOWN_INITIAL,
)
from eden.integration.correctness.exploratory.core.runspec import Action, RunSpec
from eden.integration.correctness.exploratory.core.target import Binding, Target
from eden.integration.correctness.exploratory.core.test import checker_testing as t


def _two_paths(_spec: RunSpec, action: Action) -> effects.Footprint:
    return effects.Footprint(
        reads=frozenset({action.payload["src"]}),
        writes=frozenset({action.payload["dst"]}),
    )


def _rename_source(_spec: RunSpec, action: Action) -> str:
    return effects.normalize_path(action.payload["src"])


def _descriptor_path(_spec: RunSpec, action: Action) -> str:
    return action.payload["path"]


def _dot_path(_spec: RunSpec, action: Action) -> str:
    return "./" + action.payload["path"]


def _subject_target() -> Target:
    """`_input_target` plus a two-path rename and a descriptor read with no footprint."""
    base = t.input_target()
    result = base.actions["read"].result
    rename = replace(
        t.action_def("rename", "input"),
        modeled=True,
        result=result,
        footprint=_two_paths,
        binding=Binding.PATH,
        input_subject=_rename_source,
    )
    descriptor = replace(
        t.action_def("fdread", "input"),
        modeled=True,
        result=result,
        binding=Binding.DESCRIPTOR,
        input_subject=_descriptor_path,
    )
    return replace(
        base, actions={**base.actions, "rename": rename, "fdread": descriptor}
    )


def _input_keys(target: Target, spec: RunSpec, events: list[Event]) -> dict[str, str]:
    """Return each `core.input-legality` error set's task key, by subject token."""
    rules = {r.id: r for d in registry(target) for r in d.rules.values()}
    result = t.result(check(target, spec, events), CORE_INPUT_LEGALITY)
    assert result.error is None, result.error
    sets = canonical_errors(result.findings, rules).sets
    return {error_set.subject.token: error_set.key for error_set in sets}


class InputSubjectTest(unittest.TestCase):
    def test_input_legality_keys_each_failure_by_its_path(self) -> None:
        """Unrelated input failures never share a subject; each subject's key is stable."""
        target = t.path_input_target()
        spec = t.spec(
            [
                t.action("ra", "read", path="./a"),
                t.action("rb", "read", path="b"),
                t.action("root", "read", path="."),
            ]
        )
        failures = [
            t.event(INVOKE, "ra", 1.0),
            t.event(FAIL, "ra", 2.0, errno=errno.EIO),
            t.event(INVOKE, "rb", 3.0),
            t.event(FAIL, "rb", 4.0, errno=errno.EIO),
            t.event(INVOKE, "root", 5.0),
            t.event(FAIL, "root", 6.0, errno=errno.EIO),
        ]
        rules = {r.id: r for d in registry(target) for r in d.rules.values()}

        def keys(events: list[Event]) -> dict[str, str]:
            result = t.result(check(target, spec, events), CORE_INPUT_LEGALITY)
            sets = canonical_errors(result.findings, rules).sets
            return {error_set.subject.token: error_set.key for error_set in sets}

        both = keys(failures)
        self.assertEqual(["path:.", "path:a", "path:b"], sorted(both))
        only_a = keys(
            [*failures[:2], t.event(INVOKE, "rb", 3.0), t.event(OK, "rb", 4.0)]
        )
        self.assertEqual({"path:a": both["path:a"]}, only_a)

    def test_input_legality_keys_two_path_and_no_path_inputs_apart(self) -> None:
        """A declared input subject groups two-path and descriptor inputs apart."""
        target = _subject_target()
        ids = ("fd1", "fd2", "mv1", "mv2")
        spec = t.spec(
            [
                t.action("fd1", "fdread", path="x"),
                t.action("fd2", "fdread", path="y"),
                t.action("mv1", "rename", src="./a", dst="b"),
                t.action("mv2", "rename", src="c", dst="d"),
            ]
        )

        def history(*failing: str) -> list[Event]:
            events = []
            for index, action_id in enumerate(ids):
                end = (
                    t.event(FAIL, action_id, 2.0 * index + 2, errno=errno.EIO)
                    if action_id in failing
                    else t.event(OK, action_id, 2.0 * index + 2)
                )
                events += [t.event(INVOKE, action_id, 2.0 * index + 1), end]
            return events

        every = _input_keys(target, spec, history(*ids))
        self.assertEqual(["path:a", "path:c", "path:x", "path:y"], sorted(every))
        for action_id, token in (("fd1", "path:x"), ("mv1", "path:a")):
            with self.subTest(alone=action_id):
                self.assertEqual(
                    {token: every[token]}, _input_keys(target, spec, history(action_id))
                )

    def test_input_without_a_declared_subject_is_a_target_contract_error(self) -> None:
        """Only a single-path or ROOT footprint keys an input with no input_subject."""
        base = _subject_target()
        rename, descriptor = base.actions["rename"], base.actions["fdread"]
        with self.assertRaisesRegex(
            ValueError, "descriptor input declares input_subject"
        ):
            replace(descriptor, input_subject=None)
        cases = {
            "two paths": (
                {"rename": replace(rename, input_subject=None)},
                t.action("x", "rename", src="a", dst="b"),
                "footprint of 'x' names 2 paths and the action declares no input_subject",
            ),
            "no path": (
                {
                    "fdread": replace(
                        descriptor, binding=Binding.PATH, input_subject=None
                    )
                },
                t.action("x", "fdread", path="p"),
                "footprint of 'x' names 0 paths and the action declares no input_subject",
            ),
            "non-canonical subject": (
                {"fdread": replace(descriptor, input_subject=_dot_path)},
                t.action("x", "fdread", path="p"),
                "input_subject returned './p', not a canonical path",
            ),
        }
        events = [t.event(INVOKE, "x", 1.0), t.event(OK, "x", 2.0)]
        for name, (actions, action, message) in cases.items():
            with self.subTest(case=name):
                target = replace(base, actions={**base.actions, **actions})
                verdict = check(target, t.spec([action]), events)
                error = t.result(verdict, CORE_INPUT_LEGALITY).error
                assert error is not None
                self.assertEqual("TargetContractError", error.type)
                self.assertIn(message, error.message)
                self.assertEqual(PROPERTY_ERROR, verdict.not_evaluated_cause)

    def test_state_errno_support_comes_only_from_that_inputs_observation(self) -> None:
        """An observation of another action, or a requirement, never supports it."""
        spec = t.spec([t.action("gone", "read"), t.action("other", "read")])
        events = [
            t.event(INVOKE, "gone", 3.0),
            t.event(FAIL, "gone", 4.0, errno=errno.ENOENT),
            t.event(INVOKE, "other", 10.0),
            t.event(OK, "other", 11.0),
        ]
        state = t.input_target(errnos={"ENOENT": ErrnoUse.STATE})
        ref = effects.ObligationRef(CORE_INPUT_LEGALITY, "gone")
        later = Window(10.0, 11.0)

        def elsewhere(_spec: RunSpec, _facts: Any, _pinned: Any, sink: Any) -> None:
            sink.observe(t.observation(ref, "x", action="other", window=later))

        def required(_spec: RunSpec, _facts: Any, _pinned: Any, sink: Any) -> None:
            need = effects.Requirement((t.FILE_CELL,), Expect(((effects.ANY,),)), ref)
            sink.group(t.group(action="other", window=later, requires=(need,)))

        def own(_spec: RunSpec, _facts: Any, _pinned: Any, sink: Any) -> None:
            sink.observe(
                t.observation(ref, "x", action="gone", window=Window(3.0, 4.0))
            )

        cases = {
            "another action": (elsewhere, "which judges another input"),
            "a requirement": (required, "only an observation of that input can"),
        }
        for name, (contracts, message) in cases.items():
            with self.subTest(case=name):
                verdict = check(replace(state, contracts=contracts), spec, events)
                error = t.result(verdict, "quiet").error
                assert error is not None
                self.assertEqual("EffectContractError", error.type)
                self.assertIn(message, error.message)
                gone = t.result(verdict, CORE_INPUT_LEGALITY).obligations[0]
                self.assertEqual(
                    ("gone", "core.input-legality.state-unobserved"),
                    (gone.scope, gone.not_evaluated_reason),
                )
        verdict = check(replace(state, contracts=own), spec, events)
        self.assertIsNone(t.result(verdict, "quiet").error)
        gone = t.result(verdict, CORE_INPUT_LEGALITY).obligations[0]
        self.assertEqual(SOLVER_UNKNOWN_INITIAL, gone.not_evaluated_reason)
        verdict = check(
            replace(state, contracts=own, pinned=t.pinned(ABSENT)), spec, events
        )
        gone = t.result(verdict, CORE_INPUT_LEGALITY).obligations[0]
        self.assertEqual(ObligationStatus.SATISFIED, gone.status)

    def test_contract_input_for_an_undeclared_core_obligation_is_a_contract_error(
        self,
    ) -> None:
        spec = t.spec([t.action("r", "read"), t.action("s", "sleep")])
        events = [*t.READ_OK, t.event(INVOKE, "s", 3.0), t.event(OK, "s", 4.0)]

        def observe_other(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            ref = effects.ObligationRef(CORE_INPUT_LEGALITY, "s")
            sink.observe(t.observation(ref, "zz", action="s", window=Window(3.0, 4.0)))

        verdict = check(
            replace(t.input_target(), contracts=observe_other), spec, events
        )
        error = t.result(verdict, CORE_INPUT_LEGALITY).error
        assert error is not None
        self.assertEqual("PropertyContractError", error.type)
        self.assertIn("never declared", error.message)
        self.assertEqual(PROPERTY_ERROR, verdict.not_evaluated_cause)
