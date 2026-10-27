# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import re
import unittest
from collections.abc import Mapping
from dataclasses import replace
from typing import Any

from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    DEADLINE_RUN,
    Environment,
    FINISHED,
    RunSpec,
    SpecError,
    STARTED,
    validate,
)
from eden.integration.correctness.exploratory.core.sites import POST_EFFECT, PRE_EFFECT
from eden.integration.correctness.exploratory.core.target import Target
from eden.integration.correctness.exploratory.edenfs import sites as edenfs_sites
from eden.integration.correctness.exploratory.edenfs.sites import FETCH_TREE_SITE, SITES
from eden.integration.correctness.exploratory.edenfs.test.testing import VOCAB_TARGET


_REGISTER = "tree/registers/r0.reg"


def _assert_accepted(test: unittest.TestCase, spec: RunSpec, target: Target) -> None:
    """Fail `test` with the rejection reason unless `spec` validates."""
    try:
        validate(spec, target)
    except SpecError as error:
        test.fail(f"spec rejected: {error}")


def _spec(
    actions: list[Action],
    actors: list[tuple[str, list[str]]],
    setup_profile: Mapping[str, bool | int] | None = None,
) -> RunSpec:
    profile: dict[str, bool | int] = {"materialize": False, "scatter_dirty": 0}
    if setup_profile is not None:
        profile.update(setup_profile)
    return RunSpec(
        seed=37,
        target=VOCAB_TARGET.name,
        environment=Environment(
            target={
                "repository": {
                    "file_count": 32,
                    "commit_count": 4,
                    "registers": [_REGISTER],
                },
                "channel": "fuse",
                "case_sensitive": True,
                "fixture_source": "backing",
                "setup_profile": profile,
            }
        ),
        budgets=Budgets(30.0, 10.0, 600.0),
        actors=tuple(Actor(id=name, actions=tuple(ids)) for name, ids in actors),
        actions=tuple(actions),
    )


def _install(site: str, target: str) -> Action:
    return Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        payload={
            "fault_id": "F",
            "site": site,
            "target_action": target,
            "behavior": "block",
        },
    )


def _await_and_remove(target: str) -> list[Action]:
    return [
        Action(
            id="h1",
            kind="fault_await",
            deadline=DEADLINE_OPERATION,
            requires=((target, STARTED),),
            payload={"fault_id": "F"},
        ),
        Action(
            id="x1",
            kind="fault_remove",
            deadline=DEADLINE_OPERATION,
            requires=(("h1", FINISHED),),
            payload={"fault_id": "F"},
        ),
    ]


def _parked_read(*requires: tuple[str, str]) -> Action:
    return Action(
        id="t1",
        kind="read",
        deadline=DEADLINE_RUN,
        requires=requires,
        payload={"path": _REGISTER, "channel": "fuse"},
    )


def _fetch_install(target: str, ordinal: int = 1) -> Action:
    return Action(
        id="f1",
        kind="fault_install",
        deadline=DEADLINE_OPERATION,
        payload={
            "fault_id": "F",
            "site": FETCH_TREE_SITE,
            "target_action": target,
            "behavior": "block",
            "ordinal": ordinal,
        },
    )


def _parked_status(channel: str = "thrift", revision: int | None = None) -> Action:
    payload: dict[str, Any] = {"channel": channel}
    if revision is not None:
        payload["revision"] = revision
    return Action(
        id="s1",
        kind="status",
        deadline=DEADLINE_RUN,
        requires=(("f1", FINISHED),),
        payload=payload,
    )


class EdenFSSiteTest(unittest.TestCase):
    def test_new_sites_declared(self) -> None:
        """Internal write and load checkpoints require the stack daemon."""
        for name, kind in (
            ("overlay-write", "write"),
            ("inodemap-load", "getattr"),
            ("journal-record", "write"),
        ):
            with self.subTest(site=name):
                site = SITES[name]
                self.assertEqual(site.key_class, name)
                self.assertEqual(site.target_kinds, frozenset({kind}))
                self.assertEqual(site.daemon, "stack-build")
                self.assertGreaterEqual(site.behaviors, {"block", "error"})

    def test_lookup_requires_stack_daemon(self) -> None:
        """The FUSE lookup checkpoint remains available only on the stack daemon."""
        self.assertEqual("stack-build", SITES[edenfs_sites.LOOKUP_SITE].daemon)

    def test_only_sites_past_their_targets_effect_are_post(self) -> None:
        """Journal-record and the checkout sites fire after the effect began; lookup is first."""
        post = {name for name, site in SITES.items() if site.effect == POST_EFFECT}
        self.assertEqual(
            {
                edenfs_sites.JOURNAL_RECORD_SITE,
                edenfs_sites.CHECKOUT_BEGIN_SITE,
                edenfs_sites.TREE_CHECKOUT_SITE,
            },
            post,
        )
        self.assertEqual(PRE_EFFECT, SITES[edenfs_sites.LOOKUP_SITE].effect)

    def test_unobservable_pairs_have_counts(self) -> None:
        """Each generation exclusion retains its observed fault-miss count."""
        expected = {
            ("tree-checkout", "checkout"): 27,
            ("wait-for-pending-writes", "read"): 23,
            ("fetch-tree", "status"): 15,
            ("status-cache-fill", "status"): 11,
        }
        self.assertEqual(set(expected), set(edenfs_sites.UNOBSERVABLE_PAIRS))
        for pair, reason in edenfs_sites.UNOBSERVABLE_PAIRS.items():
            self.assertRegex(reason, r"^\d+ fault-miss seeds")
            self.assertEqual(f"{expected[pair]} fault-miss seeds", reason)

    def test_fetch_tree_catalog_contract(self) -> None:
        site = SITES[FETCH_TREE_SITE]
        self.assertEqual(site.key_class, "SaplingBackingStore::getRootTree")
        self.assertEqual(site.target_kinds, frozenset({"status"}))
        self.assertEqual(site.behaviors, frozenset({"block"}))
        self.assertEqual(site.daemon, "public-master")
        self.assertEqual(site.fixed_key, None)
        self.assertFalse(site.name_keyed)
        self.assertTrue(site.witness_any)

    def test_fetch_tree_accepts_current_status_channels(self) -> None:
        for channel, revision in (
            ("thrift", None),
            ("thrift", 3),
            ("sl", None),
            ("sl", 2),
        ):
            with self.subTest(channel=channel, revision=revision):
                actions = [
                    _fetch_install("s1"),
                    _parked_status(channel, revision),
                    *_await_and_remove("s1"),
                ]
                _assert_accepted(
                    self,
                    _spec(
                        actions,
                        [
                            ("installer", ["f1"]),
                            ("status", ["s1"]),
                            ("fault", ["h1", "x1"]),
                        ],
                    ),
                    VOCAB_TARGET,
                )

    def test_fetch_tree_rejects_unsupported_binding(self) -> None:
        cases = (
            (_parked_status(revision=2), _fetch_install("s1"), "current parent"),
            (_parked_status(), _fetch_install("s1", 2), "ordinal"),
        )
        for target, install, reason in cases:
            with self.subTest(reason=reason):
                with self.assertRaisesRegex(SpecError, reason):
                    validate(
                        _spec(
                            [install, target, *_await_and_remove("s1")],
                            [
                                ("installer", ["f1"]),
                                ("status", ["s1"]),
                                ("fault", ["h1", "x1"]),
                            ],
                        ),
                        VOCAB_TARGET,
                    )

    def test_fetch_tree_rejects_nonstatus_target_and_fixture(self) -> None:
        read = _parked_read(("f1", FINISHED))
        with self.assertRaisesRegex(SpecError, "cannot target kind"):
            validate(
                _spec(
                    [_fetch_install("t1"), read, *_await_and_remove("t1")],
                    [
                        ("installer", ["f1"]),
                        ("target", ["t1"]),
                        ("fault", ["h1", "x1"]),
                    ],
                ),
                VOCAB_TARGET,
            )
        spec = _spec(
            [_fetch_install("s1"), _parked_status(), *_await_and_remove("s1")],
            [
                ("installer", ["f1"]),
                ("status", ["s1"]),
                ("fault", ["h1", "x1"]),
            ],
        )
        with self.assertRaisesRegex(SpecError, "fixture_source"):
            validate(
                RunSpec(
                    spec.seed,
                    spec.target,
                    Environment(
                        target={**spec.environment.target, "fixture_source": "overlay"}
                    ),
                    spec.budgets,
                    spec.actors,
                    spec.actions,
                ),
                VOCAB_TARGET,
            )

    def test_fetch_tree_rejects_competing_root_fetch_before_witness(self) -> None:
        actions = [
            _fetch_install("s1"),
            _parked_status(),
            Action(
                id="other",
                kind="checkout",
                deadline=DEADLINE_OPERATION,
                payload={"revision": 2, "via": "goto", "mode": "normal"},
            ),
            *_await_and_remove("s1"),
        ]
        with self.assertRaisesRegex(SpecError, "can hit the key of fault"):
            validate(
                _spec(
                    actions,
                    [
                        ("installer", ["f1"]),
                        ("status", ["s1"]),
                        ("other", ["other"]),
                        ("fault", ["h1", "x1"]),
                    ],
                ),
                VOCAB_TARGET,
            )

    def test_error_fault_keys_exclude_competing_requests_until_consumed(self) -> None:
        """Another request the armed key can match waits for the target's completion.

        A one-shot error fault fires on the first request with its key; a
        competing request admitted while it is armed could consume it and
        fail unexcused.
        """
        write = {"kind": "write", "payload": {"path": _REGISTER}}
        synced = {
            "kind": "read",
            "payload": {
                "path": _REGISTER,
                "channel": "thrift",
                "sync_timeout_seconds": 60,
            },
        }
        checkout = {
            "kind": "checkout",
            "payload": {"revision": 2, "via": "thrift", "mode": "normal"},
        }
        cases = {
            "overlay-write": write,
            "journal-record": write,
            "wait-for-pending-writes": synced,
            "checkout-begin": checkout,
        }
        for site, shape in cases.items():
            for after_target in (False, True):
                with self.subTest(site=site, after_target=after_target):
                    install = Action(
                        id="f1",
                        kind="fault_install",
                        deadline=DEADLINE_OPERATION,
                        payload={
                            "fault_id": "F",
                            "site": site,
                            "target_action": "t1",
                            "behavior": "error",
                        },
                    )
                    target, other = (
                        Action(
                            id=action_id,
                            kind=shape["kind"],
                            deadline=DEADLINE_OPERATION,
                            requires=requires,
                            payload={
                                **shape["payload"],
                                **({"value_id": action_id} if write is shape else {}),
                            },
                        )
                        for action_id, requires in (
                            ("t1", (("f1", FINISHED),)),
                            (
                                "c1",
                                (("f1", FINISHED),)
                                + ((("t1", FINISHED),) if after_target else ()),
                            ),
                        )
                    )
                    remove = Action(
                        id="x1",
                        kind="fault_remove",
                        deadline=DEADLINE_OPERATION,
                        requires=(("t1", FINISHED),),
                        payload={"fault_id": "F"},
                    )
                    spec = _spec(
                        [install, target, other, remove],
                        [("nemesis", ["f1", "x1"]), ("a", ["t1"]), ("b", ["c1"])],
                    )
                    if after_target:
                        validate(spec, VOCAB_TARGET)
                        continue
                    with self.assertRaisesRegex(
                        SpecError, "action 'c1' can hit the key of fault 'F'"
                    ):
                        validate(spec, VOCAB_TARGET)

    def test_every_checkout_and_status_can_consume_the_pending_writes_key(
        self,
    ) -> None:
        """`co_waitForPendingWrites` checks the empty key for every checkout
        (DRY_RUN included) and status, not only for synced Thrift reads."""
        target = Action(
            id="t1",
            kind="read",
            deadline=DEADLINE_OPERATION,
            requires=(("f1", FINISHED),),
            payload={
                "path": _REGISTER,
                "channel": "thrift",
                "sync_timeout_seconds": 60,
            },
        )
        install = Action(
            id="f1",
            kind="fault_install",
            deadline=DEADLINE_OPERATION,
            payload={
                "fault_id": "F",
                "site": "wait-for-pending-writes",
                "target_action": "t1",
                "behavior": "error",
            },
        )
        remove = Action(
            id="x1",
            kind="fault_remove",
            deadline=DEADLINE_OPERATION,
            requires=(("t1", FINISHED),),
            payload={"fault_id": "F"},
        )
        for kind, payload in (
            ("checkout", {"revision": 2, "via": "thrift", "mode": "normal"}),
            ("status", {"channel": "thrift", "revision": 2}),
        ):
            with self.subTest(kind=kind):
                other = Action(
                    id="c1",
                    kind=kind,
                    deadline=DEADLINE_OPERATION,
                    requires=(("f1", FINISHED),),
                    payload=payload,
                )
                spec = _spec(
                    [install, target, other, remove],
                    [("nemesis", ["f1", "x1"]), ("a", ["t1"]), ("b", ["c1"])],
                )
                with self.assertRaisesRegex(
                    SpecError, "action 'c1' can hit the key of fault 'F'"
                ):
                    validate(spec, VOCAB_TARGET)

    def test_fetch_tree_allows_root_fetch_after_witness(self) -> None:
        actions = [
            _fetch_install("s1"),
            _parked_status(),
            *_await_and_remove("s1"),
            Action(
                id="other",
                kind="status",
                deadline=DEADLINE_OPERATION,
                requires=(("h1", FINISHED),),
                payload={"channel": "sl", "revision": 2},
            ),
        ]
        _assert_accepted(
            self,
            _spec(
                actions,
                [
                    ("installer", ["f1"]),
                    ("status", ["s1"]),
                    ("fault", ["h1", "x1"]),
                    ("other", ["other"]),
                ],
            ),
            VOCAB_TARGET,
        )

    def test_fetch_tree_rejects_remove_waiting_on_parked_status(self) -> None:
        for ordering in ("direct", "transitive", "same-actor"):
            with self.subTest(ordering=ordering):
                actions = [
                    _fetch_install("s1"),
                    _parked_status(),
                    *_await_and_remove("s1"),
                ]
                actors = [
                    ("installer", ["f1"]),
                    ("status", ["s1"]),
                    ("fault", ["h1", "x1"]),
                ]
                remove = actions[3]
                if ordering == "same-actor":
                    actors = [
                        ("installer", ["f1"]),
                        ("status", ["s1", "x1"]),
                        ("fault", ["h1"]),
                    ]
                elif ordering == "direct":
                    actions[3] = replace(
                        remove,
                        requires=(("h1", FINISHED), ("s1", FINISHED)),
                    )
                else:
                    actions.append(
                        Action(
                            id="b1",
                            kind="sleep",
                            deadline=DEADLINE_OPERATION,
                            requires=(("s1", FINISHED),),
                            payload={"seconds": 0.01},
                        )
                    )
                    actions[3] = replace(
                        remove,
                        requires=(("h1", FINISHED), ("b1", FINISHED)),
                    )
                    actors.append(("bridge", ["b1"]))
                with self.assertRaisesRegex(SpecError, "must not wait on"):
                    validate(_spec(actions, actors), VOCAB_TARGET)

    def test_fetch_tree_native_process_kill_can_release_target(self) -> None:
        actions = [
            _fetch_install("s1"),
            _parked_status("sl", 2),
            *_await_and_remove("s1"),
        ]
        actions.append(
            Action(
                id="k1",
                kind="kill_process",
                deadline=DEADLINE_OPERATION,
                requires=(("s1", STARTED), ("h1", FINISHED)),
                payload={"target_action": "s1", "after_ms": 0},
            )
        )
        actions[3] = replace(
            actions[3],
            requires=(
                ("h1", FINISHED),
                ("k1", FINISHED),
            ),
        )
        _assert_accepted(
            self,
            _spec(
                actions,
                [
                    ("installer", ["f1"]),
                    ("status", ["s1"]),
                    ("fault", ["h1", "k1", "x1"]),
                ],
            ),
            VOCAB_TARGET,
        )

    def test_lookup_rejects_thrift_channel(self) -> None:
        target = Action(
            id="t1",
            kind="read",
            deadline=DEADLINE_RUN,
            requires=(("f1", FINISHED),),
            payload={
                "path": _REGISTER,
                "channel": "thrift",
                "sync_timeout_seconds": 60,
            },
        )
        actions = [_install("fuse-lookup", "t1"), target, *_await_and_remove("t1")]
        with self.assertRaisesRegex(SpecError, "fuse channel"):
            validate(
                _spec(
                    actions,
                    [("nemesis", ["f1", "h1", "x1"]), ("target", ["t1"])],
                ),
                VOCAB_TARGET,
            )

    def test_lookup_accepts_fuse_channel(self) -> None:
        actions = [
            _install("fuse-lookup", "t1"),
            _parked_read(("f1", FINISHED)),
            *_await_and_remove("t1"),
        ]
        spec = _spec(
            actions,
            [("nemesis", ["f1", "h1", "x1"]), ("target", ["t1"])],
        )
        _assert_accepted(self, spec, VOCAB_TARGET)

    def test_lookup_rejects_concurrent_same_basename(self) -> None:
        actions = [
            _install("fuse-lookup", "t1"),
            _parked_read(("f1", FINISHED)),
            Action(
                id="r1",
                kind="read",
                deadline=DEADLINE_OPERATION,
                payload={"path": _REGISTER, "channel": "fuse"},
            ),
            *_await_and_remove("t1"),
        ]
        with self.assertRaisesRegex(SpecError, "(?:can look up|target path)"):
            validate(
                _spec(
                    actions,
                    [("nemesis", ["f1", "h1", "x1"]), ("a", ["t1"]), ("b", ["r1"])],
                ),
                VOCAB_TARGET,
            )

    def test_lookup_rejects_other_actor_touch_before_fault_remove(self) -> None:
        """A lookup park excludes a target-path touch anywhere before removal."""
        other = Action(
            id="w1",
            kind="write",
            deadline=DEADLINE_OPERATION,
            payload={"path": _REGISTER, "value_id": "dirty"},
        )
        install = replace(
            _install("fuse-lookup", "t1"),
            requires=(("t1", STARTED), ("w1", FINISHED)),
        )
        target = _parked_read()
        with self.assertRaisesRegex(SpecError, "target path"):
            validate(
                _spec(
                    [target, other, install, *_await_and_remove("t1")],
                    [
                        ("other", ["w1"]),
                        ("nemesis", ["f1", "h1", "x1"]),
                        ("target", ["t1"]),
                    ],
                ),
                VOCAB_TARGET,
            )

    def test_lookup_accepts_other_actor_touch_after_fault_remove(self) -> None:
        """A target-path touch is allowed only after the lookup fault is removed."""
        other = Action(
            id="w1",
            kind="write",
            deadline=DEADLINE_OPERATION,
            requires=(("x1", FINISHED),),
            payload={"path": _REGISTER, "value_id": "after"},
        )
        _assert_accepted(
            self,
            _spec(
                [
                    _install("fuse-lookup", "t1"),
                    _parked_read(("f1", FINISHED)),
                    *_await_and_remove("t1"),
                    other,
                ],
                [
                    ("nemesis", ["f1", "h1", "x1"]),
                    ("target", ["t1"]),
                    ("other", ["w1"]),
                ],
            ),
            VOCAB_TARGET,
        )

    def test_lookup_rejects_other_actor_getattr_before_fault_remove(self) -> None:
        """Getattr on the parked path is also excluded before removal."""
        other = Action(
            id="g1",
            kind="getattr",
            deadline=DEADLINE_OPERATION,
            payload={"path": _REGISTER},
        )
        install = replace(
            _install("fuse-lookup", "t1"),
            requires=(("t1", STARTED), ("g1", FINISHED)),
        )
        with self.assertRaisesRegex(SpecError, "target path"):
            validate(
                _spec(
                    [_parked_read(), other, install, *_await_and_remove("t1")],
                    [
                        ("other", ["g1"]),
                        ("nemesis", ["f1", "h1", "x1"]),
                        ("target", ["t1"]),
                    ],
                ),
                VOCAB_TARGET,
            )

    def test_lookup_rejects_warm_target(self) -> None:
        actions = [
            Action(
                id="f1",
                kind="fault_install",
                deadline=DEADLINE_OPERATION,
                requires=(("w1", FINISHED),),
                payload={
                    "fault_id": "F",
                    "site": "fuse-lookup",
                    "target_action": "t1",
                    "behavior": "block",
                },
            ),
            Action(
                id="w1",
                kind="write",
                deadline=DEADLINE_OPERATION,
                payload={"path": _REGISTER, "value_id": "before"},
            ),
            _parked_read(("f1", FINISHED)),
            *_await_and_remove("t1"),
        ]
        with self.assertRaisesRegex(SpecError, "reads warmed register"):
            validate(
                _spec(
                    actions,
                    [("nemesis", ["f1", "h1", "x1"]), ("a", ["w1", "t1"])],
                ),
                VOCAB_TARGET,
            )

    def test_lookup_rejects_setup_warmed_target(self) -> None:
        """A FUSE lookup park is only meaningful against a cold target."""
        actions = [
            _install("fuse-lookup", "t1"),
            _parked_read(("f1", FINISHED)),
            *_await_and_remove("t1"),
        ]
        actors = [("nemesis", ["f1", "h1", "x1"]), ("target", ["t1"])]
        for profile, reason in (
            ({"warm_readdir": True}, "warm_readdir"),
            ({"aged_overlay": 1}, "aged_overlay"),
        ):
            with self.subTest(profile=profile):
                with self.assertRaisesRegex(SpecError, reason):
                    validate(_spec(actions, actors, profile), VOCAB_TARGET)

    def test_inodemap_load_validate_rejects_loaded_target(self) -> None:
        """A target read or written before its install arms cannot load on demand."""
        for kind, payload in (
            (("write", {"path": _REGISTER, "value_id": "warmed"})),
            (("read", {"path": _REGISTER, "channel": "fuse"})),
        ):
            with self.subTest(kind=kind):
                install = replace(
                    _install("inodemap-load", "t1"), requires=(("w1", FINISHED),)
                )
                target = Action(
                    id="t1",
                    kind="getattr",
                    deadline=DEADLINE_RUN,
                    requires=(("f1", FINISHED),),
                    payload={"path": _REGISTER},
                )
                warmer = Action(
                    id="w1", kind=kind, deadline=DEADLINE_OPERATION, payload=payload
                )
                spec = _spec(
                    [warmer, install, target, *_await_and_remove("t1")],
                    [
                        ("setup", ["w1"]),
                        ("nemesis", ["f1", "h1", "x1"]),
                        ("target", ["t1"]),
                    ],
                )
                with self.assertRaisesRegex(SpecError, re.escape(_REGISTER)):
                    validate(spec, VOCAB_TARGET)

    def test_inodemap_load_rejects_install_without_covering_unload(self) -> None:
        """An install whose target was never unloaded cannot reach its checkpoint."""
        install = _install("inodemap-load", "t1")
        target = Action(
            id="t1",
            kind="getattr",
            deadline=DEADLINE_RUN,
            requires=(("f1", FINISHED),),
            payload={"path": _REGISTER},
        )
        spec = _spec(
            [install, target, *_await_and_remove("t1")],
            [
                ("nemesis", ["f1", "h1", "x1"]),
                ("target", ["t1"]),
            ],
        )
        with self.assertRaisesRegex(SpecError, re.escape(_REGISTER)):
            validate(spec, VOCAB_TARGET)

    def test_inodemap_load_rejects_same_path_access_while_armed(self) -> None:
        """A load failure fails every pending lookup of the inode
        (`InodeMap::inodeLoadFailed`), so a same-path access waits for the
        target's consumption."""
        for kind, payload in (
            ("read", {"path": _REGISTER, "channel": "fuse"}),
            ("open", {"path": _REGISTER}),
            ("chmod", {"path": _REGISTER, "mode": 0o644}),
        ):
            for after_target in (False, True):
                with self.subTest(kind=kind, after_target=after_target):
                    unload = Action(
                        id="u1",
                        kind="unload_inodes",
                        deadline=DEADLINE_OPERATION,
                        payload={"path": "tree/registers"},
                    )
                    install = replace(
                        _install("inodemap-load", "t1"),
                        requires=(("u1", FINISHED),),
                        payload={
                            **_install("inodemap-load", "t1").payload,
                            "behavior": "error",
                        },
                    )
                    target = Action(
                        id="t1",
                        kind="getattr",
                        deadline=DEADLINE_OPERATION,
                        requires=(("f1", FINISHED),),
                        payload={"path": _REGISTER},
                    )
                    other = Action(
                        id="c1",
                        kind=kind,
                        deadline=DEADLINE_OPERATION,
                        requires=(("f1", FINISHED),)
                        + ((("t1", FINISHED),) if after_target else ()),
                        payload=payload,
                    )
                    remove = Action(
                        id="x1",
                        kind="fault_remove",
                        deadline=DEADLINE_OPERATION,
                        requires=(("t1", FINISHED),),
                        payload={"fault_id": "F"},
                    )
                    spec = _spec(
                        [unload, install, target, other, remove],
                        [("nemesis", ["u1", "f1", "x1"]), ("a", ["t1"]), ("b", ["c1"])],
                    )
                    if after_target:
                        validate(spec, VOCAB_TARGET)
                        continue
                    with self.assertRaisesRegex(
                        SpecError, "action 'c1' can hit the key of fault 'F'"
                    ):
                        validate(spec, VOCAB_TARGET)

    def test_journal_record_key_excludes_force_checkout_while_armed(self) -> None:
        """A FORCE checkout records conflicting paths in the journal; overlay-write
        keys only on writes, so the same checkout is no collateral there."""
        for site, mode, rejects in (
            ("journal-record", "force", True),
            ("journal-record", "normal", False),
            ("overlay-write", "force", False),
        ):
            with self.subTest(site=site, mode=mode):
                install = Action(
                    id="f1",
                    kind="fault_install",
                    deadline=DEADLINE_OPERATION,
                    payload={
                        "fault_id": "F",
                        "site": site,
                        "target_action": "t1",
                        "behavior": "error",
                    },
                )
                target = Action(
                    id="t1",
                    kind="write",
                    deadline=DEADLINE_OPERATION,
                    requires=(("f1", FINISHED),),
                    payload={"path": _REGISTER, "value_id": "t1"},
                )
                checkout = Action(
                    id="c1",
                    kind="checkout",
                    deadline=DEADLINE_OPERATION,
                    requires=(("f1", FINISHED),),
                    payload={"revision": 2, "via": "goto", "mode": mode},
                )
                remove = Action(
                    id="x1",
                    kind="fault_remove",
                    deadline=DEADLINE_OPERATION,
                    requires=(("t1", FINISHED),),
                    payload={"fault_id": "F"},
                )
                spec = _spec(
                    [install, target, checkout, remove],
                    [("nemesis", ["f1", "x1"]), ("a", ["t1"]), ("b", ["c1"])],
                )
                if not rejects:
                    validate(spec, VOCAB_TARGET)
                    continue
                with self.assertRaisesRegex(
                    SpecError, "action 'c1' can hit the key of fault 'F'"
                ):
                    validate(spec, VOCAB_TARGET)

    def test_cache_fill_rejects_post_dirt_probe(self) -> None:
        actions = [
            Action(
                id="w1",
                kind="write",
                deadline=DEADLINE_OPERATION,
                payload={"path": _REGISTER, "value_id": "dirty"},
            ),
            Action(
                id="p1",
                kind="status",
                deadline=DEADLINE_OPERATION,
                payload={"channel": "sl"},
            ),
            _install("status-cache-fill", "s1"),
            Action(
                id="s1",
                kind="status",
                deadline=DEADLINE_RUN,
                requires=(("f1", FINISHED),),
                payload={"channel": "sl"},
            ),
            *_await_and_remove("s1"),
        ]
        no_dirt = _spec(
            actions[1:],
            [("setup", ["p1", "f1"]), ("target", ["s1"]), ("nemesis", ["h1", "x1"])],
        )
        validate(no_dirt, VOCAB_TARGET)
        self.assertEqual("status", no_dirt.action("p1").kind)

        pre_dirt = _spec(
            actions,
            [
                ("setup", ["p1", "w1", "f1"]),
                ("target", ["s1"]),
                ("nemesis", ["h1", "x1"]),
            ],
        )
        validate(pre_dirt, VOCAB_TARGET)
        self.assertEqual("write", pre_dirt.action("w1").kind)

        with self.assertRaisesRegex(SpecError, "fills the status cache"):
            validate(
                _spec(
                    actions,
                    [
                        ("setup", ["w1", "p1", "f1"]),
                        ("target", ["s1"]),
                        ("nemesis", ["h1", "x1"]),
                    ],
                ),
                VOCAB_TARGET,
            )

    def test_every_site_has_daemon_label(self) -> None:
        self.assertEqual(9, len(SITES))
        self.assertTrue(
            all(
                site.daemon in {"public-master", "stack-build"}
                for site in SITES.values()
            )
        )
