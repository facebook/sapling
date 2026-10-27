# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Campaign planning: the admitted strata derive from the target's tables and kill modes
and exclude sites the daemon build cannot reach, generate and campaign share one
admission, every seed is assigned a cycled admitted stratum, a thousand planned specs
cover every admitted site and stratum, and no-fault campaigns compose fault-free
specs.
"""

from __future__ import annotations

import dataclasses
import json
import random
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest import mock

from eden.integration.correctness.exploratory.cli import _main
from eden.integration.correctness.exploratory.core import containment
from eden.integration.correctness.exploratory.core.campaign import admitted_strata, plan
from eden.integration.correctness.exploratory.core.checker import Verdict
from eden.integration.correctness.exploratory.core.containment import (
    COMPLETED,
    Execution,
    ExplorationResult,
)
from eden.integration.correctness.exploratory.core.generate import (
    compose,
    generation_target,
    shape_signature,
)
from eden.integration.correctness.exploratory.core.runspec import from_json, SpecError
from eden.integration.correctness.exploratory.edenfs.target import (
    sample_environment as sample_edenfs_environment,
)
from eden.integration.correctness.exploratory.fake.target import FAKE
from eden.integration.correctness.exploratory.targets import target_entries
from eden.integration.correctness.exploratory.test.cli_fixtures import cli_argv


# The CLI's EdenFS entry; generation, strata, and sites need only its vocabulary.
EDENFS_TARGET = target_entries()["edenfs"].target


def _campaign_arguments(root: Path) -> list[str]:
    return cli_argv(
        "campaign",
        target="edenfs",
        seeds=2,
        files=32,
        artifacts=root / "artifacts",
        report=root / "report.json",
    )


STACK_BUILD_SITES = ("fuse-lookup", "inodemap-load", "journal-record", "overlay-write")


class CampaignPlanTest(unittest.TestCase):
    def test_generate_and_campaign_share_admission(self) -> None:
        """One admitted tuple backs `generate --stratum` and campaign planning.

        Under the default daemon label admission covers the public-master
        sites, with ordinary fault behaviors and optional process kills.
        """
        admitted = admitted_strata(EDENFS_TARGET, "public-master")
        seeds = len(admitted) * len(EDENFS_TARGET.sites)
        planned = plan(EDENFS_TARGET, 0, seeds, "public-master")
        master = set(EDENFS_TARGET.sites) - set(STACK_BUILD_SITES)
        self.assertEqual(master, {stratum[0] for stratum in admitted})
        self.assertEqual(set(admitted), {entry.stratum for entry in planned})
        for stratum in admitted:
            argv = cli_argv("generate", target="edenfs", seed=5, stratum=True)
            with redirect_stdout(StringIO()) as stdout:
                code = _main([*argv, "/".join(stratum)])
            self.assertEqual(0, code, stratum)
            spec = from_json(stdout.getvalue(), EDENFS_TARGET)
            self.assertEqual(stratum, shape_signature(spec, EDENFS_TARGET)[:3])

    def test_admitted_strata_excludes_stack_build_on_master(self) -> None:
        """Stack-build sites are absent on a master daemon and present on a stack build."""
        master = {s[0] for s in admitted_strata(EDENFS_TARGET, "public-master")}
        stack = {s[0] for s in admitted_strata(EDENFS_TARGET, "stack-build")}
        self.assertEqual(set(EDENFS_TARGET.sites) - set(STACK_BUILD_SITES), master)
        self.assertEqual(set(EDENFS_TARGET.sites), stack)
        with self.assertRaises(ValueError):
            admitted_strata(EDENFS_TARGET, "nightly")

    def test_report_lists_excluded_sites(self) -> None:
        """Both report.json files say which sites the daemon label excluded and why."""
        excluded = [
            {
                "site": s,
                "reason": f"site daemon '{EDENFS_TARGET.sites[s].daemon}'"
                " not admitted on 'public-master'",
            }
            for s in STACK_BUILD_SITES
        ]
        for label, expected in (("public-master", excluded), ("stack-build", [])):
            with tempfile.TemporaryDirectory() as tmp, redirect_stdout(StringIO()):
                root = Path(tmp)
                argv = _campaign_arguments(root)
                _main([*argv, "--seeds", "0", "--daemon", label])
                for path in (root / "report.json", root / "artifacts" / "report.json"):
                    document = json.loads(path.read_text())
                    self.assertEqual(expected, document["excluded_sites"], label)

    def test_strata_derive_from_target_tables_and_kill_modes(self) -> None:
        expected = {
            (site_name, behavior, kill_mode)
            for site_name, site in EDENFS_TARGET.sites.items()
            for behavior in site.behaviors
            for kill_mode in ("none", "process")
        }
        self.assertEqual(expected, set(admitted_strata(EDENFS_TARGET, "stack-build")))
        self.assertEqual((), admitted_strata(FAKE, "public-master"))

    def test_plan_assigns_every_seed_a_cycled_admitted_stratum(self) -> None:
        target = generation_target(EDENFS_TARGET)
        attempts = plan(target, 500, 48, "public-master")
        self.assertEqual(
            list(range(500, 500 + len(attempts))), [a.seed for a in attempts]
        )
        self.assertTrue(all(a.stratum is not None for a in attempts))
        environment = {
            "repository": {
                "file_count": 32,
                "commit_count": 4,
                "registers": ["tree/registers/r0.reg", "tree/registers/r1.reg"],
            },
            "channel": "fuse",
            "case_sensitive": True,
            "fixture_source": "backing",
            "setup_profile": {"materialize": False, "scatter_dirty": 0},
        }
        for entry in attempts:
            assert entry.stratum is not None
            spec = compose(
                target,
                entry.seed,
                environment,
                stratum=entry.stratum,
                forced=True,
            )
            self.assertEqual(entry.stratum, shape_signature(spec, EDENFS_TARGET)[:3])

    def test_thousand_planned_specs_cover_every_admitted_site_and_stratum(
        self,
    ) -> None:
        """Cycled master-daemon plans keep every admitted site and stratum.

        Unplanned composition drops four of five master sites through
        generation filtering, so planned campaigns skew to checkout-begin.
        Cycling the admitted set keeps each admitted site at or above
        fifteen percent of specs and each admitted stratum at or above
        one percent, with the drawn site preserved on every seed.
        """
        master = dataclasses.replace(
            EDENFS_TARGET,
            sites={
                name: site
                for name, site in EDENFS_TARGET.sites.items()
                if site.daemon == "public-master"
            },
        )
        admitted = admitted_strata(master, "public-master")
        self.assertEqual(5, len(master.sites))
        self.assertEqual(22, len(admitted))
        entries = plan(master, 0, 1000, "public-master")
        self.assertTrue(all(entry.stratum is not None for entry in entries))
        sites: dict[str, int] = {}
        strata: dict[tuple[str, str, str], int] = {}
        unfilled = 0
        for entry in entries:
            assert entry.stratum is not None
            try:
                spec = compose(
                    master,
                    entry.seed,
                    sample_edenfs_environment(random.Random(entry.seed)),
                    stratum=entry.stratum,
                    forced=True,
                )
            except (SpecError, ValueError):
                unfilled += 1
                continue
            actual = shape_signature(spec, EDENFS_TARGET)[:3]
            sites[actual[0]] = sites.get(actual[0], 0) + 1
            strata[actual] = strata.get(actual, 0) + 1
        distribution = {
            "unfilled": unfilled,
            "sites": sites,
            "strata": len(strata),
        }
        self.assertEqual(0, unfilled, distribution)
        for site in master.sites:
            self.assertGreaterEqual(sites.get(site, 0), 150, distribution)
        for stratum in admitted:
            self.assertGreaterEqual(strata.get(stratum, 0), 10, distribution)

    def test_compose_width_8_contended_shape(self) -> None:
        """The composer emits the widest contended shape for the fake target."""
        spec = compose(FAKE, 7, {})
        self.assertEqual(8, len(spec.actors))
        writes = [action for action in spec.actions if action.kind == "write"]
        self.assertEqual(8, len(writes))
        self.assertEqual({"register-0"}, {action.payload["path"] for action in writes})

    @mock.patch.object(containment, "run_exploration")
    def test_cli_campaign_no_fault_composes_fault_free_specs(
        self, run: mock.Mock
    ) -> None:
        """`campaign --no-fault` plans no strata and hands the runner specs with no nemesis."""
        run.return_value = ExplorationResult(
            Execution(COMPLETED, "ok", 1, 0), Verdict("accept", "ok"), "seed"
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            argv = cli_argv("campaign", target="edenfs", files=32, no_fault=True)
            with redirect_stdout(StringIO()):
                code = _main([*argv, "--seeds", "3", "--artifacts", str(root)])
            summary = [
                json.loads(line)
                for line in (root / "summary.jsonl").read_text().splitlines()
            ]
            strata = json.loads((root / "strata.json").read_text())
        self.assertEqual(0, code)
        self.assertEqual([None, None, None], [row["stratum"] for row in summary])
        self.assertEqual([], strata["observable"])
        self.assertEqual(3, run.call_count)
        for call in run.call_args_list:
            kinds = {action.kind for action in call.args[0].actions}
            self.assertFalse(
                any(EDENFS_TARGET.actions[k].axis == "nemesis" for k in kinds), kinds
            )

    def test_cli_generate_no_fault_and_its_stratum_conflict(self) -> None:
        """`generate --no-fault` prints a fault-free spec; adding `--stratum` is an error."""
        argv = cli_argv("generate", target="edenfs", seed=5, no_fault=True)
        with redirect_stdout(StringIO()) as out:
            self.assertEqual(0, _main(argv))
        kinds = {a["kind"] for a in json.loads(out.getvalue())["actions"].values()}
        self.assertFalse(
            any(EDENFS_TARGET.actions[k].axis == "nemesis" for k in kinds), kinds
        )
        with (
            redirect_stdout(StringIO()) as out,
            redirect_stderr(StringIO()) as err,
        ):
            code = _main([*argv, "--stratum", "tree-checkout/block/none"])
        self.assertEqual((2, ""), (code, out.getvalue()))
        self.assertIn("--no-fault takes no --stratum", err.getvalue())
