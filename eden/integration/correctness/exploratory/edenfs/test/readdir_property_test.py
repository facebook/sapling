# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The readdir property: a directory listing, streamed or one-shot, contains every entry
bound throughout it and nothing that is not a name. Complements the corpus family
`readdir`: these tests pin unknown entries, entries rebound but never unbound, and
entries absent throughout the stream.
"""

from __future__ import annotations

import dataclasses
import errno
import random
import unittest
from collections import Counter
from pathlib import PurePosixPath
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core import checker, semantics, solver
from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    eq,
    Expect,
    none_of,
    ObligationRef,
    Observation,
    Window,
)
from eden.integration.correctness.exploratory.core.property import CheckInput
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import path_cell
from eden.integration.correctness.exploratory.edenfs.properties import readdir
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    obligations_of,
    status_of,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import getattr_facts
from eden.integration.correctness.exploratory.test.corpus.author import (
    errno_fail,
    R0,
    R1,
    Run,
    sl_abort,
    sl_checkout,
    WRITE_OK,
)


def _one_shot(channels: tuple[str, ...], entries: list[str]) -> Run:
    """One-shot listings of the registers' directory, one channel after another."""
    run = Run("t", "one-shot-" + "-".join(channels), "one-shot listings")
    for index, channel in enumerate(channels):
        name = f"l{index}"
        run.act("a", name, "readdir", path="tree/registers", channel=channel)
        run.call(name, "ok", channel=channel, entries=entries)
    run.finish()
    return run


def _stream_racing_a_rename(*, pinned_before: bool, listed: bool) -> Run:
    """The same race across a drained stream of the wide directory."""
    wide = {"name": "tree/wide", "entries": 2, "churn_all": False}
    run = Run("t", "stream-racing-rename", "a stream across a rename", wide_dir=wide)
    names = [item.path.removeprefix("tree/wide/") for item in run.layout().wide_files()]
    run.act("b", "m", "rename", source=R0, destination="tree/wide/x")
    run.act("a", "d0", "readdir_stream", path="tree/wide", pause_after=1)
    run.act("a", "d1", "readdir_resume", stream="d0")
    run.act("c", "g", "getattr", path=R0)
    run.inv("m")
    if pinned_before:
        run.call("g", "fail", **errno_fail(errno.ENOENT, "stat"))
    run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
    entries = [*names, "x"] if listed else names
    run.call("d1", "ok", stream="d0", entries=entries, pos_at_pause=1, pos_at_end=3)
    if not pinned_before:
        run.call("g", "ok", **getattr_facts())
    run.done("m", "ok", source=R0, destination="tree/wide/x")
    run.finish()
    return run


def _stream_racing_a_rename_onto_an_entry(*, failed: bool) -> Run:
    """A drained stream of the wide directory that omits the entry a racing rename
    targets; the rename fails with EIO (no effect) or succeeds."""
    wide = {"name": "tree/wide", "entries": 2, "churn_all": False}
    run = Run(
        "t", "stream-racing-rename-onto", "a stream across a rename", wide_dir=wide
    )
    names = [item.path.removeprefix("tree/wide/") for item in run.layout().wide_files()]
    target = f"tree/wide/{names[1]}"
    run.act("b", "m", "rename", source=R0, destination=target)
    run.act("a", "d0", "readdir_stream", path="tree/wide", pause_after=1)
    run.act("a", "d1", "readdir_resume", stream="d0")
    run.inv("m")
    run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
    run.call("d1", "ok", stream="d0", entries=names[:1], pos_at_pause=1, pos_at_end=2)
    if failed:
        run.done("m", "fail", **errno_fail(errno.EIO, "rename"))
    else:
        run.done("m", "ok", source=R0, destination=target)
    run.finish()
    return run


def _stream_after_sl_goto(*, aborted: bool) -> Run:
    """An sl goto (or its abort naming clean r0), mkdir of tree/wide/sub, then a
    drained stream of tree/wide that omits sub."""
    wide = {"name": "tree/wide", "entries": 2, "churn_all": False}
    run = Run("t", "stream-after-sl-goto", "a stream after an sl goto", wide_dir=wide)
    names = [item.path.removeprefix("tree/wide/") for item in run.layout().wide_files()]
    run.act("a", "c", "checkout", revision=1, via="goto", mode="normal")
    run.act("a", "m", "mkdir", path="tree/wide/sub")
    run.act("a", "d0", "readdir_stream", path="tree/wide", pause_after=1)
    run.act("a", "d1", "readdir_resume", stream="d0")
    if aborted:
        run.call("c", "fail", **sl_abort([R0]))
    else:
        run.call("c", "ok", **sl_checkout(1))
    run.call("m", "ok", path="tree/wide/sub")
    run.call("d0", "ok", pgid=1, paused_at=1, pos_at_pause=1)
    run.call("d1", "ok", stream="d0", entries=names, pos_at_pause=1, pos_at_end=2)
    run.finish()
    return run


class OneShotReaddirTest(unittest.TestCase):
    """Plain one-shot listings are judged by the readdir rules."""

    def test_channels_agreeing_on_a_wrong_listing_are_rejected(self) -> None:
        """FUSE and Thrift both list r2 and a name nothing binds."""
        for channels in (("fuse",), ("fuse", "thrift")):
            with self.subTest(channels=channels):
                verdict = judge_isolated(
                    ("readdir",), _one_shot(channels, ["r2.reg", "bogus-name"])
                )
                self.assertEqual("reject", str(verdict.kind))
                for index in range(len(channels)):
                    for rule in ("readdir.unknown-entry", "readdir.missing-entry"):
                        self.assertEqual(
                            "violated", status_of(verdict, rule, f"l{index}")
                        )

    def test_an_undecided_stream_path_is_not_evaluated(self) -> None:
        """The engine cannot place a stream's span."""
        for pinned_before, listed, rule in (
            (False, True, "readdir.unknown-entry"),
            (True, False, "readdir.missing-entry"),
        ):
            with self.subTest(pinned_before=pinned_before):
                verdict = judge_isolated(
                    ("readdir",),
                    _stream_racing_a_rename(pinned_before=pinned_before, listed=listed),
                )
                obligation = obligations_of(verdict)[(rule, "d1")]
                self.assertEqual("not-evaluated", str(obligation.status))
                self.assertEqual(
                    "readdir.membership-undecided", obligation.not_evaluated_reason
                )

    def test_only_a_rename_that_may_have_landed_excuses_an_omitted_entry(self) -> None:
        """A rename onto an entry excuses a stream omitting it unless it failed with no effect."""
        for failed, expected in ((True, "violated"), (False, "satisfied")):
            with self.subTest(failed=failed):
                verdict = judge_isolated(
                    ("readdir",), _stream_racing_a_rename_onto_an_entry(failed=failed)
                )
                self.assertEqual(
                    expected, status_of(verdict, "readdir.missing-entry", "d1")
                )

    def test_a_stream_probe_needs_consistent_contract_inputs(self) -> None:
        """The engine probes a stream's path only when the contract inputs are satisfiable.

        The acked mkdir binds sub before the stream, so omitting it is a
        missing entry; an sl abort naming clean r0 leaves no explanation of
        the contract inputs at all, so the probe decides nothing.
        """
        for aborted, expected in ((False, "violated"), (True, "not-evaluated")):
            with self.subTest(aborted=aborted):
                verdict = judge(_stream_after_sl_goto(aborted=aborted))
                self.assertEqual(
                    expected, status_of(verdict, "readdir.missing-entry", "d1")
                )

    def test_a_stream_probe_out_of_work_decides_nothing(self) -> None:
        """The missing-entry stream above, with every probe of `d1` out of work."""
        real = solver.satisfiable

        def probes_run_out(
            problem: semantics.Problem, **kwargs: Any
        ) -> tuple[str, int]:
            last = problem.observations[-1:]
            if last and last[0].id.startswith("o:d1:"):
                return "unknown", 0
            return real(problem, **kwargs)

        with mock.patch.object(solver, "satisfiable", side_effect=probes_run_out):
            verdict = judge(_stream_after_sl_goto(aborted=False))
        self.assertEqual(
            "not-evaluated", status_of(verdict, "readdir.missing-entry", "d1")
        )


def _listing_history(seed: int) -> tuple[Run, list[str]]:
    """`_replay_history`'s namespace steps, then a one-shot listing of the
    registers' directory naming r2 and a random subset of r0 and r1."""
    rng = random.Random(seed)
    run = Run("t", f"listing-{seed}", "random namespace history, then a listing")
    present = {R0: True, R1: True}
    steps: list[tuple[str, dict[str, Any]]] = []
    for index in range(rng.randint(0, 4)):
        name = f"x{index}"
        live = [path for path, here in present.items() if here]
        if not live:
            break
        kind = rng.choice(["write", "unlink", "rename"])
        target = rng.choice(live)
        actor = "ab"[index % 2]
        if kind == "write":
            run.act(actor, name, kind, path=target, value_id=name)
            steps.append((name, WRITE_OK))
            continue
        if kind == "unlink":
            present[target] = False
            payload: dict[str, Any] = {"path": target}
        else:
            other = R1 if target == R0 else R0
            present[target], present[other] = False, True
            payload = {"source": target, "destination": other}
        run.act(actor, name, kind, **payload)
        steps.append((name, payload))
    entries = ["r2.reg", *(name for name in ("r0.reg", "r1.reg") if rng.random() < 0.5)]
    channel = rng.choice(["fuse", "thrift"])
    run.act("c", "l", "readdir", path="tree/registers", channel=channel)
    pending: list[tuple[str, dict[str, Any]]] = []
    for name, facts in [*steps, ("l", {})]:
        run.inv(name)
        for earlier, earlier_facts in pending:
            if rng.random() < 0.3:
                run.done(earlier, "info", reason="timed out")
            else:
                run.done(earlier, "ok", **earlier_facts)
        pending = [(name, facts)] if name != "l" and rng.random() < 0.3 else []
        if name == "l":
            break
        if not pending:
            run.done(name, "ok", **facts)
    run.done("l", "ok", channel=channel, entries=entries)
    run.finish()
    return run, entries


class ListingParityTest(unittest.TestCase):
    """`readdir.membership`, behind the one-shot readdir rules and the differential
    `_listing_legality` (`_ILLEGAL` / `_LEGAL`), against the engine.

    The engine judges the listing as one observation per relevant entry of
    the directory inside the call (listed: bound at some point; unlisted:
    unbound at some point) with every contract input. A direct violation
    must be unsatisfiable and a fully decided listing satisfiable; an
    undecided one is judged by the engine through the readdir property's
    own observations, so its verdict must match too.
    """

    def test_listing_judgment_agrees_with_the_engine(self) -> None:
        tally: Counter[str] = Counter()
        for seed in range(300):
            run, entries = _listing_history(seed)
            spec = from_json(run.spec_text(), EDENFS_TARGET)
            facts = checker.HistoryFacts.build(
                spec, tuple(run.events), EDENFS_TARGET, execution_status="completed"
            )
            derived = checker.contract_effects(
                EDENFS_TARGET, spec, facts, CONTRACT_RULES
            )
            if isinstance(derived, checker.PropertyError):
                self.fail(f"seed {seed}: contract effects failed: {derived}")
            pinned, effect_set = derived
            check_input = CheckInput(spec, facts, pinned, effect_set)
            listing = facts.action("l")
            if listing.invoked is None or listing.completed is None:
                self.fail(f"seed {seed}: listing l did not complete")
            window = Window(listing.invoked, listing.completed)
            domain = readdir.Domain(check_input, "tree/registers")
            found = readdir.membership(domain, entries)
            ref = ObligationRef("readdir.unknown-entry", "l")
            claims = [
                Observation(
                    id=f"o:l:parity:{index}",
                    action="l",
                    cells=(path_cell(path),),
                    expect=Expect(((none_of({ABSENT}) if listed else eq(ABSENT),),)),
                    window=window,
                    channel="fuse",
                    obligations=(ref,),
                )
                for index, path in enumerate(sorted(domain.relevant))
                if PurePosixPath(path).parent.as_posix() == "tree/registers"
                for listed in (PurePosixPath(path).name in entries,)
            ]
            base = semantics.Problem(
                effect_set.groups,
                effect_set.observations,
                effect_set.constraints,
                effect_set.initial,
            )
            self.assertEqual("sat", solver.satisfiable(base, work=10**6)[0], seed)
            problem = dataclasses.replace(
                base, observations=(*effect_set.observations, *claims)
            )
            engine, _work = solver.satisfiable(problem, work=10**6)
            self.assertIn(engine, ("sat", "unsat"), seed)
            if found.unknown or found.missing:
                direct = "unsat"
            elif found.undecided_listed or found.undecided_unlisted:
                direct = "undecided"
            else:
                direct = "sat"
            tally[f"{direct}/{engine}"] += 1
            with self.subTest(seed=seed, direct=direct):
                if direct != "undecided":
                    self.assertEqual(direct, engine)
                    continue
                verdict = judge(run)
                statuses = {
                    status_of(verdict, rule, "l")
                    for rule in ("readdir.unknown-entry", "readdir.missing-entry")
                }
                self.assertEqual(
                    engine == "unsat", "violated" in statuses, (statuses, engine)
                )
        for kind in ("sat/sat", "unsat/unsat", "undecided/sat", "undecided/unsat"):
            self.assertGreater(tally[kind], 5, (kind, dict(sorted(tally.items()))))
