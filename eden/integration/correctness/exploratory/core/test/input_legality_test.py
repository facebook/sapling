# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The core input-legality rule: a failed modeled input is legal only when
something excuses it. A state errno needs a contract observation of that input
and never accepts alone; a fault errno is legal only under a targeted nemesis;
a timeout only under an exact witnessed timeout-capable fault. Every
unexcused failure is kept as a finding with typed witnesses.
"""

from __future__ import annotations

import errno
import unittest
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core import effects
from eden.integration.correctness.exploratory.core.checker import (
    ACCEPT,
    check,
    CORE_INPUT_LEGALITY,
    fault_excuses,
    NOT_EVALUATED,
    registry,
    REJECT,
)
from eden.integration.correctness.exploratory.core.effects import ABSENT, Value, Window
from eden.integration.correctness.exploratory.core.facts import (
    ErrnoUse,
    FieldKind,
    FieldSpec,
    HistoryFacts,
    NO_ERRNO,
    ResultSchema,
)
from eden.integration.correctness.exploratory.core.history import (
    Event,
    FAIL,
    INVOKE,
    OK,
)
from eden.integration.correctness.exploratory.core.property import (
    Attribution,
    Finding,
    identity_witnesses,
    ObligationStatus,
    SOLVER_UNKNOWN_INITIAL,
    Witness,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test import checker_testing as t


def _always(*_: Any) -> bool:
    return True


_FAILED = "failed and neither a nemesis nor its declared errnos excuse it"


class InputLegalityTest(unittest.TestCase):
    def test_failed_input_judges_modeled_inputs_only(self) -> None:
        """The daemon serves modeled operations on harness-authored
        state; an unmodeled input's failure is its own tool's contract."""
        spec = t.spec(
            [
                t.action("write", "write"),
                t.action("switch", "switch"),
                t.action("read", "read"),
            ]
        )
        prefix = [
            t.event(INVOKE, "write", 0.5),
            t.event(OK, "write", 0.8),
            t.event(INVOKE, "switch", 3.0),
            t.event(FAIL, "switch", 4.0),
        ]
        accepted = check(t.input_target(), spec, prefix)
        rejected = check(
            t.input_target(),
            spec,
            [*prefix, t.event(INVOKE, "read", 5.0), t.event(FAIL, "read", 6.0)],
        )

        self.assertEqual(ACCEPT, accepted.kind)
        self.assertEqual(
            [("write", "satisfied")],
            t.statuses(t.result(accepted, CORE_INPUT_LEGALITY)),
        )
        self.assertEqual(REJECT, rejected.kind)
        self.assertEqual(f"input/read-failed: read {_FAILED}", rejected.reason)

    def test_state_errnos_need_a_contract_observation_and_never_accept_alone(
        self,
    ) -> None:
        """A declared state errno is legal only in some states.

        Legal twin: the target's contracts hook observes the state behind the
        failure (the path is absent) and the pinned facts agree, so the
        engine satisfies it. Violating twins: the pinned path exists and
        nothing removes it, so the engine violates it with a proof; with no
        observation, it is not evaluated with `state-unobserved`; with no
        pinned fact, not evaluated with `unknown-initial`. It never accepts
        without evidence. The same errno undeclared, or declared only as a
        fault errno without a nemesis, is a direct violation.
        """
        spec = t.spec([t.action("gone", "read"), t.action("read", "read")])
        events = [
            t.event(INVOKE, "gone", 3.0),
            t.event(FAIL, "gone", 4.0, errno=errno.ENOENT),
        ]
        state = t.input_target(errnos={"ENOENT": ErrnoUse.STATE})

        def observe_gone(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            ref = effects.ObligationRef(CORE_INPUT_LEGALITY, "gone")
            sink.observe(
                t.observation(
                    ref, "gone:absent", action="gone", window=Window(3.0, 4.0)
                )
            )

        observed = check(replace(state, contracts=observe_gone), spec, events)
        absent = check(
            replace(state, contracts=observe_gone, pinned=t.pinned(ABSENT)),
            spec,
            events,
        )
        present = check(
            replace(state, contracts=observe_gone, pinned=t.pinned(Value("file"))),
            spec,
            events,
        )
        unobserved = check(state, spec, events)
        undeclared = check(t.input_target(), spec, events)
        unfaulted = check(
            t.input_target(errnos={"ENOENT": ErrnoUse.FAULT}), spec, events
        )
        other = check(
            state,
            spec,
            [
                *events,
                t.event(INVOKE, "read", 5.0),
                t.event(FAIL, "read", 6.0, errno=errno.EIO),
            ],
        )

        (unknown,) = t.result(observed, CORE_INPUT_LEGALITY).obligations
        self.assertEqual(
            (ObligationStatus.NOT_EVALUATED, SOLVER_UNKNOWN_INITIAL),
            (unknown.status, unknown.not_evaluated_reason),
        )
        self.assertEqual(f"blocked/{SOLVER_UNKNOWN_INITIAL}", observed.reason)
        (legal,) = t.result(absent, CORE_INPUT_LEGALITY).obligations
        self.assertEqual(ObligationStatus.SATISFIED, legal.status)
        (illegal,) = t.result(present, CORE_INPUT_LEGALITY).obligations
        self.assertEqual(
            (ObligationStatus.VIOLATED, Attribution.PRODUCT),
            (illegal.status, illegal.attribution),
        )
        self.assertEqual(REJECT, present.kind)
        (proof,) = t.report(present).proofs
        self.assertEqual(
            (illegal.proof, ("gone:absent",), (), ()),
            (proof.id, proof.observations, proof.groups, proof.constraints),
        )
        (open_,) = t.result(unobserved, CORE_INPUT_LEGALITY).obligations
        self.assertEqual(
            (
                ObligationStatus.NOT_EVALUATED,
                "core.input-legality.state-unobserved",
                "gone failed with ENOENT",
            ),
            (open_.status, open_.not_evaluated_reason, open_.reason),
        )
        for verdict in (observed, unobserved):
            self.assertEqual(NOT_EVALUATED, verdict.kind)
        for verdict in (undeclared, unfaulted):
            self.assertEqual(f"input/read-failed: gone {_FAILED}", verdict.reason)
        self.assertEqual(f"input/read-failed: read {_FAILED}", other.reason)

    def test_state_errno_obligation_declares_its_errno_identity(self) -> None:
        """An engine-decided state errno is classifiable: `errno` rides the obligation.

        Without it, an engine violation of the open obligation carries only
        `input-kind` and `identity_witnesses` cannot classify it. A
        succeeded input names `none`; a direct violation keeps one `errno`.
        """
        spec = t.spec([t.action("gone", "read"), t.action("read", "read")])
        events = [
            t.event(INVOKE, "gone", 3.0),
            t.event(FAIL, "gone", 4.0, errno=errno.ENOENT),
            t.event(INVOKE, "read", 5.0),
            t.event(OK, "read", 6.0),
        ]
        state = t.input_target(errnos={"ENOENT": ErrnoUse.STATE})

        def observe_gone(
            _spec: RunSpec, _facts: HistoryFacts, _pinned: Any, sink: Any
        ) -> None:
            ref = effects.ObligationRef(CORE_INPUT_LEGALITY, "gone")
            sink.observe(
                t.observation(
                    ref, "gone:absent", action="gone", window=Window(3.0, 4.0)
                )
            )

        rule = next(
            r
            for d in registry(state)
            for r in d.rules.values()
            if r.id == CORE_INPUT_LEGALITY
        )
        expected = {
            "gone": (("errno", "ENOENT"), ("input-kind", "read")),
            "read": (("errno", "none"), ("input-kind", "read")),
        }
        for target in (state, replace(state, contracts=observe_gone)):
            result = t.result(check(target, spec, events), CORE_INPUT_LEGALITY)
            for obligation in result.obligations:
                with self.subTest(contracts=target.contracts, scope=obligation.scope):
                    finding = Finding(
                        rule_id=CORE_INPUT_LEGALITY,
                        attribution=Attribution.PRODUCT,
                        reason="engine",
                        subject=obligation.subject,
                        witnesses=obligation.witnesses,
                    )
                    self.assertEqual(
                        expected[obligation.scope], identity_witnesses(rule, finding)
                    )
        violated = t.result(
            check(t.input_target(), spec, events), CORE_INPUT_LEGALITY
        ).findings
        self.assertEqual(
            [(("input-kind", "read"), ("errno", "ENOENT"), ("action", "gone"))],
            [tuple((w.role, w.value) for w in f.witnesses) for f in violated],
        )

    def test_error_types_classify_errnoless_failures(self) -> None:
        """An errno-less failure naming a typed error is judged by that type.

        A declared state type waits for an observation, like a state errno;
        an undeclared type, or a fault type without a nemesis, is a direct
        violation keyed by the type; a failure naming no type falls back to
        `NO_ERRNO`.
        """
        base = t.input_target()
        result = ResultSchema(
            fail=(
                FieldSpec("errno", FieldKind.INT, required=False),
                FieldSpec("error_type", FieldKind.TEXT, required=False),
            ),
            errnos={NO_ERRNO: ErrnoUse.STATE},
            error_types={"BUSY": ErrnoUse.STATE, "LOST": ErrnoUse.FAULT},
        )
        read = replace(base.actions["read"], result=result)
        target = replace(base, actions={**base.actions, "read": read})
        spec = t.spec([t.action("r", "read")])
        cases = {
            "BUSY": ("not-evaluated", "r failed with BUSY"),
            "LOST": ("violated", "LOST"),
            "GONE": ("violated", "GONE"),
            None: ("not-evaluated", "r failed with none"),
        }
        for error_type, (status, detail) in cases.items():
            with self.subTest(error_type=error_type):
                typed = {} if error_type is None else {"error_type": error_type}
                events = [t.event(INVOKE, "r", 1.0), t.event(FAIL, "r", 2.0, **typed)]
                verdict = check(target, spec, events)
                (obligation,) = t.result(verdict, CORE_INPUT_LEGALITY).obligations
                self.assertEqual(status, str(obligation.status))
                if status == "violated":
                    self.assertEqual(Attribution.PRODUCT, obligation.attribution)
                    self.assertIn(Witness("errno", detail), obligation.witnesses)
                    self.assertEqual(f"input/read-failed: r {_FAILED}", verdict.reason)
                else:
                    self.assertEqual(
                        ("core.input-legality.state-unobserved", detail),
                        (obligation.not_evaluated_reason, obligation.reason),
                    )

    def test_a_fault_excuses_only_the_failures_its_behavior_can_cause(self) -> None:
        """`error` excuses FAULT, undeclared, and errno-less failures, never a
        state errno; `block` and `delay` excuse none of these (only a
        witnessed ETIMEDOUT, pinned separately)."""
        state = t.input_target(errnos={"ENOENT": ErrnoUse.STATE, "EIO": ErrnoUse.FAULT})
        failures = {
            "state ENOENT": {"errno": errno.ENOENT},
            "fault EIO": {"errno": errno.EIO},
            "undeclared EPERM": {"errno": errno.EPERM},
            "errno-less": {},
        }
        expected = {
            "error": {
                "state ENOENT": "not-evaluated",
                "fault EIO": "satisfied",
                "undeclared EPERM": "satisfied",
                "errno-less": "satisfied",
            },
            "block": {
                "state ENOENT": "not-evaluated",
                "fault EIO": "violated",
                "undeclared EPERM": "violated",
                "errno-less": "violated",
            },
        }
        expected["delay"] = expected["block"]
        for behavior, by_failure in expected.items():
            for name, facts in failures.items():
                with self.subTest(behavior=behavior, failure=name):
                    actions = [
                        t.action(
                            "fault",
                            "fault_install",
                            fault_id="F",
                            target_action="read",
                            behavior=behavior,
                        ),
                        t.action("read", "read"),
                    ]
                    events = [
                        t.event(INVOKE, "fault", 0.5),
                        t.event(OK, "fault", 1.0),
                        t.event(INVOKE, "read", 2.0),
                        t.event(FAIL, "read", 3.0, **facts),
                    ]
                    verdict = check(state, t.spec(actions), events)
                    self.assertEqual(
                        [("read", by_failure[name])],
                        t.statuses(t.result(verdict, CORE_INPUT_LEGALITY)),
                    )

    def test_an_error_fault_never_excuses_etimedout(self) -> None:
        name = errno.errorcode[errno.ETIMEDOUT]
        self.assertFalse(fault_excuses("error", ErrnoUse.TIMEOUT, name))
        self.assertTrue(fault_excuses("block", ErrnoUse.TIMEOUT, name))
        self.assertTrue(fault_excuses("delay", ErrnoUse.TIMEOUT, name))

    def test_fault_errnos_are_legal_only_under_a_targeted_nemesis(self) -> None:
        actions = [
            t.action(
                "fault",
                "fault_install",
                fault_id="F",
                target_action="read",
                behavior="error",
            ),
            t.action("read", "read"),
        ]
        events = [
            t.event(INVOKE, "fault", 0.5),
            t.event(OK, "fault", 1.0),
            t.event(INVOKE, "read", 2.0),
            t.event(FAIL, "read", 3.0, errno=errno.EIO),
        ]
        fault = t.input_target(errnos={"EIO": ErrnoUse.FAULT})
        self.assertEqual(ACCEPT, check(fault, t.spec(actions), events).kind)
        untargeted = [
            replace(actions[0], payload={**actions[0].payload, "target_action": "x"}),
            actions[1],
        ]
        self.assertEqual(REJECT, check(fault, t.spec(untargeted), events).kind)

    def test_every_unexcused_input_failure_is_kept_with_typed_witnesses(self) -> None:
        """core.input-legality preserves every failure, not just the first."""
        spec = t.spec(
            [t.action("r1", "read", path="a"), t.action("r2", "read", path="b")]
        )
        events = [
            t.event(INVOKE, "r1", 1.0),
            t.event(FAIL, "r1", 2.0, errno=errno.EIO),
            t.event(INVOKE, "r2", 3.0),
            t.event(FAIL, "r2", 4.0),
        ]
        target = t.path_input_target()
        result = t.result(check(target, spec, events), CORE_INPUT_LEGALITY)
        self.assertEqual([("r1", "violated"), ("r2", "violated")], t.statuses(result))
        self.assertEqual(
            [
                (("input-kind", "read"), ("errno", "EIO"), ("action", "r1")),
                (("input-kind", "read"), ("errno", "none"), ("action", "r2")),
            ],
            [
                tuple((w.role, w.value) for w in finding.witnesses)
                for finding in result.findings
            ],
        )
        self.assertEqual(
            ["path:a", "path:b"], [finding.subject.token for finding in result.findings]
        )

    def test_timeout_is_excused_only_by_exact_witnessed_timeout_capable_fault(
        self,
    ) -> None:
        """ETIMEDOUT is excused only when the schema declares it a timeout
        errno, for the positively witnessed exact target on a timeout-capable
        channel that completed before the fault's removal was invoked;
        overlap with an unrelated parking fault excuses nothing."""
        timeout = {"ETIMEDOUT": ErrnoUse.TIMEOUT}
        capable = t.input_target(
            errnos=timeout, witnessed=_always, timeout_capable=_always
        )

        def scenario(
            *,
            target_action: str = "read",
            code: int = errno.ETIMEDOUT,
            remove_at: float | None = 4.0,
            remove_invoked: float | None = None,
        ) -> tuple[RunSpec, list[Event]]:
            actions = [
                t.action(
                    "fault",
                    "fault_install",
                    fault_id="F",
                    target_action=target_action,
                    behavior="block",
                ),
                t.action("read", "read"),
            ]
            events = [
                t.event(INVOKE, "fault", 0.5),
                t.event(OK, "fault", 1.0),
                t.event(INVOKE, "read", 2.0),
                t.event(FAIL, "read", 3.0, errno=code),
            ]
            if remove_at is not None:
                actions.append(t.action("remove", "fault_remove", fault_id="F"))
                invoked = remove_at - 0.5 if remove_invoked is None else remove_invoked
                events += [
                    t.event(INVOKE, "remove", invoked),
                    t.event(OK, "remove", remove_at),
                ]
            return t.spec(actions), events

        cases = (
            ("exact witnessed capable", capable, scenario(), ACCEPT),
            ("never removed", capable, scenario(remove_at=None), ACCEPT),
            (
                "timeout not declared",
                t.input_target(witnessed=_always, timeout_capable=_always),
                scenario(),
                REJECT,
            ),
            (
                "unwitnessed",
                t.input_target(errnos=timeout, timeout_capable=_always),
                scenario(),
                REJECT,
            ),
            ("wrong target", capable, scenario(target_action="other"), REJECT),
            (
                "not timeout-capable",
                t.input_target(
                    errnos=timeout, witnessed=_always, timeout_capable=t.never
                ),
                scenario(),
                REJECT,
            ),
            (
                "no timeout declaration",
                t.input_target(errnos=timeout, witnessed=_always),
                scenario(),
                REJECT,
            ),
            ("completed after removal", capable, scenario(remove_at=2.5), REJECT),
            (
                "removal invoked before completion",
                capable,
                scenario(remove_invoked=2.9, remove_at=4.0),
                REJECT,
            ),
            (
                "removal invoked at completion",
                capable,
                scenario(remove_invoked=3.0, remove_at=4.0),
                REJECT,
            ),
            (
                "removal invoked just after completion",
                capable,
                scenario(remove_invoked=3.1, remove_at=4.0),
                ACCEPT,
            ),
            (
                "unrelated parking fault",
                capable,
                scenario(target_action="other"),
                REJECT,
            ),
            (
                "targeted non-timeout error",
                t.input_target(),
                scenario(code=errno.EIO),
                REJECT,
            ),
        )
        for name, target, (spec, events), expected in cases:
            with self.subTest(case=name):
                verdict = check(target, spec, events)
                self.assertEqual(expected, verdict.kind, verdict.reason)
        # An unexcused timeout inside an unrelated fault window is
        # undetermined; one the exact fault does not excuse stays product.
        for name, target, scene, attribution in (
            ("unrelated", capable, scenario(target_action="other"), "undetermined"),
            (
                "unwitnessed",
                t.input_target(errnos=timeout, timeout_capable=_always),
                scenario(),
                "product",
            ),
            ("after removal", capable, scenario(remove_at=2.5), "product"),
            (
                "unrelated EIO",
                capable,
                scenario(target_action="other", code=errno.EIO),
                "product",
            ),
        ):
            with self.subTest(attribution=name):
                (finding,) = t.result(
                    check(target, *scene), CORE_INPUT_LEGALITY
                ).findings
                self.assertEqual(attribution, str(finding.attribution))
