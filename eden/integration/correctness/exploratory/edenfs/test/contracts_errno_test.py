# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""What a failed input says about state, for core input legality: Thrift error codes and
failures that carry no errno, the exit statuses and Thrift readdir error pairs the
probes established (anything else stays unobserved), and the path each modeled input
is keyed by.
"""

from __future__ import annotations

import errno
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    EffectSet,
    ObligationRef,
    Observation,
    Value,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts, NO_ERRNO
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.property import (
    CheckInput,
    path_subject,
)
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test.testing import ev
from eden.integration.correctness.exploratory.edenfs.actions import ACTION_DEFS
from eden.integration.correctness.exploratory.edenfs.contracts import record_contracts
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    DIR,
    EdenPinned,
    INPUT_LEGALITY_RULE,
    path_cell,
)
from eden.integration.correctness.exploratory.edenfs.test.contracts_fixtures import (
    action_payload,
    committed_value,
    contract_one,
    contract_sink,
    is_explained,
    R0,
    R1,
    run_spec,
    TIP,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    RecordingSink,
    VOCAB_TARGET,
)


class ErrnolessFailureTest(unittest.TestCase):
    """State-legal failures that carry no `errno` field."""

    def _failed(self, payload: dict[str, Any], **facts: Any) -> RecordingSink:
        _spec_, sink = contract_one(
            payload, ev("invoke", "x", 1.0), ev("fail", "x", 2.0, **facts)
        )
        return sink

    def test_a_thrift_read_error_code_means_the_same_state_as_its_errno(self) -> None:
        thrift = action_payload(
            "read", path=R0, channel="thrift", sync_timeout_seconds=60
        )
        for code, legal, illegal in (
            (errno.ENOENT, ABSENT, committed_value(R0, TIP)),
            (errno.EISDIR, DIR, ABSENT),
        ):
            with self.subTest(code=code):
                sink = self._failed(
                    thrift,
                    reason="thrift",
                    source="thrift",
                    error_type="POSIX_ERROR",
                    error_code=code,
                    message="x",
                )
                (observation,) = sink.observations
                self.assertEqual(f"o:x:{errno.errorcode[code].lower()}", observation.id)
                self.assertEqual(
                    (ObligationRef(INPUT_LEGALITY_RULE, "x"),), observation.obligations
                )
                self.assertTrue(observation.expect.holds((legal,)))
                self.assertFalse(observation.expect.holds((illegal,)))
        eio = self._failed(
            thrift,
            reason="thrift",
            source="thrift",
            error_type="POSIX_ERROR",
            error_code=errno.EIO,
            message="x",
        )
        (forbidden,) = eio.observations
        self.assertEqual("o:x:forbidden", forbidden.id)
        for value in (ABSENT, DIR, committed_value(R0, TIP)):
            self.assertFalse(forbidden.expect.holds((value,)))

    def test_errnoless_read_failures_without_a_state_code_are_forbidden(self) -> None:
        """A read failure no state allows is a product finding.

        Violating twins: a Thrift read failing EIO, and a FUSE read failing
        without an errno (an empty read), outside any nemesis. Legal twins:
        the same reads failing ENOENT after an unlink.
        """
        spec = run_spec(
            {
                "u": action_payload("unlink", path=R0),
                "t": action_payload(
                    "read", path=R0, channel="thrift", sync_timeout_seconds=60
                ),
                "f": action_payload("read", path=R0, channel="fuse"),
            },
            [{"id": "a", "actions": ["u", "t", "f"]}],
        )
        unlinked = [ev("invoke", "u", 1.0), ev("ok", "u", 2.0, path=R0)]
        thrift = {"reason": "thrift", "source": "thrift", "error_type": "POSIX_ERROR"}
        for name, t_code, f_facts, legal in (
            ("ENOENT", errno.ENOENT, {"reason": "open", "errno": errno.ENOENT}, True),
            ("thrift EIO", errno.EIO, {"reason": "open", "errno": errno.ENOENT}, False),
            ("fuse no errno", errno.ENOENT, {"reason": "empty read"}, False),
        ):
            with self.subTest(case=name):
                events = [
                    *unlinked,
                    ev("invoke", "t", 3.0),
                    ev("fail", "t", 4.0, **thrift, error_code=t_code, message="x"),
                    ev("invoke", "f", 5.0),
                    ev("fail", "f", 6.0, **f_facts),
                ]
                statuses = _input_legality(spec, events)
                self.assertEqual(("open", "open"), (statuses["t"], statuses["f"]))
                self.assertEqual(legal, is_explained(spec, events))

    def test_an_error_fault_explains_an_errnoless_thrift_read_not_a_state_code(
        self,
    ) -> None:
        """A targeted `error` fault fails a Thrift read without an `error_code`.

        `newEdenError(const std::exception&)` builds GENERIC_ERROR without
        errorCode for the injected `std::runtime_error`: the fault explains
        that failure, and no state observation judges it as the product's.
        Violating twin: the same faulted read naming ENOENT for a present
        register is a state code, still judged by its observation.
        """
        spec = run_spec(
            {
                "f": action_payload(
                    "fault_install",
                    fault_id="F",
                    site="wait-for-pending-writes",
                    target_action="t",
                    behavior="error",
                ),
                "t": {
                    **action_payload(
                        "read", path=R0, channel="thrift", sync_timeout_seconds=60
                    ),
                    "requires": [["f", "finished"]],
                },
                "x": {
                    **action_payload("fault_remove", fault_id="F"),
                    "requires": [["t", "finished"]],
                },
            },
            [{"id": "n", "actions": ["f", "x"]}, {"id": "a", "actions": ["t"]}],
        )
        for name, code, explained in (
            ("errno-less", None, True),
            ("ENOENT", errno.ENOENT, False),
        ):
            with self.subTest(case=name):
                failure = {
                    "reason": "thrift",
                    "source": "thrift",
                    "error_type": "GENERIC_ERROR",
                    "message": "exploratory fault F",
                    **({} if code is None else {"error_code": code}),
                }
                events = [
                    ev("invoke", "f", 1.0),
                    ev("ok", "f", 1.1, fault_id="F"),
                    ev("invoke", "t", 2.0),
                    ev("fail", "t", 3.0, **failure),
                    ev("invoke", "x", 4.0),
                    ev("ok", "x", 4.1, fault_id="F", removed=0),
                ]
                observed = [
                    o
                    for o in contract_sink(spec, events).observations
                    if o.action == "t"
                ]
                self.assertEqual(not explained, bool(observed))
                self.assertEqual(explained, is_explained(spec, events))

    def test_unestablished_exit_statuses_and_untyped_readdir_errors_stay_unobserved(
        self,
    ) -> None:
        """Only `sl rm` exit 1 and `sl mv` exit 255 are modeled exit statuses."""
        process = {"reason": "x", "returncode": 1, "stdout": "", "stderr": "x\n"}
        for payload, facts in (
            (action_payload("sl_rm", path=R0), {**process, "returncode": 2}),
            (
                action_payload("sl_mv", source=R0, destination="tree/registers/m"),
                process,
            ),
            (
                action_payload("readdir", path="tree/registers", channel="thrift"),
                {"reason": "x", "source": "thrift"},
            ),
            (action_payload("status", channel="sl"), {"reason": "diff: failed"}),
        ):
            with self.subTest(kind=payload["kind"]):
                self.assertEqual([], self._failed(payload, **facts).observations)

    def test_core_input_legality_leaves_errnoless_state_failures_open(self) -> None:
        """Legal twin: every state-legal errno-less failure is open, never violated.

        A refused status is decided by its contract observation (open); a
        Sapling `sl rm`/`sl mv` exit status stays unobserved.

        Violating twin: a FUSE read failing EIO outside any nemesis stays a
        product violation.
        """
        spec = run_spec(
            {
                "u": action_payload("unlink", path=R0),
                "t": action_payload(
                    "read", path=R0, channel="thrift", sync_timeout_seconds=60
                ),
                "h": action_payload("sl_rm", path=R0),
                "m": action_payload("sl_mv", source=R1, destination="tree/registers/m"),
                "s": action_payload("status", channel="sl"),
                "q": action_payload("status", channel="thrift"),
                "p": action_payload("status", channel="thrift"),
                "g": action_payload("status", channel="thrift"),
                "e": action_payload("read", path=R1, channel="fuse"),
            },
            [{"id": "a", "actions": ["u", "t", "h", "m", "s", "q", "p", "g", "e"]}],
        )

        def thrift_status(action_id: str, time: float, error_type: str) -> list[Event]:
            return [
                ev("invoke", action_id, time),
                ev(
                    "fail",
                    action_id,
                    time + 0.05,
                    reason="thrift",
                    source="thrift",
                    error_type=error_type,
                ),
            ]

        process = {"stdout": "", "stderr": "x\n"}
        events = [
            ev("invoke", "u", 1.0),
            ev("ok", "u", 2.0, path=R0),
            ev("invoke", "t", 3.0),
            ev(
                "fail",
                "t",
                4.0,
                reason="thrift",
                source="thrift",
                error_type="POSIX_ERROR",
                error_code=errno.ENOENT,
                message="x",
            ),
            ev("invoke", "h", 5.0),
            ev("fail", "h", 6.0, reason="sl rm", returncode=1, **process),
            ev("invoke", "m", 7.0),
            ev("fail", "m", 8.0, reason="sl mv", returncode=1, **process),
            ev("invoke", "s", 9.0),
            ev("fail", "s", 10.0, reason="sl status", returncode=255, **process),
            ev("invoke", "q", 10.5),
            ev(
                "fail",
                "q",
                10.7,
                reason="thrift",
                source="thrift",
                error_type="OUT_OF_DATE_PARENT",
            ),
            *thrift_status("p", 10.8, "CHECKOUT_IN_PROGRESS"),
            *thrift_status("g", 10.9, "GENERIC_ERROR"),
            ev("invoke", "e", 11.0),
            ev("fail", "e", 12.0, reason="pread", errno=errno.EIO),
        ]
        statuses = _input_legality(spec, events)
        self.assertEqual(
            {
                "u": "satisfied",
                "t": "open",
                "h": "open",
                "m": f"not-evaluated {checker.CORE_INPUT_STATE_UNOBSERVED}",
                "s": "open",
                "q": "open",
                "p": "open",
                "g": "violated product",
                "e": "violated product",
            },
            statuses,
        )
        self.assertEqual(NO_ERRNO, checker.errno_name(None))


class InputSubjectTest(unittest.TestCase):
    """Every modeled input's `core.input-legality` subject."""

    def test_every_modeled_input_is_keyed_by_the_path_it_is_about(self) -> None:
        moved = "tree/registers/m"
        expected = {
            "open": (action_payload("open", path=R0), R0),
            "read_fd": (action_payload("read_fd", fd="open"), R0),
            "write": (action_payload("write", path=R0, value_id="w"), R0),
            "read": (action_payload("read", path=R0, channel="fuse"), R0),
            "getattr": (action_payload("getattr", path=R0), R0),
            "rename": (action_payload("rename", source=R0, destination=moved), R0),
            "unlink": (action_payload("unlink", path=R1), R1),
            "mkdir": (
                action_payload("mkdir", path="tree/registers/d"),
                "tree/registers/d",
            ),
            "chmod": (action_payload("chmod", path=R0, mode=0o755), R0),
            "readdir": (
                action_payload("readdir", path="tree/registers", channel="fuse"),
                "tree/registers",
            ),
            "status": (action_payload("status", channel="sl"), ""),
            "sl_rm": (action_payload("sl_rm", path=R0), R0),
            "sl_mv": (action_payload("sl_mv", source=R1, destination=moved), R1),
            "readdir_stream": (
                action_payload("readdir_stream", path="tree/wide", pause_after=2),
                "tree/wide",
            ),
            "readdir_resume": (
                action_payload("readdir_resume", stream="readdir_stream"),
                "tree/wide",
            ),
        }
        modeled = {
            kind
            for kind, definition in ACTION_DEFS.items()
            if definition.axis == "input" and definition.modeled
        }
        self.assertEqual(modeled, set(expected))
        actions = {kind: payload for kind, (payload, _path) in expected.items()}
        spec = run_spec(
            actions,
            [{"id": "a", "actions": list(actions)}],
            wide_dir={"name": "tree/wide", "entries": 4, "churn_all": True},
        )
        for kind, (_payload, path) in expected.items():
            with self.subTest(kind=kind):
                self.assertEqual(
                    path_subject(path),
                    checker.input_subject(VOCAB_TARGET, spec, spec.action(kind)),
                )


class _LegalityRecorder:
    """The checker's collector sink, reduced to the statuses one test reads."""

    def __init__(self) -> None:
        self.statuses: dict[str, str] = {}

    def obligation(
        self, rule_id: str, scope: str, *, subject: Any, witnesses: Any = ()
    ) -> ObligationRef:
        self.statuses[scope] = "open"
        return ObligationRef(rule_id, scope)

    def satisfied(self, ref: ObligationRef) -> None:
        self.statuses[ref.scope] = "satisfied"

    def violated(
        self, ref: ObligationRef, attribution: Any, reason: str, witnesses: Any = ()
    ) -> None:
        self.statuses[ref.scope] = f"violated {attribution}"

    def violated_unless_engine(
        self, ref: ObligationRef, attribution: Any, reason: str, witnesses: Any = ()
    ) -> None:
        self.statuses[ref.scope] = f"violated-unless-engine {attribution}"

    def not_evaluated(
        self, ref: ObligationRef, reason_id: str, detail: str = ""
    ) -> None:
        self.statuses[ref.scope] = f"not-evaluated {reason_id}"


def _input_legality(spec: RunSpec, events: list[Event]) -> dict[str, str]:
    """Run `core.input-legality` over the EdenFS declarations and contract inputs.

    An obligation a contract observation supports stays open: the engine
    decides it.
    """
    facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
    sink = RecordingSink()
    record_contracts(spec, facts, None, sink)
    effects = EffectSet(
        groups=tuple(sink.groups),
        pinned=EdenPinned.for_spec(spec),
        observations=tuple(sink.observations),
    )
    recorder = _LegalityRecorder()
    checker._collect_input_legality(
        VOCAB_TARGET, CheckInput(spec, facts, None, effects), recorder
    )
    return recorder.statuses


class ThriftReaddirErrorTest(unittest.TestCase):
    """A Thrift readdir error is matched on (errorType, errno)."""

    _DIR = "tree/registers"

    def _failed(self, error_type: str, code: int | None) -> Observation:
        """A Thrift readdir failing with `error_type` and, unless None, `code`."""
        numbered = {} if code is None else {"errno": code}
        _spec_, sink = contract_one(
            action_payload("readdir", path=self._DIR, channel="thrift"),
            ev("invoke", "x", 1.0),
            ev(
                "fail",
                "x",
                2.0,
                reason="thrift: x",
                source="thrift",
                error_type=error_type,
                **numbered,
            ),
        )
        (observation,) = sink.observations
        return observation

    def test_each_pair_means_its_state(self) -> None:
        file = Value("file", ("a",))
        absent = self._failed("POSIX_ERROR", errno.ENOENT)
        self.assertEqual((path_cell(self._DIR),), absent.cells)
        self.assertTrue(absent.expect.holds((ABSENT,)))
        self.assertFalse(absent.expect.holds((DIR,)))
        regular = self._failed("ARGUMENT_ERROR", errno.EINVAL)
        self.assertTrue(regular.expect.holds((file,)))
        self.assertFalse(regular.expect.holds((DIR,)))
        under = self._failed("POSIX_ERROR", errno.ENOTDIR)
        self.assertEqual((path_cell("tree"),), under.cells)
        self.assertTrue(under.expect.holds((file,)))
        self.assertFalse(under.expect.holds((DIR,)))

    def test_a_pair_the_probe_did_not_establish_means_no_state(self) -> None:
        for error_type, code in (
            ("POSIX_ERROR", errno.EINVAL),
            ("ARGUMENT_ERROR", errno.ENOENT),
            ("CHECKOUT_IN_PROGRESS", errno.ENOENT),
            ("CHECKOUT_IN_PROGRESS", None),
        ):
            with self.subTest(error_type=error_type, code=code):
                observation = self._failed(error_type, code)
                for value in (ABSENT, DIR, Value("file", ("a",))):
                    self.assertFalse(observation.expect.holds((value,)))


class TopLevelStateErrnoTest(unittest.TestCase):
    """Under the mount root no parent can be missing or a file."""

    def test_a_top_level_enotdir_or_missing_parent_is_legal_in_no_state(self) -> None:
        for payload, code in (
            (action_payload("write", path="top", value_id="x"), errno.ENOTDIR),
            (action_payload("getattr", path="top"), errno.ENOTDIR),
            (action_payload("mkdir", path="newdir"), errno.ENOENT),
        ):
            with self.subTest(kind=payload["kind"]):
                spec = run_spec(
                    {"x": payload},
                    [{"id": "a", "actions": ["x"]}],
                    registers=("top", R1),
                )
                sink = contract_sink(
                    spec,
                    [
                        ev("invoke", "x", 1.0),
                        ev("fail", "x", 2.0, reason="x", errno=code),
                    ],
                )
                (observation,) = sink.observations
                self.assertFalse(
                    any(
                        observation.expect.holds((value,))
                        for value in (ABSENT, DIR, committed_value("top", TIP))
                    )
                )


# Type-changing registers: directories holding one file at the tip.
_T = "tree/t"
_U = "tree/u"


# One input of a sequence: its id, its payload, and its completion event's
# type and facts.
_Step = tuple[str, dict[str, Any], str, dict[str, Any]]


def _sequence(*steps: _Step) -> tuple[RunSpec, list[Event]]:
    """One actor runs `steps` in order, each completing before the next starts."""
    actions, events = {}, []
    for time, (action_id, payload, done, facts) in enumerate(steps, start=1):
        actions[action_id] = payload
        events += [
            ev("invoke", action_id, float(time)),
            ev(done, action_id, time + 0.5, **facts),
        ]
    spec = run_spec(
        actions,
        [{"id": "a", "actions": list(actions)}],
        registers=(R0, _T, _U, R1),
        type_changes=((_T, 1), (_U, 1)),
    )
    return spec, events


def _outcome(code: int | None, **ok: Any) -> tuple[str, dict[str, Any]]:
    return ("ok", ok) if code is None else ("fail", {"reason": "x", "errno": code})


def _mkdir(action_id: str, path: str, code: int | None = None) -> _Step:
    return (action_id, action_payload("mkdir", path=path), *_outcome(code, path=path))


def _rename(
    action_id: str, source: str, destination: str, code: int | None = None
) -> _Step:
    payload = action_payload("rename", source=source, destination=destination)
    return (
        action_id,
        payload,
        *_outcome(code, source=source, destination=destination),
    )


def _unlink(action_id: str, path: str) -> _Step:
    return (action_id, action_payload("unlink", path=path), "ok", {"path": path})


# A mkdir of a generated directory lets a later input name a path in it.
_KNOWN_A0 = _mkdir("k", "tree/a0", errno.EEXIST)


class NonemptyDestinationTest(unittest.TestCase):
    """A rename fails `ENOTEMPTY`/`EEXIST` only onto a directory with a present child."""

    def test_onto_an_empty_created_directory_is_a_violation(self) -> None:
        for code in (errno.ENOTEMPTY, errno.EEXIST):
            with self.subTest(errno=errno.errorcode[code]):
                spec, events = _sequence(
                    _mkdir("m", "tree/d"), _rename("x", _T, "tree/d", code)
                )
                self.assertFalse(is_explained(spec, events))

    def test_a_directory_moved_into_place_makes_its_destination_nonempty(self) -> None:
        for name, steps, legal in (
            ("moved", (_rename("y", _T, "tree/n"),), True),
            ("moved twice", (_rename("y", _T, R1), _rename("z", R1, "tree/n")), True),
            ("made", (_mkdir("m", "tree/n"),), False),
        ):
            with self.subTest(case=name):
                spec, events = _sequence(
                    *steps, _rename("x", _U, "tree/n", errno.ENOTEMPTY)
                )
                self.assertEqual(legal, is_explained(spec, events))

    def test_onto_a_committed_directory_of_registers_needs_a_present_register(
        self,
    ) -> None:
        """`tree/registers` holds only the registers `R0` and `R1`."""
        for name, steps, legal in (
            ("present", (), True),
            ("removed", (_unlink("u", R0), _unlink("v", R1)), False),
        ):
            with self.subTest(case=name):
                spec, events = _sequence(
                    *steps, _rename("x", _T, "tree/registers", errno.ENOTEMPTY)
                )
                self.assertEqual(legal, is_explained(spec, events))

    def test_onto_a_committed_generated_directory_is_not_evaluated(self) -> None:
        """Untouched generated children are not all relevant paths."""
        for destination in ("tree/a0", "tree/a0/001"):
            with self.subTest(destination=destination):
                spec, events = _sequence(
                    _KNOWN_A0, _rename("x", _T, destination, errno.ENOTEMPTY)
                )
                self.assertEqual(
                    f"not-evaluated {checker.CORE_INPUT_STATE_UNOBSERVED}",
                    _input_legality(spec, events)["x"],
                )
