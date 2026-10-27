# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Shared document schemas: strict JSON, exact versions and key sets, text bounds, run
admissions, and verdict files read through the checker's decoder with an exact
oversize threshold.
"""

from __future__ import annotations

import json
import tempfile
import unittest
from dataclasses import replace
from pathlib import Path
from unittest import mock

from eden.integration.correctness.exploratory import schemas
from eden.integration.correctness.exploratory.core.checker import encode_verdict
from eden.integration.correctness.exploratory.core.jsonio import MAX_SCALAR_BYTES
from eden.integration.correctness.exploratory.core.property import MAX_RUN_OBLIGATIONS
from eden.integration.correctness.exploratory.schemas import (
    admission_document,
    admission_for,
    BOUNDS,
    canonical_json,
    load_admission,
    load_verdict_file,
    read_admission,
    require_document,
    require_text,
    RunPurpose,
    SchemaError,
    sha256_ref,
    strict_loads,
    VERDICT_OVERSIZE,
    write_admission,
    write_document,
)
from eden.integration.correctness.exploratory.test.tasking_fixtures import (
    judge,
    Ob,
    obligations,
    script,
    WIDE,
)

_WIDE = WIDE
_SMALL = script(Ob("bytes.torn-read", identity={"channel": "fuse"}, status="satisfied"))


def _max_obligation(index: int) -> Ob:
    """A violation whose reason and evidence are at their byte bound, escaped widest."""
    return Ob(
        "bytes.torn-read",
        f"path:{index:08d}/" + "p" * (MAX_SCALAR_BYTES - 32),
        identity={"channel": "fuse"},
        evidence={"read-action": "x"},
        scope=f"s{index}",
        reason="x",
        wide=MAX_SCALAR_BYTES // 4,
    )


class StrictJsonTest(unittest.TestCase):
    def test_duplicate_keys_and_non_finite_numbers_are_rejected(self) -> None:
        for text in (
            '{"a": 1, "a": 2}',
            '{"a": NaN}',
            '{"a": Infinity}',
            '{"a": 1e999}',
        ):
            with self.subTest(text=text):
                with self.assertRaises(SchemaError):
                    strict_loads(text)
        self.assertEqual({"a": [1.5]}, strict_loads('{"a": [1.5]}'))

    def test_canonical_json_is_order_independent(self) -> None:
        self.assertEqual(
            canonical_json({"b": 1, "a": [2]}), canonical_json({"a": [2], "b": 1})
        )
        self.assertEqual(b'{"a":"\xe2\x89\xa0"}', canonical_json({"a": "≠"}))
        self.assertTrue(sha256_ref(b"").startswith("sha256:e3b0c442"))


class DocumentTest(unittest.TestCase):
    def test_a_failed_write_leaves_the_previous_document_whole(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "doc.json"
            write_document(path, {"a": 1})
            with (
                mock.patch.object(schemas.os, "replace", side_effect=OSError("disk")),
                self.assertRaises(OSError),
            ):
                write_document(path, {"a": 2})
            self.assertEqual({"a": 1}, json.loads(path.read_text()))
            write_document(path, {"a": 3})
            self.assertEqual({"a": 3}, json.loads(path.read_text()))

    def test_exact_version_rules(self) -> None:
        for version in (True, False, 0, 2, "1", 1.0, None):
            with self.subTest(version=version):
                with self.assertRaises(SchemaError) as raised:
                    require_document(
                        {"schema_version": version, "a": 1},
                        version=1,
                        required=("a",),
                        what="doc",
                        decision="d",
                    )
                self.assertEqual("d-version", raised.exception.decision)
        require_document(
            {"schema_version": 1, "a": 1},
            version=1,
            required=("a",),
            what="doc",
            decision="d",
        )

    def test_exact_key_set_with_optional_diagnostics(self) -> None:
        with self.assertRaises(SchemaError):
            require_document(
                {"schema_version": 1},
                version=1,
                required=("a",),
                what="doc",
                decision="d",
            )
        with self.assertRaises(SchemaError):
            require_document(
                {"schema_version": 1, "a": 1, "b": 2},
                version=1,
                required=("a",),
                what="doc",
                decision="d",
            )
        require_document(
            {"schema_version": 1, "a": 1, "diagnostics": {"x": 1}},
            version=1,
            required=("a",),
            what="doc",
            decision="d",
        )
        with self.assertRaises(SchemaError):
            require_document(
                {"schema_version": 1, "a": 1, "diagnostics": []},
                version=1,
                required=("a",),
                what="doc",
                decision="d",
            )

    def test_text_bound_at_limit_and_limit_plus_one(self) -> None:
        self.assertEqual("x" * 4096, require_text("x" * 4096, "t"))
        with self.assertRaises(SchemaError):
            require_text("x" * 4097, "t")
        self.assertEqual(_WIDE * 1024, require_text(_WIDE * 1024, "t"))
        with self.assertRaises(SchemaError):
            require_text(_WIDE * 1024 + "x", "t")

    def test_bounds_match_their_owners(self) -> None:
        self.assertEqual(2 * 1024 * 1024, BOUNDS.verdict_operational_bytes)
        self.assertEqual(MAX_SCALAR_BYTES, BOUNDS.text_bytes)


class AdmissionTest(unittest.TestCase):
    def test_round_trip(self) -> None:
        for purpose in RunPurpose:
            with self.subTest(purpose=purpose):
                admission = admission_for(purpose, "c")
                self.assertEqual(
                    admission, read_admission(admission_document(admission))
                )

    def test_reader_rejects_unknown_values_and_keys(self) -> None:
        document = admission_document(admission_for(RunPurpose.MANUAL, None))
        bad = [
            {**document, "run_purpose": "nightly"},
            {**document, "schema_version": True},
            {**document, "extra": 1},
            {**document, "environment_degraded": 1},
        ]
        for raw in bad:
            with self.assertRaises(SchemaError):
                read_admission(raw)

    def test_missing_and_malformed_files_are_named(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            run = Path(temporary)
            with self.assertRaises(SchemaError) as raised:
                load_admission(run)
            self.assertEqual("purpose:missing-admission", raised.exception.decision)
            (run / "admission.json").write_text(
                '{"schema_version": 1, "schema_version": 1}'
            )
            with self.assertRaises(SchemaError) as raised:
                load_admission(run)
            self.assertEqual("purpose:invalid-admission", raised.exception.decision)
            write_admission(run, admission_for(RunPurpose.EXPLORE, "c"))
            self.assertEqual(RunPurpose.EXPLORE, load_admission(run).run_purpose)


class VerdictFileTest(unittest.TestCase):
    def test_small_verdict_reads_through_the_checker_decoder(self) -> None:
        verdict = judge(_SMALL)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "verdict.json"
            path.write_bytes(encode_verdict(verdict))
            loaded = load_verdict_file(path)
        self.assertFalse(loaded.oversize)
        self.assertEqual(verdict.kind, loaded.verdict.kind)

    def test_missing_and_non_canonical_verdicts(self) -> None:
        verdict = judge(_SMALL)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "verdict.json"
            with self.assertRaises(SchemaError) as raised:
                load_verdict_file(path)
            self.assertEqual("execution.missing-verdict", raised.exception.decision)
            path.write_text(json.dumps(json.loads(encode_verdict(verdict))))
            with self.assertRaises(SchemaError) as raised:
                load_verdict_file(path)
            self.assertEqual("execution.verdict-unreadable", raised.exception.decision)

    def test_read_ceiling(self) -> None:
        verdict = judge(_SMALL)
        data = encode_verdict(verdict)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "verdict.json"
            path.write_bytes(data)
            ceiling = replace(BOUNDS, verdict_read_ceiling_bytes=len(data) - 1)
            with self.assertRaises(SchemaError) as raised:
                load_verdict_file(path, ceiling)
            self.assertEqual("execution.verdict-unreadable", raised.exception.decision)

    def test_maximum_shape_is_oversize_kept_whole_and_classified(self) -> None:
        """Maximum-width scalars escape to 3x their UTF-8 bytes; the whole verdict survives."""
        verdict = judge(script(*(_max_obligation(index) for index in range(128))))
        data = encode_verdict(verdict)
        per_obligation = (len(data) - len(encode_verdict(judge(_SMALL)))) // 128
        self.assertGreater(per_obligation, 2 * 12288)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "verdict.json"
            path.write_bytes(data)
            loaded = load_verdict_file(path)
            self.assertEqual(data, path.read_bytes())
        self.assertTrue(loaded.oversize)
        self.assertEqual(len(data), loaded.encoded_bytes)
        violated = [o for o in obligations(loaded.verdict) if o.status == "violated"]
        self.assertEqual(128, len(violated))
        self.assertEqual("execution.verdict-oversize", VERDICT_OVERSIZE)
        # A verdict at the run-wide obligation bound would need this many bytes.
        self.assertGreater(
            per_obligation * MAX_RUN_OBLIGATIONS, BOUNDS.verdict_operational_bytes
        )

    def test_threshold_is_exact(self) -> None:
        verdict = judge(_SMALL)
        data = encode_verdict(verdict)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "verdict.json"
            path.write_bytes(data)
            at = load_verdict_file(
                path, replace(BOUNDS, verdict_operational_bytes=len(data))
            )
            over = load_verdict_file(
                path, replace(BOUNDS, verdict_operational_bytes=len(data) - 1)
            )
        self.assertEqual((False, True), (at.oversize, over.oversize))
