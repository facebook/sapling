# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Live metadata, descriptor, readdir, and differential histories judged by `EDENFS_TARGET`.

A serial sequence holds a descriptor across a Thrift checkout and another
across a rename onto a register, streams the wide directory, and stats a
path around a chmod and a write: the verdict must accept with every
descriptor, attributes, and readdir obligation satisfied. The same
sequence with the stream paused across the checkout is accepted too: a
repeated name (T287885560's shape, observed here) is a wide-directory
entry the checkout replaced, listed at most twice, which
`readdir.duplicate-entry` allows. A descriptor held across a
FORCE or NORMAL checkout to the current revision sees a later write
through its path (EdenFS keeps the clean inode), and one held across a
FORCE checkout that changes the path does not. Generated differential
specs (`compose(differential=True)`), including ones holding a descriptor
or listing a directory, and generated plain specs holding a descriptor
run live and must be accepted. A descriptor whose open fails because its
path is gone (the generated seed 37 shape the composer now declines, and
its serial twin) ends in a verdict: its `read_fd` and `close` fail EBADF.
"""

from __future__ import annotations

import errno
import json
import os
import unittest
from collections import Counter
from collections.abc import Iterable, Mapping
from typing import Any

from eden.integration.correctness.exploratory.core import checker
from eden.integration.correctness.exploratory.core.generate import compose, derived_rng
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    Actor,
    Budgets,
    DEADLINE_OPERATION,
    DEFAULT_BUDGETS,
    Environment,
    RunSpec,
)
from eden.integration.correctness.exploratory.edenfs.fixture import layout_for
from eden.integration.correctness.exploratory.edenfs.target import (
    EDENFS_TARGET,
    sample_differential_environment,
    sample_environment,
)
from eden.integration.correctness.exploratory.edenfs.test.backends_fixtures import (
    _artifact_directory,
    _fail_on_setup_failure,
    _require_live,
)
from eden.integration.correctness.exploratory.edenfs.test.live import run_contained

_R0, _R1, _R2 = (f"tree/registers/r{index}.reg" for index in range(3))
_WIDE = "tree/wide"
_JUDGED_PROPERTIES = ("fd", "attributes", "readdir", "differential")


def _action(action_id: str, kind: str, **payload: Any) -> Action:
    return Action(action_id, kind, DEADLINE_OPERATION, payload=payload)


def sequence_spec(*, stream_across_checkout: bool = False) -> RunSpec:
    """Return the serial descriptor, stream, and attribute sequence.

    The stream pauses before the checkout and resumes after it only when
    `stream_across_checkout`: that shape may repeat an entry the checkout
    replaced (T287885560's shape).
    """
    stream = [
        _action("d0", "readdir_stream", path=_WIDE, pause_after=2),
        _action("d1", "readdir_resume", stream="d0"),
    ]
    checkout = [_action("c", "checkout", revision=2, via="thrift", mode="normal")]
    actions = [
        _action("o", "open", path=_R0),
        *(
            [stream[0], *checkout, stream[1]]
            if stream_across_checkout
            else [*stream, *checkout]
        ),
        _action("f", "read_fd", fd="o"),
        _action("e", "close", fd="o"),
        _action("g0", "getattr", path=_R0),
        _action("ch", "chmod", path=_R0, mode=0o755),
        _action("g1", "getattr", path=_R0),
        _action("w", "write", path=_R0, value_id="live-w"),
        _action("g2", "getattr", path=_R0),
        _action("o2", "open", path=_R0),
        _action("m", "rename", source=_R0, destination=_R1),
        _action("w2", "write", path=_R1, value_id="live-w2"),
        _action("f2", "read_fd", fd="o2"),
        _action("e2", "close", fd="o2"),
        _action("g3", "getattr", path=_R1),
        _action("g4", "getattr", path=_R1),
    ]
    environment = {
        "repository": {
            "file_count": 32,
            "commit_count": 4,
            "registers": [_R0, _R1, _R2],
        },
        "channel": "fuse",
        "case_sensitive": True,
        "fixture_source": "backing",
        "setup_profile": {
            "materialize": False,
            "scatter_dirty": 0,
            "wide_dir": {"name": _WIDE, "entries": 2000, "churn_all": True},
        },
    }
    return RunSpec(
        seed=13,
        target=EDENFS_TARGET.name,
        environment=Environment(environment),
        budgets=Budgets(120.0, 60.0, 540.0),
        actors=(Actor(id="a", actions=tuple(action.id for action in actions)),),
        actions=tuple(actions),
    )


def held_spec(mode: str, revision: int) -> RunSpec:
    """Return open r0, a Thrift checkout, a write to r0, then read_fd and close."""
    spec = sequence_spec()
    actions = [
        _action("o", "open", path=_R0),
        _action("c", "checkout", revision=revision, via="thrift", mode=mode),
        _action("w", "write", path=_R0, value_id="live-w"),
        _action("f", "read_fd", fd="o"),
        _action("e", "close", fd="o"),
    ]
    return RunSpec(
        seed=17,
        target=EDENFS_TARGET.name,
        environment=spec.environment,
        budgets=spec.budgets,
        actors=(Actor(id="a", actions=tuple(action.id for action in actions)),),
        actions=tuple(actions),
    )


def removed_open_spec(*, generated_shape: bool) -> RunSpec:
    """Return a held descriptor whose path is removed before its open.

    The generated shape is seed 37 before the composer declined it:
    `sl rm r1` precedes the open while a FORCE checkout that
    restores r1 runs concurrently. The serial twin unlinks r1 first.
    """
    if not generated_shape:
        spec = held_spec("normal", 3)
        actions = [
            _action("u", "unlink", path=_R1),
            _action("o", "open", path=_R1),
            _action("f", "read_fd", fd="o"),
            _action("e", "close", fd="o"),
        ]
        return RunSpec(
            seed=19,
            target=EDENFS_TARGET.name,
            environment=spec.environment,
            budgets=spec.budgets,
            actors=(Actor(id="a", actions=tuple(a.id for a in actions)),),
            actions=tuple(actions),
        )
    after = (("b0", "finished"), ("i1", "finished"), ("i2", "finished"))
    actions = [
        _action("b0", "status", channel="sl"),
        _action("i1", "sl_rm", path=_R1),
        _action("h0", "open", path=_R1),
        Action("v2", "getattr", DEADLINE_OPERATION, after, {"path": _R0}),
        _action("h1", "read_fd", fd="h0"),
        _action("h2", "close", fd="h0"),
        _action("i2", "read", path=_R1, channel="fuse"),
        Action(
            "v0",
            "checkout",
            DEADLINE_OPERATION,
            after,
            {"revision": 0, "via": "thrift", "mode": "force"},
        ),
        _action("v1", "checkout", revision=3, via="thrift", mode="normal"),
        _action("v3", "read", path=_R1, channel="fuse"),
    ]
    return RunSpec(
        seed=37,
        target=EDENFS_TARGET.name,
        environment=Environment(
            sample_environment(derived_rng(37, "plain-environment"))
        ),
        budgets=DEFAULT_BUDGETS,
        actors=(
            Actor(id="a0", actions=("b0", "i1", "h0", "v2", "h1", "h2")),
            Actor(id="a1", actions=("i2", "v0", "v1", "v3")),
        ),
        actions=tuple(sorted(actions, key=lambda action: action.id)),
    )


def descriptor_errno(history: str, action_id: str) -> int | None:
    """Return the errno a recorded descriptor operation failed with, or None."""
    for line in history.splitlines():
        event = json.loads(line)
        if event.get("action") == action_id and event.get("type") in ("ok", "fail"):
            return event.get("errno")
    raise AssertionError(f"no recorded result for {action_id!r}")


def differential_spec(seed: int) -> RunSpec:
    """Return the generated differential spec of one seed."""
    return compose(
        EDENFS_TARGET,
        seed,
        sample_differential_environment(derived_rng(seed, "differential-environment")),
        differential=True,
    )


def differential_specs(count: int) -> list[RunSpec]:
    """Return the first `count` generated differential specs."""
    return [differential_spec(seed) for seed in range(count)]


def _first(specs: Iterable[RunSpec], kind: str, count: int) -> list[RunSpec]:
    return [spec for spec in specs if any(a.kind == kind for a in spec.actions)][:count]


def plain_descriptor_specs(count: int) -> list[RunSpec]:
    """Return the first `count` generated no-fault plain specs holding a descriptor."""
    return _first(
        (
            compose(
                EDENFS_TARGET,
                seed,
                sample_environment(derived_rng(seed, "plain-environment")),
                no_fault=True,
            )
            for seed in range(200)
        ),
        "open",
        count,
    )


def saw_write(history: str, action_id: str) -> bool:
    """Return whether a recorded read returned the bytes of the `live-w` write."""
    for line in history.splitlines():
        event = json.loads(line)
        if event.get("action") == action_id and "bytes_prefix" in event:
            return b"action=live-w." in bytes.fromhex(event["bytes_prefix"])
    raise AssertionError(f"no recorded read for {action_id!r}")


def repeated_entries(history: str, action_id: str) -> dict[str, int]:
    """Return each name a recorded resume lists more than once, with its count."""
    for line in history.splitlines():
        event = json.loads(line)
        if event.get("action") == action_id and "entries" in event:
            counts = Counter(event["entries"])
            return {name: n for name, n in sorted(counts.items()) if n > 1}
    raise AssertionError(f"no recorded entries for {action_id!r}")


def _summary(verdict: checker.Verdict) -> Mapping[str, Any]:
    assert verdict.report is not None
    return {
        f"{item.rule_id}[{item.scope}]": [str(item.status), item.not_evaluated_reason]
        for result in verdict.report.results
        for item in result.obligations
        if str(item.status) != "satisfied"
    }


def _judged(verdict: checker.Verdict) -> dict[tuple[str, str], str]:
    assert verdict.report is not None
    return {
        (item.rule_id, item.scope): str(item.status)
        for result in verdict.report.results
        if result.property_id in _JUDGED_PROPERTIES
        for item in result.obligations
    }


class MetadataLiveTest(unittest.TestCase):
    def _live(self, spec: RunSpec, prefix: str) -> checker.Verdict:
        return self._live_history(spec, prefix)[0]

    def _live_history(self, spec: RunSpec, prefix: str) -> tuple[checker.Verdict, str]:
        _require_live(self)
        with _artifact_directory(prefix) as tmp:
            result = run_contained(spec, tmp)
            _fail_on_setup_failure(result, tmp)
            with open(os.path.join(tmp, "stderr.log")) as handle:
                stderr = handle.read()
            with open(os.path.join(tmp, "history.jsonl")) as handle:
                history = handle.read()
        self.assertEqual("completed", result.execution.status, stderr[-4000:])
        print(f"{prefix} verdict: {result.verdict.kind} {result.verdict.reason}")
        print(f"{prefix} history:\n{history}")
        return result.verdict, history

    def test_the_metadata_sequence_is_accepted(self) -> None:
        verdict = self._live(sequence_spec(), "n9-sequence.")
        summary = _summary(verdict)
        print("unsatisfied: " + json.dumps(summary, sort_keys=True))
        self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))
        judged = _judged(verdict)
        for key in (
            ("fd.stale-read", "f"),
            ("fd.stale-read", "f2"),
            ("fd.read-failed", "f"),
            ("readdir.duplicate-entry", "d1"),
            ("readdir.unknown-entry", "d1"),
            ("readdir.missing-entry", "d1"),
            ("attributes.type-disagreement", "g0"),
            ("attributes.mode-revert", "g1"),
            ("attributes.size-mismatch", "g2"),
            ("attributes.mtime-decreased", "g4"),
        ):
            with self.subTest(obligation=key):
                self.assertEqual("satisfied", judged.get(key))

    def test_a_stream_across_the_checkout_is_accepted(self) -> None:
        spec = sequence_spec(stream_across_checkout=True)
        verdict, history = self._live_history(spec, "n9-stream-across.")
        summary = _summary(verdict)
        print("unsatisfied: " + json.dumps(summary, sort_keys=True))
        self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))
        judged = _judged(verdict)
        for key in (
            ("fd.stale-read", "f"),
            ("readdir.duplicate-entry", "d1"),
            ("readdir.missing-entry", "d1"),
            ("readdir.unknown-entry", "d1"),
        ):
            with self.subTest(obligation=key):
                self.assertEqual("satisfied", judged.get(key))
        repeated = repeated_entries(history, "d1")
        print(
            f"n9-stream-across repeated names: {len(repeated)}, "
            f"extra listings: {sum(repeated.values()) - len(repeated)}"
        )
        # Every wide entry is `churn_all`: the checkout replaced each of them.
        wide = {
            entry.path.removeprefix(_WIDE + "/")
            for entry in layout_for(spec).wide_files()
        }
        self.assertLessEqual(set(repeated), wide)
        self.assertLessEqual(set(repeated.values()), {2})

    def test_a_held_descriptor_sees_a_write_only_through_an_unchanged_path(
        self,
    ) -> None:
        """FORCE to the current revision keeps the clean inode."""
        for mode, revision in (("force", 3), ("normal", 3), ("force", 2)):
            with self.subTest(mode=mode, revision=revision):
                verdict, history = self._live_history(
                    held_spec(mode, revision), f"n9-held-{mode}-{revision}."
                )
                print(
                    f"n9-held-{mode}-{revision} read_fd saw the write: "
                    f"{saw_write(history, 'f')}"
                )
                summary = _summary(verdict)
                self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))
                self.assertEqual(
                    "satisfied", _judged(verdict).get(("fd.stale-read", "f"))
                )

    def test_a_descriptor_whose_open_failed_ends_in_a_verdict(self) -> None:
        """No harness failure when the open's path is gone."""
        for generated in (False, True):
            spec = removed_open_spec(generated_shape=generated)
            opened, read = ("h0", "h1") if generated else ("o", "f")
            with self.subTest(generated_shape=generated):
                verdict, history = self._live_history(
                    spec, f"n9-removed-open-{int(generated)}."
                )
                open_errno = descriptor_errno(history, opened)
                print(
                    f"n9-removed-open-{int(generated)} open errno: {open_errno}, "
                    f"read_fd errno: {descriptor_errno(history, read)}"
                )
                if not generated:
                    self.assertEqual(errno.ENOENT, open_errno)
                if open_errno is not None:
                    self.assertEqual(errno.EBADF, descriptor_errno(history, read))
                summary = _summary(verdict)
                self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))

    def test_generated_plain_descriptor_specs_are_accepted(self) -> None:
        for spec in plain_descriptor_specs(3):
            with self.subTest(seed=spec.seed):
                verdict = self._live(spec, f"n9-plain-held-{spec.seed}.")
                summary = _summary(verdict)
                self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))
                self.assertIn(
                    "satisfied",
                    {
                        status
                        for (rule, _scope), status in _judged(verdict).items()
                        if rule == "fd.stale-read"
                    },
                )

    def test_generated_differential_specs_are_accepted(self) -> None:
        later = [differential_spec(seed) for seed in range(4, 80)]
        specs = [
            *differential_specs(4),
            *_first(later, "read_fd", 2),
            *_first(later, "readdir", 2),
        ]
        for spec in specs:
            with self.subTest(seed=spec.seed):
                verdict = self._live(spec, f"n9-differential-{spec.seed}.")
                summary = _summary(verdict)
                self.assertEqual(checker.ACCEPT, verdict.kind, json.dumps(summary))
                compared = [
                    key
                    for key in _judged(verdict)
                    if key[0].startswith("differential.")
                ]
                print(f"n9-differential-{spec.seed} compared: {len(compared)}")
                self.assertGreater(len(compared), 0)
