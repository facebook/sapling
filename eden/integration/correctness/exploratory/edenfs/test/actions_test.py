# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The EdenFS action declarations: each kind's payload fields, leases and budgets,
roles, bindings and grouping, footprints, the register layout a run may claim,
run-level validation, and write value identities.
"""

from __future__ import annotations

import itertools
import json
import random
import re
import unittest
from types import SimpleNamespace
from typing import Any

from eden.integration.correctness.exploratory.core.actions import (
    compose_action_defs,
    CORE_ACTION_DEFS,
)
from eden.integration.correctness.exploratory.core.runspec import (
    from_json,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.core.target import (
    Binding,
    Grouping,
    MUTATING_ROLES,
    Role,
)
from eden.integration.correctness.exploratory.edenfs.actions import (
    ACTION_DEFS,
    CHURN_FILES,
    descriptor_paths,
    EDENFS_ACTION_DEFS,
    MAX_COMMIT_COUNT,
    WALK_DEADLOCK_TASK,
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


def _wide_spec(actions: dict[str, dict[str, Any]], actors: list[dict[str, Any]]) -> str:
    raw = json.loads(nominal_spec_text(actions, actors))
    raw["environment"]["target"]["setup_profile"]["wide_dir"] = WIDE_DIR
    return json.dumps(raw)


class EdenFSActionsTest(unittest.TestCase):
    def test_an_overlapping_checkout_walk_is_refused_waiting_on_its_task(
        self,
    ) -> None:
        actions = {
            "c": {
                "kind": "checkout",
                "deadline": "run",
                "revision": 1,
                "via": "thrift",
                "mode": "force",
            },
            "u": {"kind": "unlink", "deadline": "operation", "path": R0},
        }
        actors = [{"id": "co", "actions": ["c"]}, {"id": "un", "actions": ["u"]}]
        with self.assertRaisesRegex(SpecError, "T293953113") as refused:
            from_json(nominal_spec_text(actions, actors), nominal_target())
        self.assertEqual(WALK_DEADLOCK_TASK, refused.exception.waiting_on)

    def test_background_read_is_nemesis_lease(self) -> None:
        definition = EDENFS_ACTION_DEFS["background_read"]
        self.assertEqual("nemesis", definition.axis)
        self.assertTrue(definition.spawns_process)
        self.assertFalse(definition.modeled)
        self.assertEqual(frozenset({"root", "seconds"}), definition.fields)
        self.assertEqual(definition.fields, definition.required)
        self.assertEqual(
            {"root": "tree", "seconds": 2.0},
            definition.sample(random.Random(0), {}),
        )
        raw = json.loads(
            one_action_spec(
                {
                    "kind": "background_read",
                    "deadline": "operation",
                    "root": "tree",
                    "seconds": 20,
                }
            )
        )
        raw["budgets"].update(operation_deadline_seconds=30, runner_deadline_seconds=60)
        from_json(json.dumps(raw), nominal_target())

    def test_background_read_requires_budget_or_reachable_kill(self) -> None:
        reader = {
            "kind": "background_read",
            "deadline": "operation",
            "root": "tree",
            "seconds": 0.1,
        }
        from_json(one_action_spec(reader), nominal_target())
        for field, values in {
            "root": ("", "/tree", "../tree"),
            "seconds": (0.0, -1.0, True, "1"),
        }.items():
            for value in values:
                with self.subTest(field=field, value=value):
                    with self.assertRaisesRegex(SpecError, field):
                        from_json(
                            one_action_spec({**reader, field: value}), nominal_target()
                        )
        for value in (float("inf"), float("nan")):
            with self.subTest(field="seconds", value=value):
                with self.assertRaisesRegex(SpecError, "is not a JSON number"):
                    from_json(
                        one_action_spec({**reader, "seconds": value}), nominal_target()
                    )
        reader["seconds"] = 2.0
        with self.assertRaisesRegex(SpecError, "lease|budget"):
            from_json(one_action_spec(reader), nominal_target())
        actions: dict[str, dict[str, Any]] = {
            "r": reader,
            "k": {
                "kind": "kill_process",
                "deadline": "operation",
                "target_action": "r",
                "after_ms": 0,
                "requires": [["r", "started"]],
            },
        }
        actors = [
            {"id": "reader", "actions": ["r"]},
            {"id": "killer", "actions": ["k"]},
        ]
        from_json(nominal_spec_text(actions, actors), nominal_target())
        for dependencies in ([["r", "finished"]], [["r", "started"], ["m", "started"]]):
            actions["m"] = {
                "kind": "sleep",
                "deadline": "operation",
                "seconds": 0.0,
                "requires": [["r", "finished"]],
            }
            actions["k"]["requires"] = [["r", "started"], *dependencies]
            with self.assertRaisesRegex(SpecError, "lease|kill"):
                from_json(
                    nominal_spec_text(
                        actions, actors + [{"id": "middle", "actions": ["m"]}]
                    ),
                    nominal_target(),
                )
        del actions["m"]
        actions["k"]["requires"] = [["r", "started"]]
        with self.assertRaisesRegex(SpecError, "lease|kill"):
            from_json(
                nominal_spec_text(actions, [{"id": "both", "actions": ["r", "k"]}]),
                nominal_target(),
            )
        actions["k"]["after_ms"] = 2000
        with self.assertRaisesRegex(SpecError, "lease|budget"):
            from_json(nominal_spec_text(actions, actors), nominal_target())
        raw = json.loads(one_action_spec({**reader, "seconds": 0.5}))
        raw["budgets"]["runner_deadline_seconds"] = 0.25
        with self.assertRaisesRegex(SpecError, "lease|budget"):
            from_json(json.dumps(raw), nominal_target())

    def test_readdir_stream_is_input_lease(self) -> None:
        """A stream is a modeled input holding one child; exactly one gated resume ends it."""
        stream = EDENFS_ACTION_DEFS["readdir_stream"]
        self.assertEqual("input", stream.axis)
        self.assertTrue(stream.spawns_process)
        self.assertTrue(stream.modeled)
        self.assertEqual(frozenset({"path", "pause_after"}), stream.fields)
        self.assertEqual(stream.fields, stream.required)
        self.assertIsNone(stream.sample(random.Random(0), {}))
        resume = EDENFS_ACTION_DEFS["readdir_resume"]
        self.assertEqual(frozenset({"stream"}), resume.fields)
        self.assertEqual(resume.fields, resume.required)
        stream_action = {
            "kind": "readdir_stream",
            "deadline": "operation",
            "path": "tree/wide",
            "pause_after": 1000,
        }
        resume_action = {
            "kind": "readdir_resume",
            "deadline": "operation",
            "stream": "d0",
            "requires": [["d0", "finished"]],
        }
        two_actors = [{"id": "s", "actions": ["d0"]}, {"id": "r", "actions": ["d1"]}]
        from_json(
            _wide_spec({"d0": stream_action, "d1": resume_action}, two_actors),
            nominal_target(),
        )
        started = {**resume_action, "requires": [["d0", "started"]]}
        with self.assertRaisesRegex(SpecError, "finished"):
            from_json(
                _wide_spec({"d0": stream_action, "d1": started}, two_actors),
                nominal_target(),
            )
        ungated = {
            key: value for key, value in resume_action.items() if key != "requires"
        }
        from_json(
            _wide_spec(
                {"d0": stream_action, "d1": ungated},
                [{"id": "s", "actions": ["d0", "d1"]}],
            ),
            nominal_target(),
        )
        with self.assertRaisesRegex(SpecError, "resume"):
            from_json(
                _wide_spec({"d0": stream_action, "d1": ungated}, two_actors),
                nominal_target(),
            )
        with self.assertRaisesRegex(SpecError, "resume"):
            from_json(
                _wide_spec({"d0": stream_action}, [{"id": "s", "actions": ["d0"]}]),
                nominal_target(),
            )
        with self.assertRaisesRegex(SpecError, "resume"):
            from_json(
                _wide_spec(
                    {"d0": stream_action, "d1": resume_action, "d2": resume_action},
                    [
                        {"id": "s", "actions": ["d0"]},
                        {"id": "r", "actions": ["d1", "d2"]},
                    ],
                ),
                nominal_target(),
            )
        with self.assertRaisesRegex(SpecError, "stream"):
            from_json(one_action_spec({**ungated, "stream": "nope"}), nominal_target())
        for field, value in (("path", "/tree/wide"), ("path", ""), ("pause_after", 0)):
            with self.subTest(field=field, value=value):
                with self.assertRaisesRegex(SpecError, field):
                    from_json(
                        _wide_spec(
                            {"d0": {**stream_action, field: value}, "d1": ungated},
                            [{"id": "s", "actions": ["d0", "d1"]}],
                        ),
                        nominal_target(),
                    )

    def test_readdir_stream_path_is_the_wide_dir(self) -> None:
        """A stream may list only the generated wide directory, so it needs one."""
        stream = {"kind": "readdir_stream", "deadline": "operation", "pause_after": 2}
        resume = {"kind": "readdir_resume", "deadline": "operation", "stream": "d0"}
        actors = [{"id": "s", "actions": ["d0", "d1"]}]
        from_json(
            _wide_spec({"d0": {**stream, "path": "tree/wide"}, "d1": resume}, actors),
            nominal_target(),
        )
        for path in ("tree", "tree/registers", "tree/wide/deeper"):
            with self.subTest(path=path):
                with self.assertRaisesRegex(SpecError, "wide_dir.*tree/wide"):
                    from_json(
                        _wide_spec(
                            {"d0": {**stream, "path": path}, "d1": resume}, actors
                        ),
                        nominal_target(),
                    )
        with self.assertRaisesRegex(SpecError, "wide_dir"):
            from_json(
                nominal_spec_text(
                    {"d0": {**stream, "path": "tree/wide"}, "d1": resume}, actors
                ),
                nominal_target(),
            )

    def test_existence_kinds_are_modeled_inputs(self) -> None:
        """rename, unlink, and mkdir are modeled inputs over paths the model knows.

        Their samplers decline a forced kind (baseline, fault or kill
        target, repeated-producer stage); composed inputs may sample them.

        The validators keep every operation inside the modeled tree: a
        source or unlinked path is a register, and a destination or new
        directory sits in a directory the model already has (register
        parents, the wide dir, or an earlier mkdir).
        """
        expected = {
            "rename": frozenset({"source", "destination"}),
            "unlink": frozenset({"path"}),
            "mkdir": frozenset({"path"}),
        }
        for kind, fields in expected.items():
            with self.subTest(kind=kind):
                definition = EDENFS_ACTION_DEFS[kind]
                self.assertEqual(("input", True), (definition.axis, definition.modeled))
                self.assertEqual(
                    (fields, fields), (definition.fields, definition.required)
                )
                self.assertIsNone(
                    definition.sample(random.Random(0), {"forced_kind": True})
                )
        register = "tree/registers/r0.reg"
        rename = {
            "kind": "rename",
            "deadline": "operation",
            "source": register,
            "destination": "tree/registers/moved.reg",
        }
        from_json(one_action_spec(rename), nominal_target())
        from_json(
            one_action_spec({**rename, "destination": "tree/registers/r1.reg"}),
            nominal_target(),
        )
        for field, value, message in (
            ("source", CHURN_FILES[0], "registers"),
            ("source", "tree/nope.reg", "registers"),
            ("destination", "tree/elsewhere/x.reg", "parent"),
            ("destination", "/tree/registers/x.reg", "mount-relative"),
            ("destination", register, "itself"),
        ):
            with self.subTest(field=field, value=value):
                with self.assertRaisesRegex(SpecError, message):
                    from_json(
                        one_action_spec({**rename, field: value}), nominal_target()
                    )
        unlink = {"kind": "unlink", "deadline": "operation", "path": register}
        from_json(one_action_spec(unlink), nominal_target())
        for path in (CHURN_FILES[0], "tree/nope.reg"):
            with self.assertRaisesRegex(SpecError, "status-only|registers"):
                from_json(one_action_spec({**unlink, "path": path}), nominal_target())
        mkdir = {"kind": "mkdir", "deadline": "operation", "path": "tree/registers/d"}
        from_json(one_action_spec(mkdir), nominal_target())
        from_json(
            _wide_spec(
                {"a": {**mkdir, "path": "tree/wide/d"}}, [{"id": "x", "actions": ["a"]}]
            ),
            nominal_target(),
        )
        with self.assertRaisesRegex(SpecError, "parent"):
            from_json(
                one_action_spec({**mkdir, "path": "tree/wide/d"}), nominal_target()
            )
        with self.assertRaisesRegex(SpecError, "modeled file"):
            from_json(one_action_spec({**mkdir, "path": register}), nominal_target())
        nested = {
            "m": mkdir,
            "n": {**rename, "destination": "tree/registers/d/f.reg"},
        }
        from_json(
            nominal_spec_text(nested, [{"id": "x", "actions": ["m", "n"]}]),
            nominal_target(),
        )
        unordered = [{"id": "x", "actions": ["m"]}, {"id": "y", "actions": ["n"]}]
        with self.assertRaisesRegex(SpecError, "parent"):
            from_json(nominal_spec_text(nested, unordered), nominal_target())

    def test_dirstate_kinds_are_process_spawning_modeled_inputs(self) -> None:
        """sl_rm and sl_mv act through an sl subprocess on paths the model knows.

        Like unlink and rename they are hand-spec inputs: a removed or moved
        source is a register, a destination sits in a known directory, and
        an sl_mv destination is model-known to later observers.
        """
        expected = {
            "sl_rm": frozenset({"path"}),
            "sl_mv": frozenset({"source", "destination"}),
        }
        for kind, fields in expected.items():
            with self.subTest(kind=kind):
                definition = EDENFS_ACTION_DEFS[kind]
                self.assertEqual(
                    ("input", True, True),
                    (definition.axis, definition.modeled, definition.spawns_process),
                )
                self.assertEqual(
                    (fields, fields), (definition.fields, definition.required)
                )
                self.assertIsNone(
                    definition.sample(random.Random(0), {"forced_kind": True})
                )
        remove = {"kind": "sl_rm", "deadline": "operation", "path": R0}
        from_json(one_action_spec(remove), nominal_target())
        for path in (CHURN_FILES[0], "tree/nope.reg"):
            with self.assertRaisesRegex(SpecError, "status-only|registers"):
                from_json(one_action_spec({**remove, "path": path}), nominal_target())
        moved = "tree/registers/moved.reg"
        move = {"kind": "sl_mv", "deadline": "operation", "source": R0}
        move["destination"] = moved
        from_json(one_action_spec(move), nominal_target())
        for field, value, message in (
            ("source", "tree/nope.reg", "registers"),
            ("destination", "tree/elsewhere/x.reg", "parent"),
            ("destination", R0, "itself"),
        ):
            with self.subTest(field=field, value=value):
                with self.assertRaisesRegex(SpecError, message):
                    from_json(one_action_spec({**move, field: value}), nominal_target())
        read = {
            "kind": "read",
            "deadline": "operation",
            "path": moved,
            "channel": "fuse",
        }
        actions = {"n": move, "r": read}
        from_json(
            nominal_spec_text(actions, [{"id": "x", "actions": ["n", "r"]}]),
            nominal_target(),
        )
        with self.assertRaisesRegex(SpecError, "not known"):
            from_json(
                nominal_spec_text(actions, [{"id": "x", "actions": ["r", "n"]}]),
                nominal_target(),
            )
        made = {"kind": "mkdir", "deadline": "operation", "path": moved}
        for order in (["m", "n"], ["n", "m"]):
            with self.subTest(order=order):
                with self.assertRaisesRegex(SpecError, "may be a directory"):
                    from_json(
                        nominal_spec_text(
                            {"m": made, "n": move}, [{"id": "x", "actions": order}]
                        ),
                        nominal_target(),
                    )

    def test_chmod_names_a_known_path_and_samples_two_modes(self) -> None:
        """chmod is a modeled input over a model-known path no mkdir names,
        with mode 0644 or 0755.

        Modeled: after a restart the daemon serves the chmod on
        harness-authored state, so a failure outside every nemesis window
        is the daemon's unless the model left the path absent.
        """
        definition = EDENFS_ACTION_DEFS["chmod"]
        self.assertEqual(("input", True), (definition.axis, definition.modeled))
        register = "tree/registers/r0.reg"
        chmod: dict[str, Any] = {"kind": "chmod", "deadline": "operation"}
        chmod.update({"path": register, "mode": 0o755})
        from_json(one_action_spec(chmod), nominal_target())
        made = {"kind": "mkdir", "deadline": "operation", "path": "tree/registers/d"}
        on_made = {"m": made, "h": {**chmod, "path": made["path"]}}
        with self.assertRaisesRegex(SpecError, "is a mkdir path"):
            from_json(
                nominal_spec_text(on_made, [{"id": "x", "actions": ["m", "h"]}]),
                nominal_target(),
            )
        invalid = (
            ("mode", 0o600, "mode"),
            ("path", made["path"], "not known"),
            ("path", CHURN_FILES[0], "status-only"),
        )
        for field, value, message in invalid:
            with self.subTest(field=field, value=value):
                with self.assertRaisesRegex(SpecError, message):
                    from_json(
                        one_action_spec({**chmod, field: value}), nominal_target()
                    )
        retyped = json.loads(one_action_spec(chmod))
        retyped["environment"]["target"]["repository"]["type_changes"] = [
            {"path": register, "at_revision": 1}
        ]
        with self.assertRaisesRegex(SpecError, "changes type"):
            from_json(json.dumps(retyped), nominal_target())
        environment = json.loads(one_action_spec(chmod))["environment"]["target"]
        context = {"environment": environment, "action_id": "h", "forced_kind": True}
        samples = [
            sample
            for seed in range(64)
            if (sample := definition.sample(random.Random(seed), context)) is not None
        ]
        modes, paths = {s["mode"] for s in samples}, {s["path"] for s in samples}
        self.assertEqual(({0o644, 0o755}, {register}), (modes, paths))

    def test_observers_name_model_known_paths(self) -> None:
        """read and getattr may name any path the model knows when they start.

        Registers are known from setup; a mkdir path and a rename's
        destination or source are known once the producing action must
        finish first, by actor order or a finished requires edge. A path
        only a later or unordered action produces is a SpecError.
        """
        register = "tree/registers/r0.reg"
        moved = "tree/registers/moved.reg"
        rename = {
            "kind": "rename",
            "deadline": "operation",
            "source": register,
            "destination": moved,
        }
        read = {"kind": "read", "deadline": "operation", "channel": "fuse"}
        stat = {"kind": "getattr", "deadline": "operation"}
        made = {"kind": "mkdir", "deadline": "operation", "path": "tree/registers/d"}
        same_actor = [{"id": "x", "actions": ["n", "r"]}]
        two_actors = [{"id": "x", "actions": ["n"]}, {"id": "y", "actions": ["r"]}]
        gated = {**read, "path": moved, "requires": [["n", "finished"]]}
        valid = (
            (
                "destination-after-rename",
                {"n": rename, "r": {**read, "path": moved}},
                same_actor,
            ),
            ("destination-gated", {"n": rename, "r": gated}, two_actors),
            (
                "source-after-rename",
                {"n": rename, "r": {**stat, "path": register}},
                same_actor,
            ),
            (
                "mkdir-path",
                {"n": made, "r": {**stat, "path": "tree/registers/d"}},
                same_actor,
            ),
            (
                "destination-stat",
                {"n": rename, "r": {**stat, "path": moved}},
                same_actor,
            ),
        )
        for name, actions, actors in valid:
            with self.subTest(case=name):
                from_json(nominal_spec_text(actions, actors), nominal_target())
        invalid = (
            (
                "destination-unordered",
                {"n": rename, "r": {**read, "path": moved}},
                two_actors,
            ),
            (
                "destination-before-rename",
                {"n": rename, "r": {**read, "path": moved}},
                [{"id": "x", "actions": ["r", "n"]}],
            ),
            (
                "unknown",
                {"n": rename, "r": {**stat, "path": "tree/registers/x.reg"}},
                same_actor,
            ),
            (
                "mkdir-later",
                {"n": made, "r": {**stat, "path": "tree/registers/d"}},
                two_actors,
            ),
        )
        for name, actions, actors in invalid:
            with self.subTest(case=name):
                with self.assertRaisesRegex(SpecError, "not known to the model"):
                    from_json(nominal_spec_text(actions, actors), nominal_target())

    def test_edenfs_kinds_exact(self) -> None:
        self.assertEqual(
            frozenset(
                {
                    "write",
                    "read",
                    "getattr",
                    "status",
                    "checkout",
                    "readdir",
                    "readdir_stream",
                    "readdir_resume",
                    "open",
                    "read_fd",
                    "close",
                    "rename",
                    "unlink",
                    "mkdir",
                    "chmod",
                    "sl_rm",
                    "sl_mv",
                    "background_read",
                    "unload_inodes",
                    "manifest_presence",
                    "journal_window",
                    "status_counts",
                }
            ),
            frozenset(EDENFS_ACTION_DEFS),
        )

    def test_every_def_has_axis(self) -> None:
        expected = {
            "write": "input",
            "read": "input",
            "getattr": "input",
            "status": "input",
            "checkout": "input",
            "readdir": "input",
            "readdir_stream": "input",
            "readdir_resume": "input",
            "open": "input",
            "read_fd": "input",
            "close": "input",
            "rename": "input",
            "unlink": "input",
            "mkdir": "input",
            "chmod": "input",
            "sl_rm": "input",
            "sl_mv": "input",
            "background_read": "nemesis",
            "unload_inodes": "nemesis",
            "manifest_presence": "observation",
            "journal_window": "observation",
            "status_counts": "observation",
        }
        self.assertEqual(
            expected,
            {kind: action_def.axis for kind, action_def in EDENFS_ACTION_DEFS.items()},
        )

    def test_manifest_presence_requires_both_revisions(self) -> None:
        """Manifest observations require both ends of the revision window."""
        action = {
            "kind": "manifest_presence",
            "deadline": "operation",
            "from_revision": 0,
            "to_revision": 1,
        }
        for missing in ("from_revision", "to_revision"):
            with self.subTest(missing=missing):
                incomplete = {
                    key: value for key, value in action.items() if key != missing
                }
                with self.assertRaisesRegex(SpecError, missing):
                    from_json(one_action_spec(incomplete), nominal_target())
        for field, value in (
            ("from_revision", -1),
            ("to_revision", 4),
            ("to_revision", 10**12),
        ):
            with self.subTest(field=field, value=value):
                with self.assertRaisesRegex(SpecError, "out of range"):
                    from_json(
                        one_action_spec({**action, field: value}), nominal_target()
                    )

    def test_observation_kinds_are_not_modeled(self) -> None:
        """Observation completions cannot satisfy the modeled-operation gate."""
        for kind in ("manifest_presence", "journal_window", "status_counts"):
            with self.subTest(kind=kind):
                self.assertFalse(EDENFS_ACTION_DEFS[kind].modeled)

    def test_kind_collision_is_startup_error(self) -> None:
        with self.assertRaisesRegex(SpecError, "collision.*sleep"):
            compose_action_defs(CORE_ACTION_DEFS, {"sleep": CORE_ACTION_DEFS["sleep"]})

    def test_write_to_churn_path_is_status_only(self) -> None:
        write = {
            "kind": "write",
            "deadline": "operation",
            "path": CHURN_FILES[0],
            "value_id": "value",
        }
        self.assertEqual(
            "write",
            from_json(one_action_spec(write), nominal_target()).action("a").kind,
        )

        read = {
            "kind": "read",
            "deadline": "operation",
            "path": CHURN_FILES[0],
            "channel": "fuse",
        }
        with self.assertRaisesRegex(SpecError, "status-only"):
            from_json(one_action_spec(read), nominal_target())

    def test_repository_blob_size_is_an_optional_positive_int(self) -> None:
        read = {"kind": "read", "deadline": "operation", "path": R0, "channel": "fuse"}
        raw = json.loads(one_action_spec(read))
        repository = raw["environment"]["target"]["repository"]
        repository["blob_size"] = 1048576
        from_json(json.dumps(raw), nominal_target())
        for bad in (0, 64, "1M"):
            repository["blob_size"] = bad
            with (
                self.subTest(blob_size=bad),
                self.assertRaisesRegex(SpecError, "blob_size"),
            ):
                from_json(json.dumps(raw), nominal_target())

    def test_repository_commit_count_has_a_ceiling(self) -> None:
        read = {"kind": "read", "deadline": "operation", "path": R0, "channel": "fuse"}
        raw = json.loads(one_action_spec(read))
        repository = raw["environment"]["target"]["repository"]
        repository["commit_count"] = MAX_COMMIT_COUNT
        from_json(json.dumps(raw), nominal_target())
        repository["commit_count"] = MAX_COMMIT_COUNT + 1
        with self.assertRaisesRegex(SpecError, "commit_count must be at most"):
            from_json(json.dumps(raw), nominal_target())

    def test_read_thrift_requires_sync_timeout(self) -> None:
        read = {
            "kind": "read",
            "deadline": "operation",
            "path": "tree/registers/r0.reg",
            "channel": "thrift",
        }
        with self.assertRaisesRegex(SpecError, "sync_timeout_seconds"):
            from_json(one_action_spec(read), nominal_target())

    def test_read_fuse_rejects_sync_timeout(self) -> None:
        read = {
            "kind": "read",
            "deadline": "operation",
            "path": "tree/registers/r0.reg",
            "channel": "fuse",
            "sync_timeout_seconds": 60,
        }
        with self.assertRaisesRegex(SpecError, "sync_timeout_seconds"):
            from_json(one_action_spec(read), nominal_target())

    def test_two_actors_may_write_one_path(self) -> None:
        actions = {
            action_id: {
                "kind": "write",
                "deadline": "operation",
                "path": "tree/registers/r0.reg",
                "value_id": action_id,
            }
            for action_id in ("a", "b")
        }
        spec: RunSpec = from_json(
            nominal_spec_text(
                actions,
                [
                    {"id": "first", "actions": ["a"]},
                    {"id": "second", "actions": ["b"]},
                ],
            ),
            nominal_target(),
        )
        self.assertEqual(
            ("write", "write"), tuple(action.kind for action in spec.actions)
        )


# The error codes a Thrift read reports in `error_code`.
_THRIFT_READ_CODES = {"ENOENT", "EISDIR"}


class RunValidationTest(unittest.TestCase):
    def test_writer_ids_that_alias_value_fields_are_rejected(self) -> None:
        """Actor `a action=b` writing `c` would encode like `a` writing `b action=c`."""
        spec = json.loads(
            nominal_spec_text(
                {"w": action_payload("write", path=R0, value_id="c")},
                [{"id": "a action=b", "actions": ["w"]}],
            )
        )
        with self.assertRaisesRegex(SpecError, "empty or ambiguous"):
            from_json(json.dumps(spec), nominal_target())

    def test_a_seed_too_large_for_the_fixture_images_is_a_spec_error(self) -> None:
        """An oversized seed fails validation, not fixture construction."""
        spec = json.loads(
            nominal_spec_text(
                {"g": action_payload("getattr", path=R0)},
                [{"id": "a", "actions": ["g"]}],
            )
        )
        spec["seed"] = 2**53
        with self.assertRaisesRegex(SpecError, "does not fit the fixture images"):
            from_json(json.dumps(spec), nominal_target())
        spec["seed"] = 10**9
        from_json(json.dumps(spec), nominal_target())

    def test_run_checks_always_run_because_a_run_has_actions(self) -> None:
        """The run checks ride the first and last action; a run has at least one."""
        spec = json.loads(differential_spec_text({}, [{"id": "a", "actions": []}]))
        with self.assertRaisesRegex(SpecError, "actions must be a non-empty object"):
            from_json(json.dumps(spec), nominal_target())


class RegisterLayoutTest(unittest.TestCase):
    """A register is a path no other class of fixture path claims."""

    def _spec(self, registers: list[str], *, type_changes: tuple[str, ...] = ()) -> str:
        raw = json.loads(
            _wide_spec(
                {"g": action_payload("getattr", path=R0)},
                [{"id": "a", "actions": ["g"]}],
            )
        )
        repository = raw["environment"]["target"]["repository"]
        repository.update(file_count=200, commit_count=4, registers=registers)
        if type_changes:
            repository["type_changes"] = [
                {"path": path, "at_revision": 2} for path in type_changes
            ]
        return json.dumps(raw)

    def test_registers_apart_from_every_other_fixture_path_are_accepted(
        self,
    ) -> None:
        r1 = "tree/registers/r1.reg"
        spec = from_json(
            self._spec(
                [R0, r1, "top.reg", "tree/registers/deep/x.reg", "tree/a2/x.reg"],
                type_changes=(r1,),
            ),
            nominal_target(),
        )
        self.assertEqual(5, len(spec.environment.target["repository"]["registers"]))

    def test_registers_claimed_by_another_fixture_path_are_refused(self) -> None:
        r1 = "tree/registers/r1.reg"
        cases = (
            ("tree/a0/000/00000.dat", "is a generated path or directory, or under one"),
            ("tree/a0/r1/00000.dat", "is a generated path or directory, or under one"),
            ("tree/a1/000/00001.dat", "is a generated path or directory, or under one"),
            ("tree", "is a generated path or directory, or under one"),
            ("tree/a0", "is a generated path or directory, or under one"),
            ("tree/a1/r2", "is a generated path or directory, or under one"),
            ("tree/a1/x.reg", "is a generated path or directory, or under one"),
            (R0, f"register '{R0}' overlaps register '{R0}'"),
            (f"{R0}/x.reg", f"register '{R0}' overlaps register '{R0}/x.reg'"),
            ("tree/registers", f"register '{R0}' overlaps register 'tree/registers'"),
            ("tree/churn/c0", "overlaps status-only churn path 'tree/churn/c0'"),
            ("tree/churn", "overlaps status-only churn path 'tree/churn/c0'"),
            ("tree/wide", "overlaps wide directory 'tree/wide'"),
            ("tree/wide/000000.reg", "overlaps wide directory 'tree/wide'"),
            ("tree/registers//x.reg", "is not a canonical path"),
            ("tree/registers/x.reg/", "is not a canonical path"),
        )
        for register, message in cases:
            with self.subTest(register=register):
                with self.assertRaisesRegex(SpecError, re.escape(message)):
                    from_json(self._spec([R0, register]), nominal_target())
        with self.subTest(register="under a type-changing register"):
            with self.assertRaisesRegex(
                SpecError, re.escape(f"'{r1}' overlaps register '{r1}/entry.dat'")
            ):
                from_json(
                    self._spec([R0, r1, f"{r1}/entry.dat"], type_changes=(r1,)),
                    nominal_target(),
                )


class StaticEffectsTest(unittest.TestCase):
    def test_roles_binding_and_grouping(self) -> None:
        mutating = {
            kind
            for kind, definition in ACTION_DEFS.items()
            if definition.roles & MUTATING_ROLES
        }
        self.assertEqual(
            {
                "write",
                "rename",
                "unlink",
                "mkdir",
                "chmod",
                "sl_rm",
                "sl_mv",
                "checkout",
            },
            mutating,
        )
        self.assertEqual(Grouping.PROGRESSIVE, ACTION_DEFS["checkout"].grouping)
        self.assertEqual(Grouping.ATOMIC, ACTION_DEFS["rename"].grouping)
        self.assertEqual(Binding.DESCRIPTOR, ACTION_DEFS["read_fd"].binding)
        self.assertEqual(Binding.AGGREGATE, ACTION_DEFS["status"].binding)
        self.assertIn(Role.TRANSFER, ACTION_DEFS["sl_mv"].roles)
        for kind in ("background_read", "unload_inodes", "sleep", "fault_install"):
            with self.subTest(kind=kind):
                self.assertEqual(frozenset(), ACTION_DEFS[kind].roles)

    def test_every_kind_declares_a_footprint_from_its_payload(self) -> None:
        spec = from_json(
            nominal_spec_text(
                {
                    "o": {"kind": "open", "deadline": "operation", "path": R0},
                    "s": {
                        "kind": "readdir_stream",
                        "deadline": "operation",
                        "path": "tree/wide",
                        "pause_after": 1,
                    },
                    "z": {
                        "kind": "readdir_resume",
                        "deadline": "operation",
                        "stream": "s",
                    },
                },
                [{"id": "a", "actions": ["o", "s", "z"]}],
            ).replace(
                '"scatter_dirty": 0',
                '"scatter_dirty": 0, "wide_dir": {"name": "tree/wide", "entries": 4, "churn_all": true}',
            ),
            nominal_target(),
        )
        self.assertEqual(
            frozenset({"tree/wide"}),
            ACTION_DEFS["readdir_resume"]
            .footprint(spec, spec.action("z"))
            .subtrees_read,
        )
        self.assertEqual(
            frozenset({R0}),
            ACTION_DEFS["close"].footprint(spec, kind_action("close", fd="o")).reads,
        )

    def test_descriptor_paths_cover_every_order_of_moves(self) -> None:
        """Every place moves can carry an opened object is under a returned root.

        Cross-nested moves (`a` -> `b/x`, `b` -> `a/y`, ...) reach a number
        of exact paths exponential in the moves; the roots stay among the
        moves' endpoints.
        """

        def moves(count: int) -> list[tuple[str, str]]:
            return [
                ("ab"[index % 2], "ab"[(index + 1) % 2] + "/" + "xyzuvw"[index])
                for index in range(count)
            ]

        def spec(pairs: list[tuple[str, str]]) -> Any:
            return SimpleNamespace(
                actions=[
                    kind_action("rename", source=source, destination=destination)
                    for source, destination in pairs
                ]
            )

        def carried(path: str, pairs: list[tuple[str, str]]) -> set[str]:
            reached = {path}
            for order in itertools.permutations(pairs):
                current = path
                for source, destination in order:
                    for old, new in ((source, destination), (destination, source)):
                        if current == old or current.startswith(old + "/"):
                            current = new + current[len(old) :]
                            reached.add(current)
                            break
            return reached

        for count in range(5):
            pairs = moves(count)
            roots = descriptor_paths(spec(pairs), "a/f")
            with self.subTest(moves=count):
                for path in carried("a/f", pairs):
                    self.assertTrue(
                        path == "a/f"
                        or any(path == r or path.startswith(r + "/") for r in roots),
                        path,
                    )
        endpoints = {path for pair in moves(6) for path in pair}
        roots = descriptor_paths(spec(moves(6)), "a/f")
        self.assertTrue(roots <= endpoints, f"{len(roots)} roots")


class WriteValueTest(unittest.TestCase):
    def test_value_ids_are_unique_and_outside_the_fixture_namespace(self) -> None:
        def writes(first: str, second: str, actor: str = "a") -> str:
            return nominal_spec_text(
                {
                    "w1": action_payload("write", path=R0, value_id=first),
                    "w2": action_payload("write", path=R0, value_id=second),
                },
                [{"id": actor, "actions": ["w1", "w2"]}],
            )

        from_json(writes("w1", "w2"), nominal_target())
        with self.assertRaisesRegex(SpecError, "repeats value_id 'v' of 'w1'"):
            from_json(writes("v", "v"), nominal_target())
        for reserved in ("init", "commit", "commit3"):
            with self.subTest(value_id=reserved):
                with self.assertRaisesRegex(SpecError, "value_id .* is reserved"):
                    from_json(writes("w1", reserved), nominal_target())
        with self.assertRaisesRegex(SpecError, "writer 'commit1' is reserved"):
            from_json(writes("w1", "w2", actor="commit1"), nominal_target())
        with self.assertRaisesRegex(SpecError, "too long"):
            from_json(writes("w1", "v" * 64), nominal_target())
