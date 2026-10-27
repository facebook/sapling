# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Which actions a differential run pairs with the reference side: one applicability per
kind, Eden-only channels and listings, what the reference side may never do (list
directories, be sampled for checkouts on a case-insensitive mount), paired event
sizes, and differential spec validation.
"""

from __future__ import annotations

import dataclasses
import json
import random
import unittest
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast

from eden.integration.correctness.exploratory.core.differential import SideResult
from eden.integration.correctness.exploratory.core.differential_schedule import (
    observe,
    reference_facts,
)
from eden.integration.correctness.exploratory.core.effects import (
    Footprint,
    footprints_interact,
    ROOT,
)
from eden.integration.correctness.exploratory.core.facts import HistoryFacts
from eden.integration.correctness.exploratory.core.history import (
    Event,
    event_line,
    history_problems,
    MAX_EVENT_BYTES,
    MAX_HISTORY_BYTES,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    CASE_INSENSITIVE_MOUNT,
    differential_applicability,
    differential_footprint,
    DIFFERENTIAL_OBSERVATIONS,
    EDEN_ONLY_CHANNEL,
    EDEN_ONLY_HELD_INPUT,
    EDEN_ONLY_NEMESIS,
    EDEN_ONLY_OBSERVATION,
    expects_reference,
    HARNESS_ONLY,
    HELD_INPUTS,
    HELD_WIDE_DIR,
    ROOT_LISTING,
    timeout_capable,
)
from eden.integration.correctness.exploratory.edenfs.test.actions_fixtures import (
    action_payload,
    differential_spec_text,
    kind_action,
    nominal_spec_text,
    nominal_target,
    one_action_spec,
    R0,
    WIDE_DIR,
)
from eden.integration.correctness.repo import GeneratedRepo, WideDir


_ALL_KINDS = ACTION_DEFS


_NO_NEMESIS = cast(RunSpec, SimpleNamespace(actions=()))


_SAMPLE_PAYLOADS: dict[str, dict[str, Any]] = {
    "read": {"path": R0, "channel": "fuse"},
    "readdir": {"path": "tree/registers", "channel": "fuse"},
    "status": {"channel": "sl"},
    "checkout": {"revision": 1, "via": "goto", "mode": "normal"},
}


class DifferentialTableTest(unittest.TestCase):
    def test_every_kind_has_one_applicability(self) -> None:
        reasons = {}
        for kind in _ALL_KINDS:
            verdict = differential_applicability(
                _NO_NEMESIS, kind_action(kind, **_SAMPLE_PAYLOADS.get(kind, {}))
            )
            reasons[kind] = "shared" if verdict.shared else verdict.reason
        self.assertEqual(
            {kind for kind, reason in reasons.items() if reason == "shared"},
            set(DIFFERENTIAL_OBSERVATIONS),
        )
        self.assertEqual(
            {
                EDEN_ONLY_NEMESIS,
                EDEN_ONLY_OBSERVATION,
                EDEN_ONLY_HELD_INPUT,
                HARNESS_ONLY,
                "shared",
            },
            set(reasons.values()),
        )
        for kind in (
            "fault_install",
            "kill_process",
            "background_read",
            "unload_inodes",
        ):
            self.assertEqual(EDEN_ONLY_NEMESIS, reasons[kind])
        with self.assertRaises(ValueError):
            differential_applicability(_NO_NEMESIS, kind_action("unknown"))

    def test_thrift_channels_are_eden_only(self) -> None:
        cases: tuple[tuple[str, dict[str, Any]], ...] = (
            ("read", {"path": R0, "channel": "thrift", "sync_timeout_seconds": 0}),
            ("readdir", {"path": "tree/registers", "channel": "thrift"}),
            ("status", {"channel": "thrift"}),
            ("checkout", {"revision": 1, "via": "thrift", "mode": "force"}),
        )
        for kind, payload in cases:
            with self.subTest(kind=kind):
                verdict = differential_applicability(
                    _NO_NEMESIS, kind_action(kind, **payload)
                )
                self.assertEqual(
                    (False, EDEN_ONLY_CHANNEL), (verdict.shared, verdict.reason)
                )

    def test_the_root_listing_is_eden_only(self) -> None:
        for root in ("", ".", "./"):
            with self.subTest(path=root):
                listing = kind_action("readdir", path=root, channel="fuse")
                self.assertEqual(
                    ROOT_LISTING,
                    differential_applicability(_NO_NEMESIS, listing).reason,
                )

    def test_expects_reference_is_the_shared_predicate(self) -> None:
        raw = json.loads(
            nominal_spec_text(
                {
                    "r": {
                        "kind": "read",
                        "deadline": "operation",
                        "path": R0,
                        "channel": "fuse",
                    },
                },
                [{"id": "a", "actions": ["r"]}],
            )
        )
        plain = from_json(json.dumps(raw), nominal_target())
        raw["environment"]["target"]["setup_profile"]["reference_clone"] = True
        paired = from_json(json.dumps(raw), nominal_target())
        journal = kind_action("journal_window", "j")
        self.assertEqual(
            [False, False, True, False],
            [
                expects_reference(spec, action)
                for spec in (plain, paired)
                for action in (spec.action("r"), journal)
            ],
        )

    def test_getattr_compares_size_for_files_only(self) -> None:
        definition = DIFFERENTIAL_OBSERVATIONS["getattr"]

        def stat(kind: str, size: int) -> SideResult:
            return SideResult(
                "ok", {"fuse_type": kind, "mode": 0o755, "size": size}, 0.0, 1.0
            )

        self.assertEqual(
            observe(stat("directory", 4096), definition),
            observe(stat("directory", 40), definition),
        )
        self.assertNotEqual(
            observe(stat("file", 64), definition),
            observe(stat("file", 0), definition),
        )

    def test_footprints(self) -> None:
        spec = from_json(
            nominal_spec_text(
                {
                    "o": {"kind": "open", "deadline": "operation", "path": R0},
                    "f": {"kind": "read_fd", "deadline": "operation", "fd": "o"},
                },
                [{"id": "a", "actions": ["o", "f"]}],
            ),
            nominal_target(),
        )
        read_fd = differential_footprint(spec, spec.action("f"))
        self.assertEqual(
            Footprint(reads=frozenset({R0}), channels=frozenset({"fuse"})), read_fd
        )
        checkout = differential_footprint(
            spec, kind_action("checkout", **_SAMPLE_PAYLOADS["checkout"])
        )
        self.assertEqual(frozenset({ROOT}), checkout.subtrees_written)
        self.assertTrue(footprints_interact(checkout, read_fd))
        fault = differential_footprint(spec, kind_action("fault_install", fault_id="F"))
        self.assertEqual(Footprint(), fault)
        status = differential_footprint(spec, kind_action("status", channel="sl"))
        write_r1 = differential_footprint(
            spec, kind_action("write", path="tree/registers/r1.reg", value_id="w")
        )
        self.assertTrue(footprints_interact(status, write_r1))
        self.assertFalse(footprints_interact(read_fd, write_r1))
        rename = differential_footprint(
            spec, kind_action("rename", source=R0, destination="tree/registers/moved")
        )
        self.assertEqual(
            frozenset({R0, "tree/registers/moved"}), rename.subtrees_written
        )
        dotted = differential_footprint(
            spec, kind_action("readdir", path="./tree//registers/", channel="fuse")
        )
        self.assertEqual(frozenset({"tree/registers"}), dotted.subtrees_read)
        listing = differential_footprint(
            spec, kind_action("readdir", path="tree/registers", channel="fuse")
        )
        mkdir = differential_footprint(
            spec, kind_action("mkdir", path="tree/registers/d")
        )
        self.assertTrue(footprints_interact(listing, mkdir))

    def test_timeout_capability_is_the_fuse_reply_channel(self) -> None:
        capable = (
            kind_action("read", path=R0, channel="fuse"),
            kind_action("readdir", path="tree", channel="fuse"),
            kind_action("write", path=R0, value_id="w"),
            kind_action("getattr", path=R0),
            kind_action("close", fd="o"),
        )
        incapable = (
            kind_action("read", path=R0, channel="thrift", sync_timeout_seconds=60),
            kind_action("readdir", path="tree", channel="thrift"),
            kind_action("status", channel="sl"),
            kind_action("checkout", revision=1, via="goto", mode="normal"),
            kind_action("sl_mv", source=R0, destination="tree/x"),
            kind_action("fault_install", fault_id="F"),
            kind_action("readdir_stream", path="tree/wide", pause_after=1),
        )
        spec = cast(RunSpec, None)
        history = cast(Any, None)
        for action in capable:
            with self.subTest(action=action.kind):
                self.assertTrue(timeout_capable(spec, history, action))
        for action in incapable:
            with self.subTest(action=action.kind):
                self.assertFalse(timeout_capable(spec, history, action))


class ReferenceChannelRemovalTest(unittest.TestCase):
    def test_reference_is_not_a_readdir_channel(self) -> None:
        raw = json.loads(
            one_action_spec(
                {
                    "kind": "readdir",
                    "deadline": "operation",
                    "path": "tree/registers",
                    "channel": "reference",
                }
            )
        )
        raw["environment"]["target"]["setup_profile"]["reference_clone"] = True
        with self.assertRaisesRegex(SpecError, r"\['fuse', 'thrift'\]"):
            from_json(json.dumps(raw), nominal_target())
        raw["actions"]["a"]["channel"] = "fuse"
        from_json(json.dumps(raw), nominal_target())
        raw["environment"]["target"]["setup_profile"]["reference_clone"] = "yes"
        with self.assertRaisesRegex(SpecError, "reference_clone must be bool"):
            from_json(json.dumps(raw), nominal_target())

    def test_reference_checkout_requires_a_case_sensitive_mount(self) -> None:
        raw = json.loads(
            one_action_spec(
                {
                    "kind": "read",
                    "deadline": "operation",
                    "path": "tree/registers/r0.reg",
                    "channel": "fuse",
                }
            )
        )
        raw["environment"]["target"]["case_sensitive"] = False
        from_json(json.dumps(raw), nominal_target())
        raw["environment"]["target"]["setup_profile"]["reference_clone"] = True
        with self.assertRaisesRegex(SpecError, CASE_INSENSITIVE_MOUNT):
            from_json(json.dumps(raw), nominal_target())
        raw["environment"]["target"]["case_sensitive"] = True
        from_json(json.dumps(raw), nominal_target())

    def test_reference_checkout_is_never_sampled(self) -> None:
        for held in HELD_INPUTS:
            self.assertNotIn("reference_clone", held.setup_profile)
        self.assertIsNone(ACTION_DEFS["readdir"].sample(random.Random(0), {}))


class PairedEventSizeTest(unittest.TestCase):
    def test_paired_wide_listing_fits_the_event_bound(self) -> None:
        """The held wide directory's listing, on both sides, is one bounded event."""
        layout = GeneratedRepo(1, wide_dir=WideDir(**HELD_WIDE_DIR))
        names = sorted(Path(f.path).name for f in layout.wide_files())
        self.assertEqual(HELD_WIDE_DIR["entries"], len(names))
        listing_bytes = len(json.dumps(names, separators=(",", ":")).encode())
        self.assertGreater(listing_bytes, 150 * 1024)
        self.assertLess(listing_bytes, 165 * 1024)
        raw = json.loads(
            nominal_spec_text(
                {
                    f"d{index}": {
                        "kind": "readdir",
                        "deadline": "operation",
                        "path": HELD_WIDE_DIR["name"],
                        "channel": "fuse",
                    }
                    for index in range(2)
                },
                [{"id": "a", "actions": ["d0", "d1"]}],
            )
        )
        raw["environment"]["target"]["setup_profile"].update(
            wide_dir=HELD_WIDE_DIR, reference_clone=True
        )
        spec = from_json(json.dumps(raw), nominal_target())
        events: list[Event] = []
        for index, action in enumerate(spec.actions):
            reference = SideResult(
                "ok", {"channel": "fuse", "entries": names}, index + 0.1, index + 0.4
            )
            events += [
                Event("invoke", float(index), {"action": action.id}),
                Event(
                    "ok",
                    index + 0.5,
                    {
                        "action": action.id,
                        "channel": "fuse",
                        "entries": names,
                        **reference_facts(reference),
                    },
                ),
            ]
        size = len(event_line(events[1]))
        self.assertGreater(size, 2 * listing_bytes)
        self.assertLess(size, MAX_EVENT_BYTES)
        self.assertGreaterEqual(MAX_HISTORY_BYTES // size, 50)
        self.assertEqual((), history_problems(spec, events))
        target = dataclasses.replace(
            nominal_target(), expects_reference=expects_reference
        )
        facts = HistoryFacts.build(spec, events, target)
        self.assertEqual(
            {"d0": (), "d1": ()},
            {
                a.id: a.reference.problems
                for a in facts.actions.values()
                if a.reference is not None
            },
        )


_ADMITTED: dict[str, dict[str, Any]] = {
    "write": action_payload("write", path=R0, value_id="w"),
    "read": action_payload("read", path=R0, channel="fuse"),
    "getattr": action_payload("getattr", path=R0),
    "open": action_payload("open", path=R0),
    "read_fd": action_payload("read_fd", fd="open"),
    "close": action_payload("close", fd="open"),
    "rename": action_payload("rename", source=R0, destination="tree/registers/moved"),
    "unlink": action_payload("unlink", path="tree/registers/r1.reg"),
    "mkdir": action_payload("mkdir", path="tree/registers/d"),
    "chmod": action_payload("chmod", path="tree/registers/moved", mode=0o755),
    "readdir": action_payload("readdir", path="tree/registers", channel="fuse"),
    "status": action_payload("status", channel="sl"),
    "checkout": action_payload("checkout", revision=0, via="goto", mode="normal"),
    "sl_rm": action_payload("sl_rm", path="tree/registers/r1.reg"),
    "sl_mv": action_payload("sl_mv", source=R0, destination="tree/registers/again"),
}


class DifferentialValidationTest(unittest.TestCase):
    def _two_registers(self, text: str) -> str:
        raw = json.loads(text)
        raw["environment"]["target"]["repository"]["registers"].append(
            "tree/registers/r1.reg"
        )
        return json.dumps(raw)

    def test_every_admitted_kind_is_accepted_with_conflicts_ordered(self) -> None:
        order = [
            "write",
            "read",
            "getattr",
            "open",
            "read_fd",
            "close",
            "unlink",
            "rename",
            "mkdir",
            "chmod",
            "readdir",
            "status",
            "sl_rm",
            "checkout",
            "sl_mv",
        ]
        self.assertEqual(set(DIFFERENTIAL_OBSERVATIONS), set(order))
        text = self._two_registers(
            differential_spec_text(_ADMITTED, [{"id": "a", "actions": order}])
        )
        spec = from_json(text, nominal_target())
        self.assertEqual(set(order), {action.kind for action in spec.actions})
        self.assertTrue(all(expects_reference(spec, action) for action in spec.actions))

    def test_eden_only_kinds_channels_and_root_listings_are_rejected(self) -> None:
        cases: dict[str, tuple[dict[str, Any], str]] = {
            "thrift read": (
                action_payload(
                    "read", path=R0, channel="thrift", sync_timeout_seconds=0
                ),
                EDEN_ONLY_CHANNEL,
            ),
            "thrift readdir": (
                action_payload("readdir", path="tree/registers", channel="thrift"),
                EDEN_ONLY_CHANNEL,
            ),
            "thrift status": (
                action_payload("status", channel="thrift"),
                EDEN_ONLY_CHANNEL,
            ),
            "thrift checkout": (
                action_payload("checkout", revision=0, via="thrift", mode="normal"),
                EDEN_ONLY_CHANNEL,
            ),
            "force checkout": (
                action_payload("checkout", revision=0, via="thrift", mode="force"),
                EDEN_ONLY_CHANNEL,
            ),
            "journal": (action_payload("journal_window"), EDEN_ONLY_OBSERVATION),
            "manifest": (
                action_payload("manifest_presence", from_revision=0, to_revision=1),
                EDEN_ONLY_OBSERVATION,
            ),
            "counts": (action_payload("status_counts"), EDEN_ONLY_OBSERVATION),
            "blocked": (action_payload("blocked_faults"), EDEN_ONLY_OBSERVATION),
            "unload": (action_payload("unload_inodes", path="tree"), EDEN_ONLY_NEMESIS),
            "lease": (
                action_payload("background_read", root="tree", seconds=0.5),
                EDEN_ONLY_NEMESIS,
            ),
            "sleep": (action_payload("sleep", seconds=0.0), HARNESS_ONLY),
        }
        for name, (action, reason) in cases.items():
            with self.subTest(case=name), self.assertRaisesRegex(SpecError, reason):
                from_json(
                    differential_spec_text(
                        {"a": action}, [{"id": "x", "actions": ["a"]}]
                    ),
                    nominal_target(),
                )

    def test_faults_kills_and_held_inputs_are_rejected(self) -> None:
        fault = {
            "c": action_payload(
                "checkout",
                revision=0,
                via="goto",
                mode="normal",
                requires=[["f", "finished"]],
            ),
            "f": action_payload(
                "fault_install",
                fault_id="F",
                target_action="c",
                site="checkout-begin",
                behavior="error",
            ),
            "x": action_payload(
                "fault_remove", fault_id="F", requires=[["c", "finished"]]
            ),
        }
        with self.assertRaisesRegex(
            SpecError, f"'c' \\(checkout\\).*|{EDEN_ONLY_NEMESIS}"
        ):
            from_json(
                differential_spec_text(
                    fault,
                    [{"id": "n", "actions": ["f", "x"]}, {"id": "a", "actions": ["c"]}],
                ),
                nominal_target(),
            )
        kill = {
            "c": action_payload("checkout", revision=0, via="goto", mode="normal"),
            "k": action_payload(
                "kill_process",
                target_action="c",
                after_ms=0,
                requires=[["c", "started"]],
            ),
        }
        with self.assertRaisesRegex(SpecError, EDEN_ONLY_NEMESIS):
            from_json(
                differential_spec_text(
                    kill, [{"id": "n", "actions": ["k"]}, {"id": "a", "actions": ["c"]}]
                ),
                nominal_target(),
            )
        held = {
            "s": action_payload("readdir_stream", path="tree/wide", pause_after=1),
            "z": action_payload("readdir_resume", stream="s"),
        }
        text = differential_spec_text(
            held, [{"id": "a", "actions": ["s", "z"]}], wide_dir=WIDE_DIR
        )
        with self.assertRaisesRegex(SpecError, EDEN_ONLY_HELD_INPUT):
            from_json(text, nominal_target())

    def test_root_listing_is_rejected(self) -> None:
        raw = json.loads(
            differential_spec_text(
                {"d": action_payload("readdir", path=".", channel="fuse")},
                [{"id": "a", "actions": ["d"]}],
            )
        )
        raw["environment"]["target"]["repository"]["registers"] = ["top.reg"]
        with self.assertRaisesRegex(SpecError, ROOT_LISTING):
            from_json(json.dumps(raw), nominal_target())
        del raw["environment"]["target"]["setup_profile"]["reference_clone"]
        from_json(json.dumps(raw), nominal_target())

    def test_unordered_conflicts_are_rejected(self) -> None:
        concurrent = {
            "w": action_payload("write", path=R0, value_id="w"),
            "r": action_payload("read", path=R0, channel="fuse"),
        }
        actors = [{"id": "a", "actions": ["w"]}, {"id": "b", "actions": ["r"]}]
        with self.assertRaisesRegex(SpecError, "unordered-conflict: 'r' and 'w'"):
            from_json(differential_spec_text(concurrent, actors), nominal_target())
        started = {
            **concurrent,
            "r": {**concurrent["r"], "requires": [["w", "started"]]},
        }
        with self.assertRaisesRegex(SpecError, "unordered-conflict"):
            from_json(differential_spec_text(started, actors), nominal_target())
        finished = {
            **concurrent,
            "r": {**concurrent["r"], "requires": [["w", "finished"]]},
        }
        from_json(differential_spec_text(finished, actors), nominal_target())
        plain = json.loads(nominal_spec_text(concurrent, actors))
        from_json(json.dumps(plain), nominal_target())

    def test_checkout_is_ordered_against_every_action(self) -> None:
        text = self._two_registers(
            differential_spec_text(
                {
                    "c": action_payload(
                        "checkout", revision=0, via="goto", mode="normal"
                    ),
                    "g": action_payload("getattr", path="tree/registers/r1.reg"),
                },
                [{"id": "a", "actions": ["c"]}, {"id": "b", "actions": ["g"]}],
            )
        )
        with self.assertRaisesRegex(SpecError, "unordered-conflict: 'c' and 'g'"):
            from_json(text, nominal_target())

    def test_whole_tree_and_subtree_footprints_conflict(self) -> None:
        """A checkout writes the whole tree, so a concurrent listing or status conflicts.

        Both footprints are subtrees: the checkout's ROOT and the readdir's
        directory or the status's ROOT.
        """
        for observer in (
            action_payload("readdir", path="tree/registers", channel="fuse"),
            action_payload("status", channel="sl"),
        ):
            concurrent = {
                "c": action_payload("checkout", revision=0, via="goto", mode="normal"),
                "o": observer,
            }
            actors = [{"id": "a", "actions": ["c"]}, {"id": "b", "actions": ["o"]}]
            with self.subTest(kind=observer["kind"]):
                with self.assertRaisesRegex(
                    SpecError, "unordered-conflict: 'c' and 'o'"
                ):
                    from_json(
                        differential_spec_text(concurrent, actors), nominal_target()
                    )
                ordered = {
                    **concurrent,
                    "o": {**observer, "requires": [["c", "finished"]]},
                }
                from_json(differential_spec_text(ordered, actors), nominal_target())

    def test_removal_beneath_an_observed_directory_conflicts(self) -> None:
        """`sl rm` may prune the emptied parent, so its stat depends on the order.

        The `mkdir` makes the directory known to the model before the stat.
        """
        sub = "tree/registers/sub"
        raw = json.loads(
            differential_spec_text(
                {
                    "k": action_payload("mkdir", path=sub),
                    "g": action_payload("getattr", path=sub),
                    "m": {
                        **action_payload("sl_rm", path=f"{sub}/s.reg"),
                        "requires": [["k", "finished"]],
                    },
                },
                [{"id": "a", "actions": ["k", "g"]}, {"id": "b", "actions": ["m"]}],
            )
        )
        raw["environment"]["target"]["repository"]["registers"].append(f"{sub}/s.reg")
        with self.assertRaisesRegex(SpecError, "unordered-conflict: 'g' and 'm'"):
            from_json(json.dumps(raw), nominal_target())

    def test_independent_paths_may_run_concurrently(self) -> None:
        text = self._two_registers(
            differential_spec_text(
                {
                    "w": action_payload("write", path=R0, value_id="w"),
                    "r": action_payload(
                        "read", path="tree/registers/r1.reg", channel="fuse"
                    ),
                },
                [{"id": "a", "actions": ["w"]}, {"id": "b", "actions": ["r"]}],
            )
        )
        from_json(text, nominal_target())
