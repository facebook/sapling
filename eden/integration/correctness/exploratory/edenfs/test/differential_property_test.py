# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The differential property: paired primary and reference actions agree, and a
disagreement is attributed after the primary's own legality is judged. Complements
the corpus family `differential`: these tests pin diverged mutations leaving their
footprint uncompared, unresolved reference sides, value, getattr, listing, and
outcome disagreement attribution, and reads through a diverged open.
"""

from __future__ import annotations

import errno
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.edenfs.test.judging import (
    judge,
    judge_isolated,
    obligations_of,
    obligations_with_prefix,
    status_of,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import getattr_facts
from eden.integration.correctness.exploratory.test.corpus.author import (
    committed,
    errno_fail,
    R0,
    read_ok,
    Run,
    sl_status,
    TIP,
    value,
    WRITE_OK,
)


def _paired_run() -> Run:
    """A write whose reference side fails, then a read and a status."""
    run = Run("t", "diverged", "a diverged write", reference=True)
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.act("a", "s", "status", channel="sl")
    run.paired(
        "w",
        "ok",
        ref_outcome="fail",
        ref_facts=errno_fail(errno.ENOENT),
        **WRITE_OK,
    )
    written = read_ok(value("a", "w"))
    run.paired("r", "ok", ref_facts=read_ok(committed(R0, TIP)), **written)
    status = sl_status({R0: "M"}, [R0])
    run.paired("s", "ok", ref_facts=sl_status({}, []), **status)
    run.finish()
    return run


class DifferentialTest(unittest.TestCase):
    def test_a_diverged_mutation_leaves_its_footprint_uncompared(self) -> None:
        verdict = judge_isolated(("differential",), _paired_run())
        self.assertEqual(
            "violated", status_of(verdict, "differential.outcome-disagreement", "w")
        )
        self.assertIsNone(status_of(verdict, "differential.value-disagreement", "r"))
        self.assertIsNone(status_of(verdict, "differential.checkout-disagreement", "s"))
        self.assertEqual("harness-suspect", str(verdict.kind))

    def test_an_unresolved_reference_side_is_a_reference_finding(self) -> None:
        """A reference write that never completed left the run a
        silent clean differential; it is the reference's finding."""
        run = Run("t", "diff-ref-info", "the reference write timed out", reference=True)
        run.act("a", "w", "write", path=R0, value_id="w")
        run.act("a", "r", "read", path=R0, channel="fuse", requires=[("w", "finished")])
        run.paired(
            "w",
            "ok",
            ref_outcome="info",
            ref_facts={"reason": "reference timed out"},
            **WRITE_OK,
        )
        run.paired(
            "r", "ok", ref_facts=read_ok(committed(R0, TIP)), **read_ok(value("a", "w"))
        )
        run.finish()
        verdict = judge_isolated(("differential",), run)
        item = obligations_of(verdict)[("differential.outcome-disagreement", "w")]
        self.assertEqual(
            ("violated", "reference"), (str(item.status), str(item.attribution))
        )
        self.assertIsNone(status_of(verdict, "differential.value-disagreement", "r"))
        self.assertEqual("harness-suspect", str(verdict.kind))

    def test_a_malformed_checkout_pair_is_a_reference_finding(self) -> None:
        reference = sl_status({})
        del reference["diff_paths"]
        run = Run("t", "diff-status", "a paired sl status", reference=True)
        run.act("a", "s", "status", channel="sl")
        run.paired("s", "ok", ref_facts=reference, **sl_status({}))
        run.finish()
        item = obligations_of(judge(run))[("differential.checkout-disagreement", "s")]
        self.assertEqual(
            ("violated", "reference"), (str(item.status), str(item.attribution))
        )

    def test_a_run_without_a_reference_side_has_no_obligation(self) -> None:
        run = Run("t", "plain", "no reference clone")
        run.act("a", "w", "write", path=R0, value_id="w")
        run.call("w", "ok", **WRITE_OK)
        run.finish()
        self.assertEqual(
            set(),
            obligations_with_prefix(
                judge_isolated(("differential",), run), "differential."
            ),
        )


def _ordered_read(primary: bytes, reference: bytes, *, held: bool = False) -> Run:
    """A write, then a read of r0 (through a descriptor opened first when held)."""
    run = Run("t", "ordered-read", "an ordered paired read", reference=True)
    if held:
        run.act("a", "o", "open", path=R0)
    run.act("a", "w", "write", path=R0, value_id="w")
    if held:
        run.act("a", "q", "read_fd", fd="o")
        run.act("a", "e", "close", fd="o")
        run.paired("o", "ok", fd=3)
    else:
        run.act("a", "q", "read", path=R0, channel="fuse")
    run.paired("w", "ok", **WRITE_OK)
    run.paired("q", "ok", ref_facts=read_ok(reference), **read_ok(primary))
    if held:
        run.paired("e", "ok", fd=3)
    run.finish()
    return run


def _value_item(verdict: checker.Verdict) -> Any:
    assert verdict.report is not None
    (item,) = [
        item
        for result in verdict.report.results
        for item in result.obligations
        if (item.rule_id, item.scope) == ("differential.value-disagreement", "q")
    ]
    return item


class ValueDisagreementAttributionTest(unittest.TestCase):
    """A disagreement is attributed after the EdenFS side's legality."""

    def test_a_legal_eden_value_makes_the_disagreement_the_references(self) -> None:
        """EdenFS reads w, the reference stale or garbage bytes."""
        for held in (False, True):
            for reference in (committed(R0, TIP), b"\0" * 64):
                with self.subTest(held=held, reference=reference[:4]):
                    verdict = judge(
                        _ordered_read(value("a", "w"), reference, held=held)
                    )
                    item = _value_item(verdict)
                    self.assertEqual("violated", str(item.status))
                    self.assertEqual("reference", str(item.attribution))
                    self.assertEqual("harness-suspect", str(verdict.kind))

    def test_an_illegal_eden_value_makes_the_disagreement_the_products(self) -> None:
        """EdenFS reads the pre-write bytes after the acked write; the engine proves it."""
        for held in (False, True):
            with self.subTest(held=held):
                verdict = judge(
                    _ordered_read(committed(R0, TIP), value("a", "w"), held=held)
                )
                item = _value_item(verdict)
                self.assertEqual("violated", str(item.status))
                self.assertEqual("product", str(item.attribution))
                self.assertIsNotNone(item.proof)
                self.assertEqual("reject", str(verdict.kind))

    def test_torn_eden_bytes_make_the_disagreement_the_products(self) -> None:
        verdict = judge_isolated(
            ("differential",), _ordered_read(b"\0" * 64, value("a", "w"))
        )
        item = _value_item(verdict)
        self.assertEqual("product", str(item.attribution))
        self.assertIsNone(item.proof)

    def test_a_getattr_disagreement_is_attributed_after_eden_legality(self) -> None:
        """Untouched r0 is 64 bytes; the side reporting 32 is wrong."""
        for eden, reference, attribution in (
            (64, 32, "reference"),
            (32, 64, "product"),
        ):
            with self.subTest(eden=eden):
                run = Run("t", "diff-getattr", "a differential getattr", reference=True)
                run.act("a", "g", "getattr", path=R0)
                run.paired(
                    "g",
                    "ok",
                    ref_facts=getattr_facts(size=reference),
                    **getattr_facts(size=eden),
                )
                run.finish()
                item = obligations_of(judge_isolated(("differential",), run))[
                    ("differential.value-disagreement", "g")
                ]
                self.assertEqual(
                    ("violated", attribution), (str(item.status), str(item.attribution))
                )

    def test_a_listing_disagreement_is_attributed_after_eden_legality(self) -> None:
        """An EdenFS listing with an unknown name and missing registers, or
        a repeated name, is the product's; a complete one makes the reference
        wrong."""
        names = ["r0.reg", "r1.reg", "r2.reg"]
        for eden, reference, attribution in (
            (["r2.reg", "bogus-name"], names, "product"),
            (names + ["r0.reg"], names, "product"),
            (names, ["r0.reg"], "reference"),
        ):
            with self.subTest(eden=eden):
                run = Run("t", "diff-readdir", "a differential listing", reference=True)
                run.act("a", "l", "readdir", path="tree/registers", channel="fuse")
                run.paired(
                    "l",
                    "ok",
                    ref_facts={"channel": "fuse", "entries": reference},
                    channel="fuse",
                    entries=eden,
                )
                run.finish()
                item = obligations_of(judge_isolated(("differential",), run))[
                    ("differential.value-disagreement", "l")
                ]
                self.assertEqual(
                    ("violated", attribution), (str(item.status), str(item.attribution))
                )

    def test_reads_and_closes_through_a_diverged_open_are_uncompared(self) -> None:
        run = Run("t", "diverged-open", "the reference open fails", reference=True)
        run.act("a", "o", "open", path=R0)
        run.act("a", "d", "read_fd", fd="o")
        run.act("a", "e", "close", fd="o")
        run.paired(
            "o", "ok", ref_outcome="fail", ref_facts=errno_fail(errno.ENOENT), fd=3
        )
        run.paired("d", "ok", **read_ok(committed(R0, TIP)))
        run.paired("e", "ok", fd=3)
        run.finish()
        verdict = judge_isolated(("differential",), run)
        self.assertEqual(
            "violated", status_of(verdict, "differential.outcome-disagreement", "o")
        )
        self.assertEqual(
            {("differential.outcome-disagreement", "o")},
            obligations_with_prefix(verdict, "differential."),
        )


def _reference_fails_read(primary: bytes, *, unlink: bool) -> Run:
    """An optional unlink of r0, then a read EdenFS serves with `primary` and the reference fails ENOENT."""
    run = Run("t", "reference-fails-read", "the reference read fails", reference=True)
    if unlink:
        run.act("a", "u", "unlink", path=R0)
    run.act("a", "q", "read", path=R0, channel="fuse")
    if unlink:
        run.paired("u", "ok", path=R0)
    run.paired(
        "q",
        "ok",
        ref_outcome="fail",
        ref_facts=errno_fail(errno.ENOENT),
        **read_ok(primary),
    )
    run.finish()
    return run


class OutcomeDisagreementAttributionTest(unittest.TestCase):
    """A reference failure is the reference's only once EdenFS's outcome is legal."""

    def _item(self, verdict: checker.Verdict) -> Any:
        assert verdict.report is not None
        (item,) = [
            item
            for result in verdict.report.results
            for item in result.obligations
            if (item.rule_id, item.scope) == ("differential.outcome-disagreement", "q")
        ]
        return item

    def test_an_illegal_eden_read_makes_the_disagreement_the_products(self) -> None:
        """EdenFS serves r0 after the unlink; the reference rightly fails ENOENT."""
        verdict = judge_isolated(
            ("differential",), _reference_fails_read(committed(R0, TIP), unlink=True)
        )
        item = self._item(verdict)
        self.assertEqual("violated", str(item.status))
        self.assertEqual("product", str(item.attribution))
        self.assertEqual("reject", str(verdict.kind))

    def test_a_legal_eden_read_makes_the_disagreement_the_references(self) -> None:
        verdict = judge_isolated(
            ("differential",), _reference_fails_read(committed(R0, TIP), unlink=False)
        )
        item = self._item(verdict)
        self.assertEqual("violated", str(item.status))
        self.assertEqual("reference", str(item.attribution))
        self.assertEqual("harness-suspect", str(verdict.kind))

    def test_torn_eden_bytes_make_the_disagreement_the_products(self) -> None:
        verdict = judge_isolated(
            ("differential",), _reference_fails_read(b"\0" * 64, unlink=False)
        )
        item = self._item(verdict)
        self.assertEqual("product", str(item.attribution))
