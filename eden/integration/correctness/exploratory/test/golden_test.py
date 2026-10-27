# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Golden findings reports for the reviewed task-key cases.

Every scenario is a real offline campaign: synthetic EdenFS histories
judged by `check(EDENFS_TARGET, ...)` with real bindings (no finding is
substituted into a verdict); the report scenario adds the four red
controls' live captures re-judged for its campaign. The findings pass
writes the report with every subprocess refused, and the normalized
findings, output lines, finding body, and replay command must equal the
checked-in goldens. They pin the report's output contract: which runs
form one key, which keys are related, what routes away from a key, and
how a person reproduces a finding. Regenerate by naming the source
directory explicitly, never the test's extracted resources:
`buck2 test ...:golden_test -- --env EDEN_EXPLORATORY_UPDATE_GOLDEN=$PWD/fbcode/eden/integration/correctness/exploratory/testdata/golden`,
then review the diff.
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from collections.abc import Mapping
from pathlib import Path
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory import findings
from eden.integration.correctness.exploratory.core.identity import error_set_document
from eden.integration.correctness.exploratory.schemas import load_verdict_file
from eden.integration.correctness.exploratory.test.campaign_fixtures import (
    Campaign,
    control_runs,
    new_run,
    R0,
    R1,
    scripted,
    Shape,
    shape_of,
    spliced,
    TARGET,
    torn_shape,
    written,
)
from eden.integration.correctness.exploratory.test.corpus.author import (
    read_ok,
    sl_status,
    WRITE_OK,
)

GOLDEN = Path(__file__).parent.with_name("testdata") / "golden"
UPDATE_VARIABLE = "EDEN_EXPLORATORY_UPDATE_GOLDEN"
SOURCE = Path("fbcode/eden/integration/correctness/exploratory/testdata/golden")


def _checkout_root(path: Path) -> Path | None:
    """Return the nearest directory at or above `path` that holds a `.hg`, or None."""
    return next((p for p in (path, *path.parents) if (p / ".hg").is_dir()), None)


def update_directory(value: str | None) -> Path | None:
    """Return the source golden directory to rewrite, or None to compare.

    It must be an absolute, existing, non-symlink directory at exactly
    `SOURCE` under its checkout root, so regeneration never writes into a
    copy of the package (a par extraction or a link tree) wherever the
    build puts one.
    """
    if not value:
        return None
    path = Path(value)
    root = _checkout_root(path) if path.is_absolute() else None
    if (
        root is None
        or not path.is_dir()
        or path.is_symlink()
        or path.relative_to(root) != SOURCE
    ):
        raise ValueError(
            f"{UPDATE_VARIABLE} must name the source golden directory, got {value!r}"
        )
    return path


UPDATE = update_directory(os.environ.get(UPDATE_VARIABLE))
REGENERATE = (
    "regenerate: buck2 test @fbcode//mode/opt"
    " fbcode//eden/integration/correctness/exploratory:golden_test --"
    f" --env {UPDATE_VARIABLE}="
    "$PWD/fbcode/eden/integration/correctness/exploratory/testdata/golden"
)
TORN_FUSE = error_set_document(
    [("bytes.torn-read", "torn-read", (("channel", "fuse"),))]
)


def _refuse(*_args: object, **_kwargs: object) -> None:
    raise AssertionError("the findings report must not spawn or contact a store")


def four_correlated() -> Shape:
    """One run whose facts violate four properties on r0.

    After an acked write `w`: a FUSE read returns a splice of `w` and rev3
    (torn), a Thrift read returns `w` (the channels disagree), a complete
    journal window omits r0, and sl diff lists r0 while sl status omits it.
    """
    return scripted(
        "four-correlated",
        "Torn FUSE bytes, a disagreeing Thrift read, a journal omission, and a "
        "status/diff mismatch on r0.",
        writes=[("w", R0)],
        reads=[("r", R0, "fuse", spliced("w")), ("t", R0, "thrift", written("w"))],
        journal=True,
        status=({}, [R0]),
    )


def two_subjects() -> Shape:
    """A torn FUSE read of r0 and a status/diff mismatch on r1 in one run."""
    return scripted(
        "two-subjects",
        "A torn read of r0; sl status omits r1 while sl diff lists it.",
        writes=[("w0", R0), ("w1", R1)],
        reads=[("r", R0, "fuse", spliced("w0", R0))],
        status=({R0: "M"}, [R0, R1]),
    )


def subset_superset() -> Shape:
    """Torn reads of r0 and r1, plus a status/diff mismatch on r1 only."""
    return scripted(
        "subset-superset",
        "Torn reads of r0 and r1; sl status omits r1 while sl diff lists it.",
        writes=[("w0", R0), ("w1", R1)],
        reads=[
            ("r0", R0, "fuse", spliced("w0", R0)),
            ("r1", R1, "fuse", spliced("w1", R1)),
        ],
        status=({R0: "M"}, [R0, R1]),
    )


def torn_with_harness_finding() -> Shape:
    """A torn FUSE read of r0, then an sl status recorded without its diff.

    The status's missing diff is a recorder shape finding
    (`result-shape.channel-fields`, `harness`, on the checkout subject);
    the status itself, r0 `M`, holds.
    """
    run = new_run(
        "torn-with-harness-finding",
        "A torn FUSE read of r0, then an sl status recorded without diff paths.",
    )
    run.act("a", "w", "write", path=R0, value_id="w")
    run.act("a", "r", "read", path=R0, channel="fuse")
    run.act("a", "s", "status", channel="sl")
    run.call("w", "ok", **WRITE_OK)
    run.call("r", "ok", **read_ok(spliced("w")))
    result = dict(sl_status({R0: "M"}, [R0]))
    del result["diff_paths"]
    run.call("s", "ok", **result)
    return shape_of(run)


def report(campaign: Campaign, controls: list[Path] | None = None) -> dict[str, Any]:
    """Write the campaign's findings report with every subprocess refused."""
    out = campaign.root / "findings"
    with (
        mock.patch.object(subprocess, "run", _refuse),
        mock.patch.object(subprocess, "Popen", _refuse),
    ):
        lines = findings.write_findings(
            campaign.report, campaign.artifacts, out, TARGET, controls
        )
    return {
        "findings": findings.read_findings(out / findings.FINDINGS_NAME),
        "lines": lines,
        "out": out,
    }


def _summary(result: Mapping[str, Any], root: Path) -> dict[str, Any]:
    document = result["findings"]
    return {
        "findings": [
            {
                "task_key": entry["task_key"],
                "errors": entry["errors"],
                "related": entry["related"],
                "runs": [
                    {
                        "seed": run["seed"],
                        "subjects": run["subjects"],
                        "verdict": run["verdict"],
                        "violated": sorted(
                            [o["rule_id"], o["attribution"], o["subject"]]
                            for o in run["obligations"]
                        ),
                    }
                    for run in entry["runs"]
                ],
            }
            for entry in document["findings"]
        ],
        "routed": document["routed"],
        "execution": [[e["seed"], e["execution_id"]] for e in document["execution"]],
        "unclassified": document["unclassified"],
        "controls": document["controls"],
        "lines": [line.replace(str(root), "<root>") for line in result["lines"]],
    }


class UpdateDirectoryTest(unittest.TestCase):
    def test_regeneration_targets_only_an_explicit_source_directory(self) -> None:
        self.assertIsNone(update_directory(None))
        self.assertIsNone(update_directory(""))
        with tempfile.TemporaryDirectory(prefix="n12g-") as tmp:
            Path(tmp, ".hg").mkdir()
            source = Path(tmp, SOURCE)
            source.mkdir(parents=True)
            self.assertEqual(source, update_directory(str(source)))
            # A copy of the package nested under the checkout, as a build makes.
            copy = Path(tmp, "out", SOURCE)
            copy.mkdir(parents=True)
            link = Path(tmp, "link", SOURCE)
            link.parent.mkdir(parents=True)
            link.symlink_to(source)
            with tempfile.TemporaryDirectory() as outside:
                elsewhere = Path(outside, SOURCE)
                elsewhere.mkdir(parents=True)
                for candidate in (
                    "1",
                    "testdata/golden",
                    str(copy),
                    str(link),
                    str(Path(tmp)),
                    str(elsewhere),
                ):
                    with self.subTest(value=candidate), self.assertRaises(ValueError):
                        update_directory(candidate)
                with self.subTest(value=candidate), self.assertRaises(ValueError):
                    update_directory(candidate)


class GoldenTest(unittest.TestCase):
    maxDiff = None

    @classmethod
    def setUpClass(cls) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="r4-golden-")
        cls.addClassCleanup(temporary.cleanup)
        cls.root = Path(temporary.name)

    def _campaign(self, name: str) -> Campaign:
        return Campaign(self.root / name / "campaign", f"golden-{name}")

    def _check(self, name: str, campaign: Campaign, **kwargs: Any) -> dict[str, Any]:
        result = report(campaign, **kwargs)
        summary = _summary(result, campaign.root)
        path = GOLDEN / f"{name}.json"
        if UPDATE is not None:
            written = UPDATE / f"{name}.json"
            written.write_text(
                json.dumps(summary, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
            )
            self.assertEqual(summary, json.loads(written.read_text()))
        else:
            self.assertEqual(json.loads(path.read_text()), summary, REGENERATE)
        return {**summary, "out": result["out"], "entries": result["findings"]}

    def test_finding_body_and_code_owned_replay_command(self) -> None:
        campaign = self._campaign("report")
        campaign.add_run(7, torn_shape())
        controls = control_runs(self.root / "report", campaign.campaign_id)
        summary = self._check("report", campaign, controls=controls)
        (entry,) = summary["entries"]["findings"]
        self.assertEqual(TORN_FUSE["errors"], entry["errors"])
        self.assertTrue(summary["controls"])
        self.assertTrue(
            all(r == "pass" for c in summary["controls"].values() for r in c.values()),
            summary["controls"],
        )
        body = (summary["out"] / entry["body"]).read_text()
        body = body.replace(str(campaign.root), "<root>")
        command = findings.replay_command(TARGET.name, "<root>/artifacts/seed-7")
        self.assertIn(command, body)
        self.assertIn(entry["task_key"], body)
        self.assertIn(entry["runs"][0]["runspec_digest"], body)
        self.assertIn("`channel=fuse`", body)
        if UPDATE is not None:
            written = UPDATE / "finding-body.md"
            written.write_text(body)
            self.assertEqual(body, written.read_text())
        else:
            self.assertEqual((GOLDEN / "finding-body.md").read_text(), body, REGENERATE)

    def test_four_correlated_properties_form_one_sorted_key(self) -> None:
        campaign = self._campaign("four-correlated")
        campaign.add_run(1, four_correlated())
        (entry,) = self._check("four-correlated", campaign)["findings"]
        self.assertEqual(
            [
                "bytes.torn-read",
                "cross-channel.byte-disagreement",
                "journal.missing-acked-write",
                "status.status-diff-mismatch",
            ],
            [error[0] for error in entry["errors"]],
        )

    def test_two_subjects_are_two_keys(self) -> None:
        campaign = self._campaign("two-subjects")
        campaign.add_run(1, two_subjects())
        summary = self._check("two-subjects", campaign)
        self.assertEqual(
            [[f"path:{R0}"], [f"path:{R1}"]],
            sorted(e["runs"][0]["subjects"] for e in summary["findings"]),
        )

    def test_same_semantic_set_across_generated_paths_is_one_key(self) -> None:
        campaign = self._campaign("same-set-across-paths")
        campaign.add_run(1, torn_shape(R0))
        campaign.add_run(2, torn_shape(R1))
        (entry,) = self._check("same-set-across-paths", campaign)["findings"]
        self.assertEqual(
            [(1, [f"path:{R0}"]), (2, [f"path:{R1}"])],
            [(run["seed"], run["subjects"]) for run in entry["runs"]],
        )

    def test_identity_bearing_variant_is_a_distinct_key(self) -> None:
        campaign = self._campaign("identity-variant")
        campaign.add_run(1, torn_shape(R0, "fuse"))
        campaign.add_run(2, torn_shape(R0, "thrift"))
        summary = self._check("identity-variant", campaign)
        self.assertEqual([[], []], [e["related"] for e in summary["findings"]])
        self.assertEqual(
            [[["channel", "fuse"]], [["channel", "thrift"]]],
            sorted(e["errors"][0][2] for e in summary["findings"]),
        )

    def test_attribution_change_keeps_the_key(self) -> None:
        """Harness evidence beside a product finding never re-keys the subject.

        Seed 1 is the torn read alone; seed 2 adds a harness finding on the
        checkout subject. The torn read's key is the same.
        """
        campaign = self._campaign("attribution-no-rekey")
        campaign.add_run(1, torn_shape())
        campaign.add_run(2, torn_with_harness_finding())
        verdict = load_verdict_file(
            campaign.artifacts / "seed-2" / "verdict.json"
        ).verdict
        (occurrence,) = findings.keyed(verdict, TARGET).occurrences
        self.assertEqual(("undetermined",), occurrence.attributions)
        (entry,) = self._check("attribution-no-rekey", campaign)["findings"]
        self.assertEqual([1, 2], [run["seed"] for run in entry["runs"]])

    def test_subset_and_superset_are_related_not_merged(self) -> None:
        campaign = self._campaign("subset-superset")
        campaign.add_run(1, subset_superset())
        summary = self._check("subset-superset", campaign)
        relations = sorted(
            r["relation"] for e in summary["findings"] for r in e["related"]
        )
        self.assertEqual(["subset", "superset"], relations)
