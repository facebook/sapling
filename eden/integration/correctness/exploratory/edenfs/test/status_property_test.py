# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The status property: sl status, its sl diff, and Thrift getScmStatusV2 against the
path, mode, track, and parent cells, plus what a failed status says about state.
Complements the corpus families `bytes-status`, `aggregate-omission`, and the status
rows of `errno-twins`: these tests pin the code-function preimage rows, unmodeled
codes, entries outside the relevant paths, status and diff observed at two points,
which stale-status rule a disagreement files under, typed refusals and their texts,
and the recorded live Sapling outputs.
"""

from __future__ import annotations

import errno
import itertools
import unittest
from collections.abc import Mapping
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.effects import Expect
from eden.integration.correctness.exploratory.core.facts import ActionFacts
from eden.integration.correctness.exploratory.core.property import ObligationStatus
from eden.integration.correctness.exploratory.core.runspec import from_json
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    relevant_paths,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    EdenPinned,
    rev_value,
)
from eden.integration.correctness.exploratory.edenfs.properties import (
    status as status_property,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    obligations_of,
    status_of,
    violations_of,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    errno_fail,
    R0,
    R1,
    R2,
    read_ok,
    Run,
    sl_status,
    thrift_checkout,
    thrift_status,
    TIP,
    value,
    WRITE_OK,
)


class StatusRowsTest(unittest.TestCase):
    def _probes(self, spec: Any) -> list[Any]:
        pinned = EdenPinned.for_spec(spec)
        found = []
        for sl, revision, candidates in itertools.product(
            (True, False), (None, 0, TIP), ((TIP,), (TIP, 2, 0))
        ):
            found.append(
                status_property._Probe(
                    action=ActionFacts("s", "status", "a", {"channel": "sl"}),
                    sl=sl,
                    revision=revision,
                    candidates=candidates,
                    initial=TIP,
                )
            )
        return [(probe, pinned) for probe in found]

    def test_rows_are_exactly_the_code_functions_preimage(self) -> None:
        """Each state class satisfies the rows of a code iff the channel may report it."""
        run = Run("t", "rows", "a write of r0")
        run.act("a", "w", "write", path=R0, value_id="w")
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        for probe, pinned in self._probes(spec):
            codes = (
                status_property.SL_CODES if probe.sl else status_property.THRIFT_CODES
            )
            for code in (*codes, status_property.CLEAN_CODE):
                expect = Expect(status_property._rows(probe, pinned, R0, code))
                for state in status_property._states(probe, pinned, R0):
                    values = (state.value, state.mode)
                    if probe.sl:
                        values += (state.track,)
                    if probe.multi:
                        values += (rev_value(state.eden),)
                        if probe.reads_sapling:
                            values += (rev_value(state.sapling),)
                    allowed = status_property._codes(probe, pinned, R0, state)
                    with self.subTest(probe=probe, code=code, state=state):
                        self.assertEqual(code in allowed, expect.holds(values))

    def test_every_sl_state_has_a_code(self) -> None:
        run = Run("t", "codes", "a write of r0")
        run.act("a", "w", "write", path=R0, value_id="w")
        spec = from_json(run.spec_text(), EDENFS_TARGET)
        for probe, pinned in self._probes(spec):
            if not probe.sl:
                continue
            for state in status_property._states(probe, pinned, R0):
                with self.subTest(probe=probe, state=state):
                    self.assertTrue(status_property._codes(probe, pinned, R0, state))


def _status_run(name: str, entries: Mapping[str, str], **status: Any) -> Run:
    run = Run("t", name, "a write then a status")
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "s", "status", requires=[("w", "finished")], **status)
    run.call("w", "ok", **WRITE_OK)
    return run


class StatusTest(unittest.TestCase):
    def test_an_unmodeled_code_is_not_evaluated(self) -> None:
        run = _status_run("ignored", {}, channel="thrift", revision=TIP)
        run.call("s", "ok", **thrift_status({R0: "IGNORED"}))
        run.finish()
        item = obligations_of(judge_isolated(("status",), run))[
            ("status.thrift-model-stale-status", f"s:{R0}")
        ]
        self.assertEqual(
            (ObligationStatus.NOT_EVALUATED, status_property.UNMODELED_CODE),
            (item.status, item.not_evaluated_reason),
        )

    def test_a_listed_untouched_path_no_class_represents_is_judged(self) -> None:
        def status_only(entries: Mapping[str, str], diff: list[str]) -> Run:
            run = Run("t", "phantom", "a status with no mutation")
            run.act("a", "s", "status", channel="sl")
            run.call("s", "ok", **sl_status(entries, diff))
            run.finish()
            return run

        spec = from_json(status_only({}, []).spec_text(), EDENFS_TARGET)
        pinned = EdenPinned.for_spec(spec)
        layout = pinned.fixture.layout
        unrepresented = next(
            path
            for i in range(layout.file_count)
            if (path := layout.file_at(i, layout.commit_count - 1).path)
            not in relevant_paths(spec, pinned)
        )
        for code in ("M", "A", "R"):
            verdict = judge(status_only({unrepresented: code}, [unrepresented]))
            self.assertEqual(checker.REJECT, verdict.kind, code)
            self.assertTrue(
                _status_findings(verdict, unrepresented), (code, unrepresented)
            )

    def test_sl_diff_never_lists_a_missing_path(self) -> None:
        run = Run("t", "unlink-status", "an unlink then sl status")
        run.act("a", "u", "unlink", path=R0)
        run.act("a", "s", "status", channel="sl")
        run.call("u", "ok", path=R0)
        run.call("s", "ok", **sl_status({R0: "!"}, [R0]))
        run.finish()
        self.assertEqual(
            {("status.status-diff-mismatch", (("observed", "!"),), "undetermined")},
            {(r, w[:1], a) for r, w, a in _status_findings(judge(run), R0)},
        )


_SL_FUSE = "status.sl-fuse-stale-status"


def _status_findings(
    verdict: checker.Verdict, path: str
) -> set[tuple[str, tuple[tuple[str, str], ...], str]]:
    """Return each violated status obligation on `path`: rule, witnesses, attribution."""
    return {
        (
            rule,
            tuple(sorted((w.role, w.value) for w in item.witnesses)),
            str(item.attribution),
        )
        for (rule, scope), item in obligations_of(verdict).items()
        if rule.startswith("status.")
        and scope.endswith(f":{path}")
        and item.status == ObligationStatus.VIOLATED
    }


class StatusDiffWindowTest(unittest.TestCase):
    """sl status and its `sl diff` are two commands, observed at two points."""

    def test_a_status_without_diff_files_alone_and_stays_product(self) -> None:
        run = _status_run("no-diff", {}, channel="sl")
        result = sl_status({})
        del result["diff_paths"]
        run.call("s", "ok", **result)
        run.finish()
        self.assertEqual(
            {
                ("status.sl-model-stale-status", f"s:{R0}"): "product",
                # No backend records an sl status without its diff.
                ("result-shape.channel-fields", "s"): "harness",
            },
            violations_of(judge(run)),
        )

    def _omission(self, name: str, read: str | None) -> Run:
        """An acked write of r0, an sl status and diff omitting r0, and a FUSE
        read of r0 returning the write's bytes after the status (`after`), on
        another actor while the status runs (`during`), or none."""
        run = Run("t", name, "status loses an acked write")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "s", "status", channel="sl", requires=[("w", "finished")])
        if read is not None:
            run.act(
                "c", "r", "read", path=R0, channel="fuse", requires=[("w", "finished")]
            )
        run.call("w", "ok", **WRITE_OK)
        if read == "during":
            run.inv("s", "r")
            run.done("r", "ok", **read_ok(value("a", "w")))
            run.done("s", "ok", **sl_status({}, diff=[]))
        else:
            run.call("s", "ok", **sl_status({}, diff=[]))
            if read == "after":
                run.call("r", "ok", **read_ok(value("a", "w")))
        run.finish()
        return run

    def test_an_omission_files_one_rule_whatever_other_channels_saw(self) -> None:
        """Neither a FUSE read of the written bytes nor when
        it ran relative to the status chooses the rule, witnesses, or
        attribution of a status omission."""
        found = {
            read: _status_findings(
                judge_isolated(("status",), self._omission(f"omit-{read}", read)), R0
            )
            for read in (None, "after", "during")
        }
        expected = {
            (
                _SL_FUSE,
                (("observed", "absent"), ("producer-kinds", "write")),
                "undetermined",
            )
        }
        self.assertEqual({None: expected, "after": expected, "during": expected}, found)

    def test_the_omitted_change_kind_is_identity(self) -> None:
        """A lost write and a lost unlink name different producer
        kinds, from the actions' declared footprints, not from when they ran."""
        found = {}
        for kind in ("write", "unlink", "both"):
            run = Run("t", f"lost-{kind}", "a mutation of r0 status omits")
            requires: list[tuple[str, str]] = []
            if kind in ("write", "both"):
                run.act("a", "w", "write", path=R0, value_id="w")
                requires.append(("w", "finished"))
            if kind in ("unlink", "both"):
                run.act("a", "u", "unlink", path=R0, requires=list(requires))
                requires.append(("u", "finished"))
            run.act("a", "s", "status", channel="sl", requires=requires)
            if kind in ("write", "both"):
                run.call("w", "ok", **WRITE_OK)
            if kind in ("unlink", "both"):
                run.call("u", "ok", path=R0)
            run.call("s", "ok", **sl_status({}, diff=[]))
            run.finish()
            ((rule, witnesses, _attribution),) = _status_findings(
                judge_isolated(("status",), run), R0
            )
            found[kind] = (rule, dict(witnesses)["producer-kinds"])
        self.assertEqual(
            {
                "write": (_SL_FUSE, "write"),
                "unlink": (_SL_FUSE, "unlink"),
                "both": (_SL_FUSE, "unlink+write"),
            },
            found,
        )

    def test_producer_kinds_do_not_depend_on_outcomes(self) -> None:
        """A write and an unlink of r0
        race on two actors; whichever ran first (the write failing ENOENT when
        the unlink did), status omitting r0 while its diff lists it files one
        key."""
        found = set()
        for order in ("write-first", "unlink-first"):
            run = Run("t", f"kinds-{order}", "write and unlink of r0 race")
            run.act("a", "w", "write", path=R0, value_id="w")
            run.act("b", "u", "unlink", path=R0)
            run.act(
                "c",
                "s",
                "status",
                channel="sl",
                requires=[("w", "finished"), ("u", "finished")],
            )
            if order == "write-first":
                run.call("w", "ok", **WRITE_OK)
                run.call("u", "ok", path=R0)
            else:
                run.call("u", "ok", path=R0)
                run.call("w", "fail", **errno_fail(errno.ENOENT))
            run.call("s", "ok", **sl_status({}, diff=[R0]))
            run.finish()
            ((rule, witnesses, _attribution),) = _status_findings(
                judge_isolated(("status",), run), R0
            )
            found.add((rule, dict(witnesses)["producer-kinds"]))
        self.assertEqual({("status.status-diff-mismatch", "unlink+write")}, found)

    def test_actions_ordered_after_the_status_are_not_producers(self) -> None:
        """A status omitting a written r0 names the write alone, with or
        without a checkout that runs strictly after the status: an action
        the status finishes before cannot have produced what it reported."""
        found = {}
        for later in (None, "checkout"):
            run = Run("t", f"later-{later}", "a write of r0, status omits it")
            run.act("a", "w", "write", path=R0, value_id="w")
            run.act("a", "s", "status", channel="sl")
            if later == "checkout":
                run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
            run.call("w", "ok", **WRITE_OK)
            run.call("s", "ok", **sl_status({}, diff=[]))
            if later == "checkout":
                run.call("c", "ok", **thrift_checkout(2, conflicts=1))
            run.finish()
            ((rule, witnesses, _attribution),) = _status_findings(
                judge_isolated(("status",), run), R0
            )
            found[later] = (rule, dict(witnesses)["producer-kinds"])
        self.assertEqual(
            {None: (_SL_FUSE, "write"), "checkout": (_SL_FUSE, "write")}, found
        )

    def _rm_recreate(self, code: str, diff: list[str], *, overlap: bool) -> Run:
        """`sl rm` r0, rename r1 onto it, then sl status (racing the rename
        when `overlap`)."""
        run = Run("t", f"rm-recreate-{code}", "sl rm r0, rename r1 onto r0, sl status")
        run.act("a", "m", "sl_rm", path=R0)
        run.act(
            "a", "n", "rename", source=R1, destination=R0, requires=[("m", "finished")]
        )
        after = [("m", "finished")] if overlap else [("n", "finished")]
        run.act("b", "s", "status", channel="sl", requires=after)
        run.call("m", "ok", path=R0)
        status = sl_status({R0: code, R1: "!"}, diff=diff)
        if overlap:
            run.inv("n", "s")
            run.done("n", "ok", source=R1, destination=R0)
            run.done("s", "ok", **status)
        else:
            run.call("n", "ok", source=R1, destination=R0)
            run.call("s", "ok", **status)
        run.finish()
        return run


class TypeChangedStatusTest(unittest.TestCase):
    """A path that is a directory at some revision is judged, not skipped."""

    def _run(self, name: str, entries: Mapping[str, str]) -> Run:
        """r2 is a directory at the tip and a file at revision 2:
        check out revision 2, write r2, then a Thrift status for revision 2."""
        run = Run(
            "t",
            name,
            "status of a type-changed register while it is a file",
            type_changes=[{"path": R2, "at_revision": TIP}],
        )
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "w", "write", path=R2, value_id="w", requires=[("c", "finished")])
        run.act(
            "a",
            "s",
            "status",
            channel="thrift",
            revision=2,
            requires=[("w", "finished")],
        )
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
        run.call("w", "ok", **WRITE_OK)
        run.call("s", "ok", **thrift_status(entries, revision=2))
        run.finish()
        return run

    def test_a_path_that_is_a_directory_at_some_revision_is_judged(self) -> None:
        scope = ("status.thrift-model-stale-status", f"s:{R2}")
        legal = obligations_of(
            judge_isolated(
                ("status",), self._run("type-change-listed", {R2: "MODIFIED"})
            )
        )
        self.assertEqual(ObligationStatus.SATISFIED, legal[scope].status)
        omitted = obligations_of(
            judge_isolated(("status",), self._run("type-change-omitted", {}))
        )
        self.assertEqual(
            (ObligationStatus.VIOLATED, "product"),
            (omitted[scope].status, str(omitted[scope].attribution)),
        )


class StatusRefusalTest(unittest.TestCase):
    def _sl(self, **facts: object) -> Run:
        run = Run("t", "sl-status", "an sl status fails")
        run.act("a", "s", "status", channel="sl")
        run.call("s", "fail", **facts)
        run.finish()
        return run

    def test_another_sl_exit_is_forbidden(self) -> None:
        verdict = judge_isolated(
            ("core.input-legality", "status"),
            self._sl(
                reason="status: exit 1",
                returncode=1,
                stdout="",
                stderr="abort: something else\n",
            ),
        )
        self.assertEqual("violated", status_of(verdict, "core.input-legality", "s"))

    def test_a_killed_sl_status_is_excused_by_its_kill(self) -> None:
        run = Run("t", "killed-status", "an sl status killed -9 by a nemesis")
        run.act("a", "s", "status", channel="sl")
        run.act(
            "nemesis",
            "k",
            "kill_process",
            target_action="s",
            after_ms=100,
            requires=[("s", "started")],
        )
        run.inv("s")
        run.call("k", "ok", target_action="s", pgid=4242, group_empty=True)
        run.done(
            "s",
            "fail",
            reason="status: killed by signal 9",
            returncode=-9,
            stdout="",
            stderr="",
        )
        run.finish()
        verdict = judge_isolated(("core.input-legality", "status"), run)
        self.assertEqual("satisfied", status_of(verdict, "core.input-legality", "s"))

    def test_an_untyped_sl_failure_stays_unobserved(self) -> None:
        verdict = judge_isolated(
            ("core.input-legality", "status"), self._sl(reason="diff: failed")
        )
        item = obligations_of(verdict)[("core.input-legality", "s")]
        self.assertEqual(ObligationStatus.NOT_EVALUATED, item.status)
        self.assertEqual(checker.CORE_INPUT_STATE_UNOBSERVED, item.not_evaluated_reason)

    def _out_of_date(self, *, moved: bool) -> Run:
        run = Run("t", "out-of-date", "a Thrift status of Sapling's parent")
        if moved:
            run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.act("a", "s", "status", channel="thrift")
        if moved:
            run.call("c", "ok", **thrift_checkout(2, conflicts=0))
        run.call(
            "s",
            "fail",
            reason="thrift: out of date",
            source="thrift",
            error_type="OUT_OF_DATE_PARENT",
        )
        run.finish()
        return run

    def test_out_of_date_without_a_revision_compares_the_two_parents(self) -> None:
        """A Thrift checkout moves EdenFS's parent only, so Sapling's `.` is stale."""
        moved = judge_isolated(
            ("core.input-legality", "status"), self._out_of_date(moved=True)
        )
        self.assertEqual("satisfied", status_of(moved, "core.input-legality", "s"))
        stayed = judge_isolated(
            ("core.input-legality", "status"), self._out_of_date(moved=False)
        )
        self.assertEqual("violated", status_of(stayed, "core.input-legality", "s"))
