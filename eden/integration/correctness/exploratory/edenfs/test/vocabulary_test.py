# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The EdenFS action vocabulary in RunSpecs.

Every action kind rejects unknown payload fields and wrongly typed values;
checkout, status, and read payloads accept exactly their channel's modes and
fields; churn and register paths are kept apart; descriptor handles live from
their open to their close; the target validates its environment; metadata is an
object; a fault site and input pair that generation never observes still
validates when written by hand; and the worked example validates and
round-trips byte-identically.
"""

from __future__ import annotations

import json
import unittest

from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    from_json as _from_json,
    RunSpec,
    SpecError,
    to_json,
)
from eden.integration.correctness.exploratory.core.test.testing import (
    minimal_spec as _minimal_spec,
)
from eden.integration.correctness.exploratory.edenfs import sites as edenfs_sites
from eden.integration.correctness.exploratory.edenfs.actions import CHURN_FILES
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    VOCAB_TARGET,
    WORKED_EXAMPLE,
)


_MINIMAL_ACTIONS = {
    "background_read": {"root": "tree", "seconds": 0.1},
    "unload_inodes": {"path": "tree/registers"},
    "sleep": {"seconds": 1.0},
    "fault_install": {
        "fault_id": "F",
        "target_action": "target",
        "site": "checkout-begin",
        "behavior": "error",
    },
    "fault_await": {"fault_id": "F"},
    "fault_remove": {"fault_id": "F"},
    "kill_process": {"target_action": "target", "after_ms": 1},
    "blocked_faults": {},
    "write": {"path": "tree/registers/r0.reg", "value_id": "value"},
    "read": {"path": "tree/registers/r0.reg", "channel": "fuse"},
    "getattr": {"path": "tree/registers/r0.reg"},
    "status": {"channel": "sl"},
    "checkout": {"revision": 2, "via": "goto", "mode": "normal"},
    "readdir": {"path": "tree/registers", "channel": "fuse"},
    "readdir_stream": {"path": "tree/wide", "pause_after": 1},
    "readdir_resume": {"stream": "d0"},
    "open": {"path": "tree/registers/r0.reg"},
    "read_fd": {"fd": "o0"},
    "close": {"fd": "o0"},
    "rename": {"source": "tree/registers/r0.reg", "destination": "tree/registers/x"},
    "unlink": {"path": "tree/registers/r0.reg"},
    "mkdir": {"path": "tree/registers/d"},
    "chmod": {"path": "tree/registers/r0.reg", "mode": 0o755},
    "sl_rm": {"path": "tree/registers/r0.reg"},
    "sl_mv": {"source": "tree/registers/r0.reg", "destination": "tree/registers/x"},
    "manifest_presence": {"from_revision": 0, "to_revision": 1},
    "journal_window": {},
    "status_counts": {},
}


def _minimal_action(kind: str) -> dict:
    return {"kind": kind, "deadline": "operation", **_MINIMAL_ACTIONS[kind]}


def from_json(text: str) -> RunSpec:
    return _from_json(text, VOCAB_TARGET)


def _environment_target() -> dict:
    return {
        "repository": {
            "file_count": 1,
            "commit_count": 4,
            "registers": ["tree/registers/r0.reg"],
        },
        "channel": "fuse",
        "case_sensitive": True,
        "fixture_source": "backing",
        "setup_profile": {"materialize": False, "scatter_dirty": 0},
    }


def _single_action_spec(action: dict) -> str:
    actions = {"a": action}
    actors = [{"id": "actor", "actions": ["a"]}]
    if action["kind"] == "fault_install":
        actions["target"] = {
            **_minimal_action("checkout"),
            "requires": [["a", "finished"]],
        }
        actions["remove"] = {
            **_minimal_action("fault_remove"),
            "fault_id": action["fault_id"],
            "requires": [["target", "finished"]],
        }
        actors[0]["actions"].append("remove")
        actors.append({"id": "client", "actions": ["target"]})
    return _minimal_spec(
        actions,
        actors,
        target_name=VOCAB_TARGET.name,
        environment_target=_environment_target(),
    )


def _worked_dict() -> dict:
    return json.loads(WORKED_EXAMPLE)


class VocabularyTest(unittest.TestCase):
    def test_hand_written_unobservable_pair_still_validates(self) -> None:
        """Observed generation misses do not weaken the handwritten vocabulary."""
        payloads = {
            ("tree-checkout", "checkout"): {
                "revision": 2,
                "via": "thrift",
                "mode": "normal",
            },
            ("wait-for-pending-writes", "read"): {
                "path": "tree/registers/r0.reg",
                "channel": "thrift",
                "sync_timeout_seconds": 60,
            },
            ("fetch-tree", "status"): {"channel": "thrift", "revision": 3},
            ("status-cache-fill", "status"): {"channel": "sl"},
        }
        for (site, kind), payload in payloads.items():
            with self.subTest(site=site, kind=kind):
                self.assertIn((site, kind), edenfs_sites.UNOBSERVABLE_PAIRS)
                install = {
                    **_minimal_action("fault_install"),
                    "site": site,
                    "behavior": "block",
                }
                if site == "tree-checkout":
                    install["key"] = "tree/a1"
                raw = _minimal_spec(
                    {
                        "install": install,
                        "target": {
                            "kind": kind,
                            "deadline": "run",
                            "requires": [["install", "finished"]],
                            **payload,
                        },
                        "await": {
                            **_minimal_action("fault_await"),
                            "requires": [["target", "started"]],
                        },
                        "remove": _minimal_action("fault_remove"),
                    },
                    [
                        {"id": "nemesis", "actions": ["install", "await", "remove"]},
                        {"id": "input", "actions": ["target"]},
                    ],
                    target_name=VOCAB_TARGET.name,
                    environment_target=_environment_target(),
                )
                self.assertEqual(kind, from_json(raw).action("target").kind)

    def test_checkout_mode_validates(self) -> None:
        """Checkout mode is required and is normal or force on either channel."""

        def checkout(**fields: object) -> Action:
            raw = _single_action_spec({**_minimal_action("checkout"), **fields})
            return from_json(raw).action("a")

        without_mode = _minimal_action("checkout")
        del without_mode["mode"]
        with self.assertRaisesRegex(SpecError, r"missing fields: \['mode'\]"):
            from_json(_single_action_spec(without_mode))
        self.assertEqual("normal", checkout(mode="normal").payload["mode"])
        for via in ("thrift", "goto"):
            with self.subTest(via=via):
                forced = checkout(via=via, mode="force")
                self.assertEqual("force", forced.payload["mode"])
        with self.assertRaisesRegex(SpecError, "checkout mode"):
            checkout(mode="clean")

    def test_fd_handle_lifetime(self) -> None:
        """read_fd follows its finished open, close follows every read_fd, closed fds are dead."""

        def spec(actions: dict, *actors: tuple[str, list[str]]) -> str:
            return _minimal_spec(
                {k: {"deadline": "operation", **v} for k, v in actions.items()},
                [{"id": name, "actions": ids} for name, ids in actors],
                target_name=VOCAB_TARGET.name,
                environment_target=_environment_target(),
            )

        o0 = {"kind": "open", "path": "tree/registers/r0.reg"}
        f0 = {"kind": "read_fd", "fd": "o0"}
        x0 = {"kind": "close", "fd": "o0"}
        w0 = _minimal_action("write")
        good = from_json(
            spec({"o0": o0, "f0": f0, "x0": x0}, ("a", ["o0", "f0", "x0"]))
        )
        self.assertEqual("o0", good.action("f0").payload["fd"])
        gated = {**f0, "requires": [["o0", "finished"]]}
        from_json(spec({"o0": o0, "f0": gated}, ("a", ["o0"]), ("b", ["f0"])))
        cases = (
            (
                "read before open",
                {"o0": o0, "f0": f0},
                (("a", ["f0", "o0"]),),
                "finished",
            ),
            (
                "read on other actor",
                {"o0": o0, "f0": f0},
                (("a", ["o0"]), ("b", ["f0"])),
                "finished",
            ),
            (
                "unknown fd",
                {"o0": o0, "f0": {**f0, "fd": "zz"}},
                (("a", ["o0", "f0"]),),
                "open action",
            ),
            (
                "fd is not an open",
                {"w0": w0, "f0": {**f0, "fd": "w0"}},
                (("a", ["w0", "f0"]),),
                "open action",
            ),
            (
                "close before read",
                {"o0": o0, "f0": f0, "x0": x0},
                (("a", ["o0", "x0", "f0"]),),
                "after close",
            ),
            (
                "close then read elsewhere",
                {"o0": o0, "x0": x0, "f0": {**f0, "requires": [["x0", "finished"]]}},
                (("a", ["o0", "x0"]), ("b", ["f0"])),
                "after close",
            ),
            (
                "close unordered with read",
                {"o0": o0, "f0": f0, "x0": x0},
                (("a", ["o0", "f0"]), ("b", ["x0"])),
                "must follow read_fd",
            ),
            (
                "second close",
                {"o0": o0, "x0": x0, "x1": x0},
                (("a", ["o0", "x0", "x1"]),),
                "one close",
            ),
            (
                "open of a churn path",
                {"o0": {**o0, "path": CHURN_FILES[0]}},
                (("a", ["o0"]),),
                "status-only",
            ),
        )
        for name, actions, actors, substring in cases:
            with self.subTest(case=name):
                with self.assertRaisesRegex(SpecError, substring):
                    from_json(spec(actions, *actors))

    def test_environment_target_payload_validated_by_target(self) -> None:
        raw = _worked_dict()
        raw["environment"]["target"]["unknown"] = True
        with self.assertRaisesRegex(SpecError, "unknown"):
            _from_json(json.dumps(raw), VOCAB_TARGET)

        raw = _worked_dict()
        del raw["environment"]["target"]["repository"]
        with self.assertRaisesRegex(SpecError, "missing"):
            _from_json(json.dumps(raw), VOCAB_TARGET)

    def test_unknown_target_rejects(self) -> None:
        raw = _worked_dict()
        raw["target"] = "unknown"
        with self.assertRaisesRegex(SpecError, "target"):
            _from_json(json.dumps(raw), VOCAB_TARGET)

    def test_worked_example_round_trips_byte_identical(self) -> None:
        encoded = to_json(from_json(WORKED_EXAMPLE))
        self.assertEqual(encoded, to_json(from_json(encoded)))

    def test_metadata_round_trips_through_to_json(self) -> None:
        """A deck's metadata object is stored and emitted unchanged."""
        raw = _worked_dict()
        raw["metadata"] = {"expected": "reject", "added": "2026-09-22"}
        spec = from_json(json.dumps(raw))
        self.assertEqual(raw["metadata"], spec.metadata)
        self.assertEqual(raw["metadata"], json.loads(to_json(spec))["metadata"])
        self.assertEqual(spec, from_json(to_json(spec)))

    def test_absent_metadata_stays_absent(self) -> None:
        spec = from_json(WORKED_EXAMPLE)
        self.assertEqual({}, spec.metadata)
        self.assertNotIn("metadata", json.loads(to_json(spec)))

    def test_metadata_must_be_an_object(self) -> None:
        raw = _worked_dict()
        raw["metadata"] = ["note"]
        with self.assertRaisesRegex(SpecError, "metadata must be dict"):
            from_json(json.dumps(raw))

    def test_every_kind_rejects_unknown_payload_field(self) -> None:
        for kind in VOCAB_TARGET.actions:
            with self.subTest(kind=kind):
                action = _minimal_action(kind)
                action["zzz"] = 1
                with self.assertRaisesRegex(SpecError, "zzz"):
                    from_json(_single_action_spec(action))

    def test_every_kind_rejects_wrong_type(self) -> None:
        for kind, action_def in VOCAB_TARGET.actions.items():
            for field, expected_type in action_def.types.items():
                with self.subTest(kind=kind, field=field):
                    action = _minimal_action(kind)
                    action[field] = False if expected_type is int else 1
                    with self.assertRaisesRegex(SpecError, field):
                        from_json(_single_action_spec(action))

    def test_worked_example_validates(self) -> None:
        spec = from_json(WORKED_EXAMPLE)
        self.assertEqual(spec.seed, 6201)
        self.assertEqual(len(spec.actions), 8)
        self.assertEqual(spec.action("c0").deadline, "run")

    def test_round_trip_is_stable(self) -> None:
        spec = from_json(WORKED_EXAMPLE)
        restored = from_json(to_json(spec))
        self.assertEqual(spec, restored)
        self.assertEqual(to_json(spec), to_json(restored))

    def test_wrong_schema_rejects(self) -> None:
        for mutate in ("schema", "schema_version"):
            with self.subTest(mutate=mutate):
                raw = _worked_dict()
                raw[mutate] = "bogus"
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_unknown_fields_reject(self) -> None:
        cases = [
            (lambda raw: raw.update({"future": 1}), "top level"),
            (
                lambda raw: raw["actions"]["r1"].update({"mode": 0o644}),
                "action level",
            ),
            (
                lambda raw: raw["actions"].update(
                    {"z9": {"kind": "teleport", "deadline": "operation"}}
                ),
                "unknown kind",
            ),
        ]
        for mutate, _label in cases:
            with self.subTest():
                raw = _worked_dict()
                mutate(raw)
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_missing_fields_reject(self) -> None:
        raw = _worked_dict()
        del raw["actors"]
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))
        raw = _worked_dict()
        del raw["actions"]["w1"]["value_id"]
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))

    def test_register_must_be_declared(self) -> None:
        raw = _worked_dict()
        raw["actions"]["r1"]["path"] = "tree/other.reg"
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))

    def test_churn_path_write_validates_and_round_trips(self) -> None:
        raw = _worked_dict()
        raw["actions"]["u1"] = {
            "kind": "write",
            "path": CHURN_FILES[0],
            "value_id": "u1",
            "deadline": "operation",
        }
        raw["actors"][2]["actions"].append("u1")
        spec = from_json(json.dumps(raw))
        self.assertEqual(spec.action("u1").payload["path"], CHURN_FILES[0])
        self.assertEqual(spec, from_json(to_json(spec)))

    def test_read_rejects_churn_path(self) -> None:
        raw = _worked_dict()
        raw["actions"]["u1"] = {
            "kind": "read",
            "path": CHURN_FILES[0],
            "channel": "fuse",
            "deadline": "operation",
        }
        raw["actors"][2]["actions"].append("u1")
        with self.assertRaisesRegex(SpecError, "status-only"):
            from_json(json.dumps(raw))

    def test_write_rejects_missing_payload(self) -> None:
        raw = _worked_dict()
        raw["actions"]["u1"] = {
            "kind": "write",
            "path": CHURN_FILES[0],
            "deadline": "operation",
        }
        raw["actors"][2]["actions"].append("u1")
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))

    def test_write_accepts_churn_path(self) -> None:
        raw = _worked_dict()
        raw["actions"]["w1"]["path"] = CHURN_FILES[0]
        self.assertEqual(
            CHURN_FILES[0], from_json(json.dumps(raw)).action("w1").payload["path"]
        )

    def test_registers_reject_churn_paths(self) -> None:
        raw = _worked_dict()
        raw["environment"]["target"]["repository"]["registers"].append(CHURN_FILES[1])
        with self.assertRaisesRegex(SpecError, "status-only"):
            from_json(json.dumps(raw))

    def test_escaping_register_rejects(self) -> None:
        # `mount_path / register` escapes the mount on absolute paths
        # and dotdot, so both reject even when declared.
        with self.subTest("absolute declared register"):
            raw = _worked_dict()
            raw["environment"]["target"]["repository"]["registers"].append("/abs.reg")
            with self.assertRaisesRegex(SpecError, "escapes the mount"):
                from_json(json.dumps(raw))
        with self.subTest("absolute action register"):
            raw = _worked_dict()
            raw["environment"]["target"]["repository"]["registers"].append("/abs.reg")
            raw["actions"]["r1"]["path"] = "/abs.reg"
            with self.assertRaisesRegex(SpecError, "escapes the mount"):
                from_json(json.dumps(raw))
        with self.subTest("dotdot action register"):
            raw = _worked_dict()
            raw["environment"]["target"]["repository"]["registers"].append(
                "tree/../../evil.reg"
            )
            raw["actions"]["r1"]["path"] = "tree/../../evil.reg"
            with self.assertRaisesRegex(SpecError, "escapes the mount"):
                from_json(json.dumps(raw))

    def test_unsupported_environment_choices_reject(self) -> None:
        cases = [
            ("channel", "thrift"),
            ("fixture_source", "overlay"),
            ("case_sensitive", "sometimes"),
        ]
        for field, value in cases:
            with self.subTest(field=field, value=value):
                raw = _worked_dict()
                raw["environment"]["target"][field] = value
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_payload_rules(self) -> None:
        cases = [
            ("t1", {"sync_timeout_seconds": 7}, "sync_timeout"),
            ("c0", {"revision": 4}, "revision"),
            ("c0", {"revision": -1}, "revision"),
            ("f1", {"site": "fuse-read"}, "site"),
        ]
        for action_id, override, _label in cases:
            with self.subTest(action=action_id, override=override):
                raw = _worked_dict()
                raw["actions"][action_id].update(override)
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_unsupported_checker_rejects(self) -> None:
        raw = _worked_dict()
        raw["checker"] = {}
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))

    def test_payload_type_rules(self) -> None:
        # bool is not int (False == 0 must not pass as a sync timeout)
        # and every string payload field rejects non-strings at parse.
        cases = [
            ("w1", {"path": 5}, "path"),
            ("w1", {"value_id": 5}, "value_id"),
            ("t1", {"sync_timeout_seconds": False}, "sync_timeout"),
            ("c0", {"revision": True}, "revision"),
            ("f1", {"fault_id": 5}, "fault_id"),
            ("f1", {"target_action": 5}, "target_action"),
            ("f1", {"site": 5}, "site"),
            ("h1", {"fault_id": 5}, "fault_id"),
        ]
        for action_id, override, _label in cases:
            with self.subTest(action=action_id, override=override):
                raw = _worked_dict()
                raw["actions"][action_id].update(override)
                with self.assertRaises(SpecError):
                    from_json(json.dumps(raw))

    def test_bool_is_not_an_int(self) -> None:
        raw = _worked_dict()
        raw["seed"] = True
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))

    def test_spec_action_lookup(self) -> None:
        spec: RunSpec = from_json(WORKED_EXAMPLE)
        self.assertEqual(spec.action("t1").kind, "read")
        with self.assertRaises(SpecError):
            spec.action("missing")

    def test_status_parses_with_channel(self) -> None:
        raw = _worked_dict()
        raw["actions"]["s1"] = {
            "kind": "status",
            "deadline": "operation",
            "channel": "sl",
        }
        raw["actors"].append({"id": "c", "actions": ["s1"]})
        spec = from_json(json.dumps(raw))
        self.assertEqual(spec.action("s1").kind, "status")
        self.assertEqual(spec.action("s1").payload, {"channel": "sl"})
        restored = from_json(to_json(spec))
        self.assertEqual(spec, restored)

    def test_status_via_thrift_revision_is_optional_and_bounded(self) -> None:
        for revision in (None, 0, 3):
            with self.subTest(revision=revision):
                action: dict[str, object] = {
                    "kind": "status",
                    "deadline": "operation",
                    "channel": "thrift",
                }
                if revision is not None:
                    action["revision"] = revision
                spec = from_json(_single_action_spec(action))
                self.assertEqual(
                    spec.action("a").payload,
                    {
                        key: value
                        for key, value in action.items()
                        if key not in {"kind", "deadline"}
                    },
                )

        for revision in (-1, 4, False, "1"):
            with self.subTest(revision=revision):
                with self.assertRaises(SpecError):
                    from_json(
                        _single_action_spec(
                            {
                                "kind": "status",
                                "deadline": "operation",
                                "channel": "thrift",
                                "revision": revision,
                            }
                        )
                    )

    def test_status_via_thrift_rejects_cancellation_payloads(self) -> None:
        for field in ("cancelled_status", "kill_delay_ms"):
            with self.subTest(field=field):
                action = {
                    "kind": "status",
                    "deadline": "operation",
                    "channel": "thrift",
                    field: False if field == "cancelled_status" else 1,
                }
                with self.assertRaisesRegex(SpecError, field):
                    from_json(_single_action_spec(action))

    def test_status_rejects_unknown_payload(self) -> None:
        raw = _worked_dict()
        raw["actions"]["s1"] = {
            "kind": "status",
            "deadline": "operation",
            "channel": "sl",
            "path": "tree/registers/r0.reg",
        }
        raw["actors"].append({"id": "c", "actions": ["s1"]})
        with self.assertRaises(SpecError):
            from_json(json.dumps(raw))
