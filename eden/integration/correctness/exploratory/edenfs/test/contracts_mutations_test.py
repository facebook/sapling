# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""How the contracts turns one recorded mutation into effect groups: the resolution class
of each outcome (acked, definitely failed, indeterminate), the window its effects may
land in, the cells each kind writes, the state cell an errno names, `sl rm` and `sl
mv` outcomes as a function of the pre-state, and cell and descriptor keys that do not
depend on action names.
"""

from __future__ import annotations

import errno
import json
import math
import unittest
from typing import Any

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    ANY,
    Cell,
    EffectKind,
    Expect,
    ObligationRef,
    Status,
    Value,
    Window,
)
from eden.integration.correctness.exploratory.core.facts import (
    ErrnoUse,
    HistoryFacts,
    NO_ERRNO,
)
from eden.integration.correctness.exploratory.core.history import Event
from eden.integration.correctness.exploratory.core.runspec import RunSpec
from eden.integration.correctness.exploratory.core.test.testing import ev
from eden.integration.correctness.exploratory.edenfs.actions import ACTION_DEFS
from eden.integration.correctness.exploratory.edenfs.contracts import (
    make_contracts,
    record_contracts,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    CONTRACT_RULES,
    resolution,
)
from eden.integration.correctness.exploratory.edenfs.contracts.files import (
    descriptor_keys,
    PRECONDITION_RULE,
    REMOVED,
    UNJUDGED_STATE_ERRNOS,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import (
    DIR,
    file_value,
    INPUT_LEGALITY_RULE,
    mode_value,
    path_cell,
    Resolution,
    track_cell,
)
from eden.integration.correctness.exploratory.edenfs.encoding import encode_value
from eden.integration.correctness.exploratory.edenfs.test.contracts_fixtures import (
    action_payload,
    checkout_op,
    committed_value,
    contract_one,
    contract_sink,
    digest_facts,
    guard_holds,
    is_explained,
    R0,
    R1,
    read_obs,
    run_spec,
    SEED,
    SL_OK,
    THRIFT_OK,
    TIP,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import (
    RecordingSink,
    VOCAB_TARGET,
)


DESCRIPTOR_KINDS = frozenset({"read_fd", "close"})


class ResolutionTest(unittest.TestCase):
    def test_outcome_classes(self) -> None:
        spec = run_spec(
            {
                "w": action_payload("write", path=R0, value_id="w"),
                "m": action_payload("sl_rm", path=R1),
            },
            [{"id": "a", "actions": ["w", "m"]}],
        )
        cases = (
            ("w", ("ok", {"bytes_written": 64}), Resolution.ACKED),
            ("w", ("info", {"reason": "operation-timeout"}), Resolution.INDETERMINATE),
            ("w", ("fail", {"reason": "x", "errno": 110}), Resolution.INDETERMINATE),
            ("w", ("fail", {"reason": "x", "errno": 107}), Resolution.INDETERMINATE),
            (
                "w",
                ("fail", {"reason": "x", "errno": 2, "close_errno": 5}),
                Resolution.INDETERMINATE,
            ),
            ("w", ("fail", {"reason": "x", "errno": 2}), Resolution.NONE),
            ("w", ("fail", {"reason": "x", "errno": 5}), Resolution.NONE),
            (
                "m",
                ("fail", {"reason": "x", "returncode": -9, "stdout": "", "stderr": ""}),
                Resolution.INDETERMINATE,
            ),
            (
                "m",
                ("fail", {"reason": "x", "returncode": 1, "stdout": "", "stderr": ""}),
                Resolution.NONE,
            ),
            ("w", ("ok", {"bytes_written": True}), Resolution.INDETERMINATE),
        )
        for action, (outcome, facts), expected in cases:
            with self.subTest(action=action, facts=facts):
                events = [ev("invoke", "w", 1.0)]
                if action == "w":
                    events.append(ev(outcome, "w", 2.0, **facts))
                else:
                    events += [
                        ev("ok", "w", 2.0, bytes_written=64),
                        ev("invoke", "m", 3.0),
                        ev(outcome, "m", 4.0, **facts),
                    ]
                built = HistoryFacts.build(spec, events, VOCAB_TARGET)
                self.assertEqual(expected, resolution(built.action(action)))
        never = HistoryFacts.build(spec, [], VOCAB_TARGET)
        self.assertEqual(Resolution.NONE, resolution(never.action("w")))


class MutationTest(unittest.TestCase):
    def test_an_acked_write_produces_its_value_and_needs_a_file(self) -> None:
        _spec_, sink = contract_one(
            action_payload("write", path=R0, value_id="w"),
            ev("invoke", "x", 1.0),
            ev("ok", "x", 2.0, bytes_written=64),
            actor="b",
        )
        (group,) = sink.groups
        (effect,) = group.effects
        self.assertEqual(Status.ACKED, group.status)
        self.assertEqual(Window(1.0, 2.0), group.window)
        self.assertEqual(path_cell(R0), effect.cell)
        self.assertEqual(file_value(encode_value(SEED, "b", "w")), effect.value)
        (requirement,) = group.requires
        self.assertEqual(ObligationRef(PRECONDITION_RULE, "x"), requirement.obligation)
        self.assertTrue(
            guard_holds(requirement, {path_cell(R0): committed_value(R0, TIP)})
        )
        self.assertFalse(guard_holds(requirement, {path_cell(R0): ABSENT}))
        self.assertFalse(guard_holds(requirement, {path_cell(R0): DIR}))
        self.assertEqual([], sink.observations)

    def test_an_open_write_lasts_until_the_resolution_boundary(self) -> None:
        """The supervisor's run-closed record resolves it, else an unowned daemon exit.

        The runner's own former close record, `target-closed`, resolves nothing.
        """
        closed = Event("run-closed", 9.0, {})
        exited = Event("info", 7.0, {"reason": "daemon-exit", "returncode": 1})
        runner_closed = Event("target-closed", 8.0, {})
        for extra, end in (
            ([closed], 9.0),
            ([exited], 7.0),
            ([exited, closed], 9.0),
            ([runner_closed], math.inf),
            ([], math.inf),
        ):
            with self.subTest(extra=[event.type for event in extra]):
                _spec_, sink = contract_one(
                    action_payload("write", path=R0, value_id="w"),
                    ev("invoke", "x", 1.0),
                    ev("fail", "x", 2.0, reason="timeout", errno=110),
                    *extra,
                )
                (group,) = sink.groups
                self.assertEqual(Status.INDETERMINATE, group.status)
                self.assertEqual(Window(1.0, end), group.window)

    def test_namespace_mutations(self) -> None:
        cases = {
            "rename": action_payload(
                "rename", source=R0, destination="tree/registers/m"
            ),
            "unlink": action_payload("unlink", path=R0),
            "mkdir": action_payload("mkdir", path="tree/registers/d"),
            "chmod": action_payload("chmod", path=R0, mode=0o755),
            "sl_rm": action_payload("sl_rm", path=R0),
            "sl_mv": action_payload("sl_mv", source=R0, destination="tree/registers/m"),
        }
        moved = "tree/registers/m"
        expected = {
            "rename": {(path_cell(moved), EffectKind.TRANSFER, None)},
            "unlink": {(path_cell(R0), EffectKind.PRODUCE, ABSENT)},
            "mkdir": {(path_cell("tree/registers/d"), EffectKind.PRODUCE, DIR)},
            "chmod": {
                (Cell("primary", "mode", R0), EffectKind.PRODUCE, mode_value(True))
            },
            "sl_rm": {
                (path_cell(R0), EffectKind.PRODUCE, ABSENT),
                (track_cell(R0), EffectKind.PRODUCE, REMOVED),
            },
            "sl_mv": {
                (path_cell(moved), EffectKind.TRANSFER, None),
                (track_cell(R0), EffectKind.PRODUCE, REMOVED),
                (track_cell(moved), EffectKind.PRODUCE, Value("added", (R0,))),
            },
        }
        for kind, payload in cases.items():
            with self.subTest(kind=kind):
                ok = {"path": "p"} if kind in ("unlink", "mkdir", "sl_rm") else {}
                if kind == "chmod":
                    ok = {"path": R0, "mode": 0o755}
                if kind in ("rename", "sl_mv"):
                    ok = {"source": R0, "destination": moved}
                _spec_, sink = contract_one(
                    payload, ev("invoke", "x", 1.0), ev("ok", "x", 2.0, **ok)
                )
                group = sink.groups[0]
                found = {
                    (effect.cell, effect.kind, effect.value) for effect in group.effects
                }
                self.assertLessEqual(expected[kind], found)

    def test_a_definite_failure_has_no_effect_and_a_state_errno_is_observed(
        self,
    ) -> None:
        spec, sink = contract_one(
            action_payload("write", path=R0, value_id="w"),
            ev("invoke", "x", 1.0),
            ev("fail", "x", 2.0, reason="open", errno=2),
        )
        self.assertEqual([], sink.groups)
        (observation,) = sink.observations
        self.assertEqual(
            (ObligationRef(INPUT_LEGALITY_RULE, "x"),), observation.obligations
        )
        self.assertEqual((path_cell(R0),), observation.cells)
        self.assertTrue(observation.expect.holds((ABSENT,)))
        self.assertFalse(observation.expect.holds((committed_value(R0, TIP),)))
        self.assertEqual(Window(1.0, 2.0), observation.window)

    def test_state_errnos_map_to_their_cells(self) -> None:
        cases = (
            (
                action_payload("rename", source=R0, destination="tree/registers/m"),
                21,
                (
                    (path_cell(R0), Value("file", ("a",))),
                    (path_cell("tree/registers/m"), DIR),
                ),
            ),
            (
                action_payload("mkdir", path="tree/registers/d"),
                17,
                ((path_cell("tree/registers/d"), DIR),),
            ),
            (
                action_payload("mkdir", path="tree/registers/d"),
                2,
                ((path_cell("tree/registers"), ABSENT),),
            ),
            (
                action_payload("readdir", path="tree/registers", channel="fuse"),
                20,
                (
                    (path_cell("tree/registers"), Value("file", ("a",))),
                    (path_cell("tree"), DIR),
                ),
            ),
            (action_payload("getattr", path=R0), 2, ((path_cell(R0), ABSENT),)),
        )
        for payload, code, legal in cases:
            with self.subTest(kind=payload["kind"], errno=code):
                _spec_, sink = contract_one(
                    payload,
                    ev("invoke", "x", 1.0),
                    ev("fail", "x", 2.0, reason="x", errno=code),
                )
                (observation,) = sink.observations
                self.assertEqual(tuple(cell for cell, _ in legal), observation.cells)
                self.assertTrue(
                    observation.expect.holds(tuple(value for _, value in legal))
                )
        _spec_, sink = contract_one(
            action_payload("write", path=R0, value_id="w"),
            ev("invoke", "x", 1.0),
            ev("fail", "x", 2.0, reason="x", errno=5),
        )
        self.assertEqual([], sink.observations)

    def test_every_state_errno_has_an_observation(self) -> None:
        payloads = {
            "write": action_payload("write", path=R0, value_id="w"),
            "read": action_payload("read", path=R0, channel="fuse"),
            "getattr": action_payload("getattr", path=R0),
            "open": action_payload("open", path=R0),
            "chmod": action_payload("chmod", path=R0, mode=0o644),
            "rename": action_payload(
                "rename", source=R0, destination="tree/registers/m"
            ),
            "unlink": action_payload("unlink", path=R0),
            "mkdir": action_payload("mkdir", path="tree/registers/d"),
            "readdir": action_payload("readdir", path="tree/registers", channel="fuse"),
        }
        for kind, definition in ACTION_DEFS.items():
            for name, use in definition.result.errnos.items():
                if (
                    use != ErrnoUse.STATE
                    or name == NO_ERRNO
                    or (kind, name) in UNJUDGED_STATE_ERRNOS
                    or kind in DESCRIPTOR_KINDS
                ):
                    continue
                with self.subTest(kind=kind, errno=name):
                    code = getattr(errno, name)
                    _spec_, sink = contract_one(
                        payloads[kind],
                        ev("invoke", "x", 1.0),
                        ev("fail", "x", 2.0, reason="x", errno=code),
                    )
                    self.assertEqual(1, len(sink.observations))

    def test_ebadf_through_an_unopened_descriptor_observes_nothing_about_state(
        self,
    ) -> None:
        """EBADF is legal exactly when the open was not acked.

        `close` is not a modeled input, so it has no obligation to support.
        """
        for kind in sorted(DESCRIPTOR_KINDS):
            for opened in ("fail", "ok"):
                with self.subTest(kind=kind, open=opened):
                    spec = run_spec(
                        {
                            "o": action_payload("open", path=R0),
                            "x": action_payload(kind, fd="o"),
                        },
                        [{"id": "a", "actions": ["o", "x"]}],
                    )
                    open_result = (
                        {"reason": "open", "errno": errno.ENOENT}
                        if opened == "fail"
                        else {"fd": 3}
                    )
                    events = [
                        ev("invoke", "o", 1.0),
                        ev(opened, "o", 2.0, **open_result),
                        ev("invoke", "x", 3.0),
                        ev("fail", "x", 4.0, reason="x", errno=errno.EBADF),
                    ]
                    sink = RecordingSink()
                    record_contracts(
                        spec,
                        HistoryFacts.build(spec, events, VOCAB_TARGET),
                        None,
                        sink,
                        CONTRACT_RULES,
                    )
                    found = [o for o in sink.observations if o.action == "x"]
                    if opened == "ok" or kind == "close":
                        self.assertEqual([], found)
                        continue
                    (observation,) = found
                    self.assertEqual((path_cell(R0),), observation.cells)
                    self.assertEqual(Expect(((ANY,),)), observation.expect)
                    self.assertEqual(
                        (ObligationRef(INPUT_LEGALITY_RULE, "x"),),
                        observation.obligations,
                    )

    def test_unregistered_rules_are_left_out(self) -> None:
        spec = run_spec(
            {"x": action_payload("write", path=R0, value_id="w")},
            [{"id": "a", "actions": ["x"]}],
        )
        events = [ev("invoke", "x", 1.0), ev("ok", "x", 2.0, bytes_written=64)]
        (group,) = contract_sink(spec, events, rules=frozenset()).groups
        self.assertEqual((), group.requires)
        facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
        sink = RecordingSink()
        make_contracts({INPUT_LEGALITY_RULE, "other.rule"})(spec, facts, None, sink)
        self.assertEqual((), sink.groups[0].requires)


def _content(sink: RecordingSink) -> list[str]:
    """Return the contract inputs' documents without IDs (obligations keep only their rule)."""
    documents = []
    for group in sink.groups:
        document = group.document()
        for name in ("id", "action", "outcome_var"):
            document.pop(name)
        for effect in document["effects"]:
            effect.pop("provenance")
        for requirement in document["requires"]:
            requirement["obligation"] = requirement["obligation"][0]
        documents.append(json.dumps(document, sort_keys=True))
    for observation in sink.observations:
        document = observation.document()
        for name in ("id", "action", "obligations"):
            document.pop(name)
        documents.append(json.dumps(document, sort_keys=True))
    return sorted(documents)


class IdentityFreeCellsTest(unittest.TestCase):
    def test_renaming_actions_leaves_every_cell_key_unchanged(self) -> None:
        """Deletion order reads cells and expectations; no cell key may carry an action ID.

        The write overlaps the FORCE checkout, so the contracts also
        build the write-vs-checkout supersede marks.
        """

        def run(names: dict[str, str]) -> RecordingSink:
            w, c, d = names["w"], names["c"], names["d"]
            spec = run_spec(
                {
                    w: action_payload("write", path=R0, value_id="v"),
                    c: checkout_op(1, via="goto"),
                    d: checkout_op(TIP, mode="force"),
                },
                [{"id": "a", "actions": [w]}, {"id": "b", "actions": [d, c]}],
            )
            return contract_sink(
                spec,
                [
                    ev("invoke", w, 1.0),
                    ev("invoke", d, 1.5),
                    ev("ok", w, 2.0, bytes_written=64),
                    ev("ok", d, 3.0, revision=TIP, **THRIFT_OK),
                    ev("invoke", c, 5.0),
                    ev("ok", c, 6.0, revision=1, **SL_OK),
                ],
            )

        plain = run({"w": "w", "c": "c", "d": "d"})
        names = {"w": "zw-write", "c": "a0-update", "d": "b9-force"}
        renamed = run(names)
        self.assertEqual(_content(plain), _content(renamed))
        self.assertFalse(
            [
                name
                for document in _content(renamed)
                for name in names.values()
                if name in document
            ]
        )
        marks = {
            cell.key
            for group in plain.groups
            for effect in group.effects
            for cell in effect.writes()
            if cell.space == "visit" and ":" in cell.key
        }
        self.assertTrue(marks)
        keys = {
            cell.key
            for group in plain.groups
            for effect in group.effects
            for cell in effect.writes()
            if cell.space in ("source", "visit")
        }
        self.assertTrue(keys)
        self.assertTrue(all(key.startswith(("k0/", "k1/", "k0:o0/")) for key in keys))


class DescriptorKeysTest(unittest.TestCase):
    def test_renaming_opens_leaves_descriptor_keys_and_cells_unchanged(self) -> None:
        """`d<rank>` ranks opens by time, never by action ID."""

        def run(names: dict[str, str]) -> tuple[dict[str, str], RecordingSink]:
            first, second = names["o1"], names["o2"]
            spec = run_spec(
                {
                    first: action_payload("open", path=R0),
                    second: action_payload("open", path=R1),
                    "w": action_payload("write", path=R0, value_id="v"),
                    "f": action_payload("read_fd", fd=first),
                    "g": action_payload("read_fd", fd=second),
                },
                [{"id": "a", "actions": [first, second, "w", "f", "g"]}],
            )
            events = [
                ev("invoke", first, 1.0),
                ev("ok", first, 2.0, fd=3),
                ev("invoke", second, 3.0),
                ev("ok", second, 4.0, fd=4),
                ev("invoke", "w", 5.0),
                ev("ok", "w", 6.0, bytes_written=64),
                ev("invoke", "f", 7.0),
                ev("ok", "f", 8.0, **digest_facts(b"a" * 64)),
                ev("invoke", "g", 9.0),
                ev("ok", "g", 10.0, **digest_facts(b"b" * 64)),
            ]
            facts = HistoryFacts.build(spec, events, VOCAB_TARGET)
            sink = RecordingSink()
            record_contracts(spec, facts, None, sink, CONTRACT_RULES)
            return descriptor_keys(facts), sink

        plain_keys, plain = run({"o1": "o1", "o2": "o2"})
        renamed_keys, renamed = run({"o1": "zz", "o2": "aa"})
        self.assertEqual({"o1": "d0", "o2": "d1"}, plain_keys)
        self.assertEqual({"zz": "d0", "aa": "d1"}, renamed_keys)
        self.assertEqual(_content(plain), _content(renamed))
        spaces = {
            cell.space
            for group in plain.groups
            for effect in group.effects
            for cell in effect.writes()
        }
        self.assertIn("fd-object", spaces)


class SlRmTest(unittest.TestCase):
    """`sl rm --force` outcome as a function of pre-state."""

    def test_a_missing_tracked_file_is_marked_removed(self) -> None:
        spec = run_spec(
            {
                "m": action_payload(
                    "rename", source=R0, destination="tree/registers/m"
                ),
                "x": action_payload("sl_rm", path=R0),
            },
            [{"id": "a", "actions": ["m", "x"]}],
        )
        events = [
            ev("invoke", "m", 1.0),
            ev("ok", "m", 2.0, source=R0, destination="tree/registers/m"),
            ev("invoke", "x", 3.0),
            ev("ok", "x", 4.0, path=R0),
        ]
        self.assertTrue(is_explained(spec, events))
        moved = read_obs("r", "tree/registers/m", committed_value(R0, TIP), 5, 6)
        self.assertTrue(is_explained(spec, events, (moved,)))


class SlMvTest(unittest.TestCase):
    """`sl mv` exit 0 does not imply a move."""

    _D = "tree/registers/m"

    def _mv(
        self, *before: tuple[str, dict[str, Any], dict[str, Any]], outcome: Event
    ) -> tuple[RunSpec, list[Event]]:
        actions = {name: payload for name, payload, _done in before}
        actions["v"] = action_payload("sl_mv", source=R0, destination=self._D)
        spec = run_spec(actions, [{"id": "a", "actions": list(actions)}])
        events: list[Event] = []
        for index, (name, _payload, done) in enumerate(before):
            events += [
                ev("invoke", name, index + 0.1),
                ev("ok", name, index + 0.5, **done),
            ]
        return spec, [*events, ev("invoke", "v", 10.0), outcome]

    def test_a_tracked_source_absent_from_disk_is_only_marked_removed(self) -> None:
        spec, events = self._mv(
            ("u", action_payload("unlink", path=R0), {"path": R0}),
            outcome=ev("ok", "v", 11.0, source=R0, destination=self._D),
        )
        self.assertTrue(
            is_explained(spec, events, (read_obs("b", self._D, ABSENT, 12, 13),))
        )
