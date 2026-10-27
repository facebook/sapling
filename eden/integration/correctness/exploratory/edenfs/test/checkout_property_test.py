# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The checkout property's interruption and activity rules: when an interrupted checkout
ends, statuses served while it lasts, and which checkout-only runs exercise the
target. Complements the interrupted and sl goto cases of the corpus family
`checkout`.
"""

from __future__ import annotations

import unittest

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    IN_PROGRESS_RULE,
)
from eden.integration.correctness.exploratory.edenfs.target import EDENFS_TARGET
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    status_of,
)
from eden.integration.correctness.exploratory.test.corpus import loader
from eden.integration.correctness.exploratory.test.corpus.author import (
    Run,
    sl_status,
    thrift_checkout,
    thrift_status,
)


class InterruptionTest(unittest.TestCase):
    def _interrupted(self, name: str) -> Run:
        """c1 (Thrift to rev2) fails on an injected checkout-begin error."""
        run = Run("t", name, "c1 is interrupted")
        run.act(
            "nemesis",
            "f",
            "fault_install",
            fault_id="F",
            site="checkout-begin",
            target_action="c1",
            behavior="error",
        )
        run.act(
            "a",
            "c1",
            "checkout",
            revision=2,
            via="thrift",
            mode="normal",
            requires=[("f", "finished")],
        )
        run.act(
            "nemesis", "x", "fault_remove", fault_id="F", requires=[("c1", "finished")]
        )
        run.call("f", "ok", fault_id="F")
        run.call(
            "c1",
            "fail",
            reason="checkout: EdenError: exploratory fault F",
            source="thrift",
            error_type="GENERIC_ERROR",
        )
        run.call("x", "ok", fault_id="F", removed=1)
        run.act("a", "c2", "checkout", revision=2, via="thrift", mode="normal")
        run.act("b", "s", "status", channel="thrift", revision=2)
        return run

    def test_a_resume_the_daemon_may_finish_ends_the_interruption(self) -> None:
        """A status served once a same-target checkout was invoked is legal."""
        shapes = {
            "ok-acked-after-status": (
                "ok",
                thrift_checkout(2, conflicts=0),
            ),
            "client-timeout": (
                "fail",
                {"reason": "TTransportException: TIMED_OUT", "source": "thrift"},
            ),
            "info": ("info", {}),
        }
        for name, (outcome, facts) in shapes.items():
            with self.subTest(shape=name):
                run = self._interrupted(name)
                run.inv("c2")
                run.call("s", "ok", **thrift_status({}, 2))
                run.done("c2", outcome, **facts)
                run.finish()
                verdict = judge_isolated(("checkout",), run)
                self.assertEqual(
                    "satisfied",
                    status_of(verdict, "checkout.interrupted-refusal", "s"),
                )
                self.assertNotEqual(checker.REJECT, verdict.kind)


_INTERRUPTED_ADVICE = (
    "a previous checkout was interrupted - please run `sl go 2` to resume it."
)


def _thrift_interrupted(name: str) -> Run:
    """c (Thrift to rev2) fails mid-walk with an EdenError."""
    run = Run("t", name, "c is interrupted")
    run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
    run.call(
        "c", "fail", reason="thrift: fault", source="thrift", error_type="GENERIC_ERROR"
    )
    return run


class InterruptedCellTest(unittest.TestCase):
    """The interruption is the `interrupted` cell: set by a failure, ended by a resume."""

    def _refused(self, target: int, via: str, text: str) -> checker.Verdict:
        run = _thrift_interrupted("refused")
        run.act("a", "d", "checkout", revision=target, via=via, mode="normal")
        if via == "thrift":
            run.call(
                "d",
                "fail",
                reason=f"thrift: {text}",
                source="thrift",
                error_type="CHECKOUT_IN_PROGRESS",
            )
        else:
            run.call(
                "d",
                "fail",
                reason="goto: exit 255",
                returncode=255,
                stdout="",
                stderr=f"abort: EdenError: {text}\n",
            )
        run.finish()
        return judge(run)

    def test_a_refusal_is_judged_by_its_wording(self) -> None:
        """EdenFS words a checkout refusal by its state (EdenMount.cpp:1757-1783)."""
        for target, via, text, expected in (
            (1, "goto", _INTERRUPTED_ADVICE, "satisfied"),
            (1, "thrift", _INTERRUPTED_ADVICE, "satisfied"),
            (2, "goto", _INTERRUPTED_ADVICE, "violated"),
            (2, "thrift", _INTERRUPTED_ADVICE, "violated"),
            (
                1,
                "goto",
                "another checkout operation is still in progress",
                "violated",
            ),
        ):
            with self.subTest(target=target, via=via, text=text):
                verdict = self._refused(target, via, text)
                self.assertEqual(expected, status_of(verdict, IN_PROGRESS_RULE, "d"))

    def test_a_status_refused_as_interrupted_needs_an_interruption(self) -> None:
        stderr = f"abort: EdenError: {_INTERRUPTED_ADVICE}\n"
        refused = {
            "reason": "status: exit 255",
            "returncode": 255,
            "stdout": "",
            "stderr": stderr,
        }
        for interrupted, expected in ((True, "satisfied"), (False, "violated")):
            with self.subTest(interrupted=interrupted):
                if interrupted:
                    run = _thrift_interrupted("status")
                else:
                    run = Run("t", "status", "an acked checkout")
                    run.act(
                        "a", "c", "checkout", revision=2, via="thrift", mode="normal"
                    )
                    run.call("c", "ok", **thrift_checkout(2, conflicts=0))
                run.act("a", "s", "status", channel="sl")
                run.call("s", "fail", **refused)
                run.finish()
                verdict = judge_isolated(("checkout", "core.input-legality"), run)
                self.assertEqual(
                    expected, status_of(verdict, "core.input-legality", "s")
                )


class ActivityTest(unittest.TestCase):
    def test_a_checkout_only_run_exercises_the_target(self) -> None:
        run = Run("t", "checkout-only", "one Thrift checkout")
        run.act("a", "c", "checkout", revision=2, via="thrift", mode="normal")
        run.call("c", "ok", **thrift_checkout(2, conflicts=0))
        run.finish()
        verdict = judge_isolated(("checkout", "core.activity"), run)
        self.assertEqual("satisfied", status_of(verdict, "core.activity", "run"))
        self.assertEqual(checker.ACCEPT, verdict.kind)

    def test_an_sl_goto_is_judged_by_its_shape_rules(self) -> None:
        run = Run("t", "sl", "an sl goto to rev2 then a status")
        run.act("a", "c", "checkout", revision=2, via="goto", mode="normal")
        run.act("a", "s", "status", channel="sl")
        run.call("c", "ok", revision=2, node="0" * 40, via="goto")
        run.call("s", "ok", **sl_status({}))
        run.finish()
        verdict = judge_isolated(("checkout",), run)
        # Settled (no earlier move, nothing overlaps it): no order to judge.
        self.assertIsNone(status_of(verdict, "checkout.parent-before-paths", "c"))
        self.assertEqual("satisfied", status_of(verdict, "checkout.sl-abort", "c"))


class AbortIdentityTest(unittest.TestCase):
    """An illegal abort's key names the contradicted input."""

    def _abort(self, case: str) -> dict[str, tuple[str, dict[str, str]]]:
        found = loader.case(case)
        verdict = checker.check(
            EDENFS_TARGET,
            found.spec(EDENFS_TARGET),
            found.events(),
            execution={"status": "completed"},
        )
        assert verdict.report is not None
        return {
            item.scope: (
                str(item.status),
                {w.role: w.value for w in item.witnesses},
            )
            for result in verdict.report.results
            for item in result.obligations
            if item.rule_id == "checkout.sl-abort"
        }

    def test_a_listed_clean_path_has_one_identity_whatever_else_is_listed(
        self,
    ) -> None:
        not_blocking = {"outcome": "aborted", "blockers": "not-blocking"}
        self.assertEqual(
            {"c:listed": ("satisfied", not_blocking)},
            self._abort("checkout/sl-goto-missing-removed-aborts"),
        )
        for case in (
            "checkout/sl-goto-abort-lists-clean-path",
            "checkout/sl-goto-abort-lists-clean-path-beside-blocker",
        ):
            with self.subTest(case=case):
                self.assertEqual(
                    {"c:listed": ("violated", not_blocking)}, self._abort(case)
                )

    def test_a_successful_update_over_a_blocking_state_is_unlisted(self) -> None:
        found = self._abort("checkout/sl-goto-dirty-changed-succeeds")
        self.assertEqual(
            {"c": ("violated", {"outcome": "succeeded", "blockers": "unlisted"})},
            found,
        )
