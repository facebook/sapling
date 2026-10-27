# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import random
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core.generate import compose
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    finish_before,
    FINISHED,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs import sites as edenfs_sites
from eden.integration.correctness.exploratory.edenfs.actions import CHURN_FILES
from eden.integration.correctness.exploratory.edenfs.encoding import manifest_diff
from eden.integration.correctness.exploratory.edenfs.sites import (
    FETCH_TREE_SITE,
    SITES,
    TREE_CHECKOUT_KEY,
)
from eden.integration.correctness.exploratory.edenfs.target import RECORDING_TARGET
from eden.integration.correctness.repo import GeneratedRepo

_REGISTER = "tree/registers/r0.reg"
_REGISTERS = (_REGISTER, "tree/registers/r1.reg")


def _engage_env() -> dict[str, Any]:
    return {
        "repository": {
            "file_count": 256,
            "commit_count": 4,
            "registers": list(_REGISTERS),
        },
        "channel": "fuse",
        "case_sensitive": True,
        "fixture_source": "backing",
        "setup_profile": {"materialize": False, "scatter_dirty": 0},
    }


def _engage_ctx(seed: int = 7) -> dict[str, Any]:
    return {"seed": seed, "environment": _engage_env()}


def _engaged_spec(site: str, seed: int) -> RunSpec:
    return compose(
        RECORDING_TARGET,
        seed,
        _engage_env(),
        stratum=(site, "block", "none"),
        forced=True,
    )


def _install_and_target(spec: RunSpec) -> tuple[Action, Action]:
    install = next(a for a in spec.actions if a.kind == "fault_install")
    return install, spec.action(install.payload["target_action"])


class EdenFSSiteEngageTest(unittest.TestCase):
    def test_inodemap_load_engage_targets_untouched_tree_path(self) -> None:
        """Engaged loads target a cold generated path no earlier action touches."""
        for seed in range(1000):
            behavior = "block" if seed % 2 else "error"
            kill = ("none", "process")[seed % 2]
            spec = compose(
                RECORDING_TARGET,
                seed,
                _engage_env(),
                stratum=("inodemap-load", behavior, kill),
                forced=True,
            )
            _, target = _install_and_target(spec)
            self.assertIn(target.kind, {"getattr", "read"})
            if target.kind == "read":
                self.assertEqual("fuse", target.payload["channel"])
            path = target.payload["path"]
            repository = spec.environment.target["repository"]
            self.assertNotIn(path, repository["registers"])
            self.assertNotIn(path, CHURN_FILES)
            layout = GeneratedRepo(
                seed, repository["file_count"], repository["commit_count"]
            )
            tip = repository["commit_count"] - 1
            paths = {
                layout.file_at(index, tip).path
                for index in range(repository["file_count"])
            }
            self.assertIn(path, paths)
            earlier = finish_before(spec, target.id)
            for action in spec.actions:
                if action.id in earlier and action.kind in {"write", "read", "getattr"}:
                    self.assertNotEqual(path, action.payload.get("path"))
            profile = spec.environment.target["setup_profile"]
            self.assertEqual(False, profile["materialize"])
            self.assertEqual(False, profile["warm_readdir"])

    def test_each_site_engage_plan_shapes_its_composed_spec(self) -> None:
        """One offline engage case per site that declares an engage plan."""
        for site, seed in (
            (FETCH_TREE_SITE, 11),
            (edenfs_sites.STATUS_CACHE_FILL_SITE, 12),
            (edenfs_sites.PENDING_WRITES_SITE, 13),
            (edenfs_sites.TREE_CHECKOUT_SITE, 14),
            (edenfs_sites.CHECKOUT_BEGIN_SITE, 15),
        ):
            with self.subTest(site=site):
                engage = SITES[site].engage
                self.assertIsNotNone(engage)
                assert engage is not None
                plan = engage(random.Random(0), _engage_ctx())
                spec = _engaged_spec(site, seed)
                install, target = _install_and_target(spec)
                if site == edenfs_sites.CHECKOUT_BEGIN_SITE:
                    self.assertEqual({}, dict(plan.setup_profile_overrides))
                    self.assertEqual((), plan.pre_install_actions)
                    self.assertEqual({}, dict(plan.target_payload))
                    continue
                if site == edenfs_sites.TREE_CHECKOUT_SITE:
                    self._check_tree_checkout_engage(plan, install, target)
                    continue
                ((kind, payload),) = plan.pre_install_actions
                self.assertEqual("write", kind)
                self.assertIn(payload["path"], _REGISTERS)
                dirt = spec.action("e0")
                self.assertEqual("write", dirt.kind)
                self.assertIn(("b0", FINISHED), dirt.requires)
                self.assertIn(("e0", FINISHED), install.requires)
                if site == edenfs_sites.PENDING_WRITES_SITE:
                    self.assertEqual(payload["path"], dict(plan.target_payload)["path"])
                    self.assertEqual("read", target.kind)
                    self.assertEqual("thrift", target.payload["channel"])
                    self.assertGreaterEqual(target.payload["sync_timeout_seconds"], 0)
                    self.assertEqual(target.payload["path"], dirt.payload["path"])
                else:
                    self.assertEqual("status", target.kind)
                    self.assertEqual("sl", target.payload["channel"])
                    if site == FETCH_TREE_SITE:
                        self.assertEqual(
                            {"channel": "sl", "revision": None},
                            dict(plan.target_payload),
                        )
                        self.assertNotIn("revision", target.payload)

    def _check_tree_checkout_engage(
        self, plan: Any, install: Action, target: Action
    ) -> None:
        env = _engage_env()
        count = env["repository"]["file_count"]
        tip = env["repository"]["commit_count"] - 1
        self.assertEqual((), plan.pre_install_actions)
        self.assertEqual("thrift", dict(plan.target_payload)["via"])
        for revision, seed in (
            (dict(plan.target_payload)["revision"], 7),
            (target.payload["revision"], 14),
        ):
            self.assertLess(revision, tip)
            deleted, added, _type_changed = manifest_diff(
                GeneratedRepo(seed, count, tip + 1), revision, tip
            )
            changed = (*deleted, *added)
            self.assertTrue(
                any(
                    path == TREE_CHECKOUT_KEY
                    or path.startswith(TREE_CHECKOUT_KEY + "/")
                    for path in changed
                )
            )
            dirs = {path.rsplit("/", 1)[0] for path in changed}
            self.assertLessEqual(install.payload["ordinal"], len(dirs))
        self.assertEqual(TREE_CHECKOUT_KEY, install.payload["key"])
        self.assertEqual(1, install.payload["ordinal"])
