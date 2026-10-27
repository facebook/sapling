# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Invariants of the contracts as a whole: folding a settled checkout's encoding never
changes what the engine explains (a randomized parity check against the unfolded
encoding), and the action declarations (builders, derived kind sets, state errno
meanings, path fields) agree with the contracts.
"""

from __future__ import annotations

import dataclasses
import errno
import random
import unittest
from collections import Counter
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    eq,
    Expect,
    normalize_path,
    ObligationRef,
    Observation,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ErrnoUse,
    HistoryFacts,
    NO_ERRNO,
)
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.target import (
    Binding,
    MUTATING_ROLES,
    Role,
)
from eden.integration.correctness.exploratory.core.test.testing import ev
from eden.integration.correctness.exploratory.edenfs import contracts
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    MOVE_KINDS,
)
from eden.integration.correctness.exploratory.edenfs.contracts import (
    checkout as contracts_checkout,
    files as contracts_files,
    model as contracts_model,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    UNJUDGED_STATE_ERRNOS,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    EdenPinned,
    file_value,
    parent_cell,
    path_cell,
    rev_value,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.test.contracts_fixtures import (
    action_payload,
    checkout_op,
    committed_value,
    contract_sink,
    is_explained,
    R0,
    R1,
    read_obs,
    run_spec,
    SEED,
    sl_abort_facts,
    SL_OK,
    THRIFT_OK,
    TIP,
    WALK_OVERLAP,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import VOCAB_TARGET


_PLANS = contracts_checkout._plans


def _unfolded_plans(*args: Any) -> dict[str, Any]:
    """`contracts.checkout._plans` with nothing settled: every checkout reads marks and the parent."""
    plans = _PLANS(*args)
    return {
        checkout: dataclasses.replace(
            plan,
            settled=None,
            serialized=True,
            resumes=tuple(
                dataclasses.replace(resume, source=None) for resume in plan.resumes
            ),
        )
        for checkout, plan in plans.items()
    }


class SettledFoldingParityTest(unittest.TestCase):
    """A settled checkout's folded encoding explains exactly what the full one does.

    Generated histories are judged by the engine twice: with
    `_plans` as the contracts computes it, and with every checkout unsettled and
    serialized (marks and the parent read everywhere, the encoding before
    folding). Each history aims at one condition folding relies on (`_shape`)
    and is judged several ways: every conflict count or sl outcome of the
    checkout under test, and every probe that tells its source apart.
    """

    _CASES = 40
    # Larger inputs cost the engine seconds each.
    _NODES = 12

    def _shape(  # noqa: C901 - one generated history per family, step by step
        self, rng: random.Random, case: int
    ) -> list[tuple[RunSpec, list[Event], tuple[Observation, ...]]]:
        """One history per `case % 5`, then a dirtying write, chmod, or unlink of r0
        (or a status overlapping it) and the checkout `c` under test.

        0: two acked checkouts overlap each other, `c` goes to one's revision;
        1: an indeterminate checkout may land any time before `c`; 2: it ends
        at a daemon exit before `c`; 3: a status overlaps `c`, whose probes read
        the parent with r0 at `c`'s target; 4: one acked checkout before `c`,
        which goes to its revision or the initial one. In 1 and 2 `c` goes to
        the revision held before the indeterminate one.
        """
        family = case % 5
        via = "goto" if rng.random() < 0.3 else "thrift"
        actions: dict[str, dict[str, Any]] = {}
        timed: list[tuple[str, float, float, str, dict[str, Any]]] = []
        runs: list[Event] = []

        def checkout(name: str, target: int, lo: float, hi: float, info: bool) -> None:
            sl = name == "c" and via == "goto"
            actions[name] = checkout_op(target, via="goto" if sl else "thrift")
            ok = {"revision": target, **(SL_OK if sl else THRIFT_OK)}
            done = {"reason": "deadline"} if info else ok
            timed.append((name, lo, hi, "info" if info else "ok", done))

        a, b = rng.sample(range(TIP + 1), 2)
        target = rng.randrange(TIP + 1)
        if family == 0:
            checkout("p0", a, 0.5, 2.5, False)
            checkout("p1", b, rng.choice((1.0, 2.0)), 3.0, False)
            target = rng.choice((a, b))
        elif family in (1, 2):
            if rng.random() < 0.5:
                checkout("p0", a, 0.5, 1.5, False)
            else:
                a = TIP
            checkout("p1", b if b != a else (a + 1) % (TIP + 1), 2.0, 3.0, True)
            if family == 2:
                runs.append(Event("info", 3.5, {"reason": "daemon-exit"}))
            target = a
        elif family == 3:
            target = rng.randrange(TIP)
        else:
            a = rng.randrange(TIP)
            checkout("p0", a, 0.5, 2.0, False)
            target = rng.choice((a, TIP))
        if family == 3:
            actions["s"] = action_payload("status", channel="sl")
            timed.append(("s", 5.5, 7.5, "ok", {"status": [], "diff_paths": []}))
        else:
            kind = rng.choice(("write", "write", "chmod", "unlink"))
            if kind == "write":
                actions["m"] = action_payload("write", path=R0, value_id="m")
                done: dict[str, Any] = {"bytes_written": 64}
            elif kind == "chmod":
                actions["m"] = action_payload("chmod", path=R0, mode=0o755)
                done = {"path": R0, "mode": 0o755}
            else:
                actions["m"] = action_payload("unlink", path=R0)
                done = {"path": R0}
            timed.append(("m", 4.0, 4.5, "ok", done))
        checkout("c", target, 5.0, 8.0, False)
        spec = run_spec(
            actions,
            [{"id": f"a{name}", "actions": [name]} for name in actions],
            registers=(R0,),
            metadata=WALK_OVERLAP,
        )
        if family == 3:
            results: list[tuple[str, dict[str, Any]]] = [("ok", {})]
        elif via == "thrift":
            results = [("ok", {"conflicts": 0}), ("ok", {"conflicts": 1})]
        else:
            results = [("ok", {}), ("fail", sl_abort_facts(target, R0))]
        found = []
        for outcome, change in results:
            events = list(runs)
            for name, lo, hi, kind, done in timed:
                if name == "c":
                    kind = outcome
                    done = {**done, **change} if outcome == "ok" else change
                events += [ev("invoke", name, lo), ev(kind, name, hi, **done)]
            events.sort(key=lambda event: event.time)
            if family == 3:
                found += [
                    (spec, events, (_probe(rng.uniform(5.5, 7.0), R0, target, parent),))
                    for parent in range(TIP + 1)
                ]
                continue
            found.append((spec, events, ()))
            for value in (ABSENT, committed_value(R0, target), _WRITTEN_M):
                found.append((spec, events, (read_obs("r", R0, value, 9.0, 9.5),)))
        return found

    def test_folding_never_changes_what_is_explained(self) -> None:
        rng = random.Random(20261023)
        verdicts: Counter[tuple[bool, bool]] = Counter()
        families: Counter[int] = Counter()
        folded = judged = 0
        for case in range(self._CASES):
            for spec, events, probes in self._shape(rng, case):
                facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
                boundary = contracts_model.resolution_boundary(facts)
                plans = contracts_checkout._plans(
                    spec, facts, EdenPinned.for_spec(spec), boundary
                )
                settled = any(plan.settled is not None for plan in plans.values())
                sink = contract_sink(spec, events)
                if (
                    len(sink.groups) + len(sink.observations) + len(probes)
                    > self._NODES
                ):
                    continue
                try:
                    mine = _explained(spec, events, probes)
                    with mock.patch.object(
                        contracts_checkout, "_plans", _unfolded_plans
                    ):
                        reference = _explained(spec, events, probes)
                except ValueError:
                    continue
                judged += 1
                folded += settled
                families[case % 5] += 1
                with self.subTest(case=case, probes=[p.id for p in probes]):
                    self.assertEqual(reference, mine)
                verdicts[(mine, settled)] += 1
        self.assertEqual(set(range(5)), set(families))
        self.assertGreater(folded, judged // 3)
        self.assertGreater(verdicts[(True, True)], 0)
        self.assertGreater(verdicts[(False, True)], 0)


_WRITTEN_M = file_value(encode_value(SEED, "am", "m"))


def _probe(lo: float, path: str, target: int, parent: int) -> Observation:
    """A status inside `lo..lo+0.5` reading the parent at `parent` with `path` at `target`."""
    return Observation(
        id="r",
        action="r",
        cells=(parent_cell(), path_cell(path)),
        expect=Expect(((eq(rev_value(parent)), eq(committed_value(path, target))),)),
        window=Window(lo, lo + 0.5),
        channel="sl",
        obligations=(ObligationRef("status.parent", "r"),),
    )


def _explained(
    spec: RunSpec, events: list[Event], probes: tuple[Observation, ...]
) -> bool:
    """Whether the contract inputs and `probes` have an explanation.

    A nonzero conflict count with no derived count observation (no path
    visit) is unexplained: the checkout property judges it directly.
    """
    sink = contract_sink(spec, events)
    counted = {o.action for o in sink.observations if o.id.endswith(":conflicts")}
    uncounted = any(
        event.type == "ok"
        and event.data.get("conflicts")
        and event.data["action"] not in counted
        for event in events
    )
    return not uncounted and is_explained(spec, events, probes)


def _builder_gaps(builders: Any, definitions: Any) -> set[str]:
    """Return the kinds whose declared roles mutate state but that have no builder, and the reverse."""
    mutating = {
        kind
        for kind, definition in definitions.items()
        if definition.roles & MUTATING_ROLES
    }
    return mutating ^ set(builders)


class DeclarationParityTest(unittest.TestCase):
    """Every hand table in the contracts agrees with the `ActionDef` declarations."""

    def test_every_mutating_kind_has_a_builder_and_no_other_does(self) -> None:
        self.assertEqual(set(), _builder_gaps(contracts._GROUPS, ACTION_DEFS))
        missing = {k: v for k, v in contracts._GROUPS.items() if k != "chmod"}
        self.assertEqual({"chmod"}, _builder_gaps(missing, ACTION_DEFS))

    def test_contract_kind_sets_follow_roles_and_bindings(self) -> None:
        """Each kind set equals a hand list a declaration change must update deliberately."""
        self.assertEqual(
            {"write", "rename", "unlink", "mkdir", "chmod"},
            set(contracts_model.FUSE_MUTATIONS),
        )
        # The kinds whose results list an aggregate: a directory, a status,
        # a manifest, or a journal window.
        self.assertEqual(
            {
                "status",
                "readdir_stream",
                "readdir",
                "readdir_resume",
                "manifest_presence",
                "journal_window",
                "status_counts",
            },
            set(contracts_model._AGGREGATE_KINDS),
        )
        self.assertEqual(
            {
                kind
                for kind, definition in ACTION_DEFS.items()
                if Role.TRANSFER in definition.roles
            },
            set(MOVE_KINDS),
        )
        self.assertEqual({"rename", "sl_mv"}, set(MOVE_KINDS))

    def test_fuse_indeterminate_errnos_are_declared_non_state(self) -> None:
        for kind in contracts_model.FUSE_MUTATIONS:
            declared = ACTION_DEFS[kind].result.errnos
            for code in contracts_model._FUSE_INDETERMINATE_ERRNOS:
                with self.subTest(kind=kind, errno=errno.errorcode[code]):
                    self.assertIn(
                        declared.get(errno.errorcode[code]),
                        (ErrnoUse.FAULT, ErrnoUse.TIMEOUT),
                    )

    def test_failure_tables_follow_the_declared_failures(self) -> None:
        """`_UNOPENED` and `_GUARD_TYPES` name exactly the declared failures they classify."""
        self.assertEqual(
            {
                (kind, "EBADF")
                for kind, definition in ACTION_DEFS.items()
                if definition.modeled
                and definition.binding == Binding.DESCRIPTOR
                and definition.result.errnos.get("EBADF") == ErrnoUse.STATE
            },
            set(contracts_files._UNOPENED),
        )
        self.assertLessEqual(
            contracts_files._UNOPENED, contracts_files.UNJUDGED_STATE_ERRNOS
        )
        status_types = ACTION_DEFS["status"].result.error_types
        self.assertEqual(
            set(status_types),
            contracts_checkout._GUARD_TYPES | {contracts_model._OUT_OF_DATE_PARENT},
        )
        self.assertNotIn(
            contracts_model._OUT_OF_DATE_PARENT, contracts_checkout._GUARD_TYPES
        )
        self.assertTrue(all(use == ErrnoUse.STATE for use in status_types.values()))

    def test_state_meanings_cover_exactly_the_declared_state_errnos(self) -> None:
        declared = {
            (kind, name)
            for kind, definition in ACTION_DEFS.items()
            if definition.modeled
            for name, use in definition.result.errnos.items()
            if use == ErrnoUse.STATE
        }
        # An errno-less failure no state allows is the `_FORBIDDEN` meaning.
        meanings = {
            (kind, NO_ERRNO if name == contracts_files._FORBIDDEN else name)
            for kind, name in contracts_files._STATE_MEANINGS
        }
        self.assertLessEqual(meanings, declared)
        self.assertEqual(set(), declared - meanings - UNJUDGED_STATE_ERRNOS)
        self.assertLessEqual(UNJUDGED_STATE_ERRNOS, declared)
        read = ACTION_DEFS["read"].result.errnos
        self.assertEqual(
            {
                name
                for name, use in read.items()
                if use == ErrnoUse.STATE and name != NO_ERRNO
            },
            set(contracts_files._THRIFT_READ_STATE),
        )
        readdir = ACTION_DEFS["readdir"].result.errnos
        self.assertTrue(
            all(
                readdir[name] == ErrnoUse.STATE
                for name in contracts_files._THRIFT_READDIR_STATE.values()
            )
        )

    def test_path_fields_name_every_path_a_footprint_names(self) -> None:
        payloads = {
            "write": action_payload("write", path=R0, value_id="w"),
            "read": action_payload("read", path=R0, channel="fuse"),
            "getattr": action_payload("getattr", path=R0),
            "open": action_payload("open", path=R0),
            "chmod": action_payload("chmod", path=R0, mode=0o644),
            "rename": action_payload(
                "rename", source=R0, destination="tree/registers/m"
            ),
            "unlink": action_payload("unlink", path=R1),
            "mkdir": action_payload("mkdir", path="tree/registers/d"),
            "sl_rm": action_payload("sl_rm", path=R0),
            "sl_mv": action_payload("sl_mv", source=R1, destination="tree/registers/m"),
            "readdir": action_payload("readdir", path="tree/registers", channel="fuse"),
        }
        path_bound = {
            kind
            for kind, definition in ACTION_DEFS.items()
            if definition.binding == Binding.PATH
        }
        self.assertLessEqual(path_bound, set(payloads))
        spec = run_spec(
            {f"x{kind}": payload for kind, payload in payloads.items()},
            [{"id": f"a{kind}", "actions": [f"x{kind}"]} for kind in payloads],
        )
        for action in spec.actions:
            with self.subTest(kind=action.kind):
                named = {
                    normalize_path(action.payload[name])
                    for name in contracts_model._PATH_FIELDS
                    if isinstance(action.payload.get(name), str)
                }
                footprint = ACTION_DEFS[action.kind].footprint(spec, action)
                self.assertLessEqual(footprint.paths() - {""}, named)
