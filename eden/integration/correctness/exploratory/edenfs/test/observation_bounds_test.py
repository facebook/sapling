# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Tests for the static complete-observation event bounds."""

from __future__ import annotations

import json
import unittest
from pathlib import PurePosixPath
from typing import Any
from unittest import mock

from eden.integration.correctness.exploratory.core.history import (
    Event,
    event_line,
    MAX_EVENT_BYTES,
    OK,
)
from eden.integration.correctness.exploratory.core.runspec import (
    Action,
    from_json,
    RunSpec,
    SpecError,
)
from eden.integration.correctness.exploratory.edenfs.encoding import manifest_diff
from eden.integration.correctness.exploratory.edenfs.fixture import (
    CHURN_FILES,
    layout_for,
)
from eden.integration.correctness.exploratory.edenfs.observation_bounds import (
    observation_bound,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import VOCAB_TARGET
from eden.integration.correctness.repo import GeneratedRepo

_R0 = "tree/registers/r0.reg"
_R1 = "tree/registers/r1.reg"
_WIDE = "tree/wide"


def _spec(
    actions: dict[str, dict[str, Any]],
    *,
    file_count: int = 1000,
    commit_count: int = 4,
    scatter_dirty: int = 0,
    wide_entries: int | None = None,
    reference_clone: bool = False,
    type_changes: tuple[dict[str, Any], ...] = (),
) -> RunSpec:
    profile: dict[str, Any] = {"materialize": False, "scatter_dirty": scatter_dirty}
    if wide_entries is not None:
        profile["wide_dir"] = {
            "name": _WIDE,
            "entries": wide_entries,
            "churn_all": True,
        }
    if reference_clone:
        profile["reference_clone"] = True
    repository: dict[str, Any] = {
        "file_count": file_count,
        "commit_count": commit_count,
        "registers": [_R0, _R1],
    }
    if type_changes:
        repository["type_changes"] = list(type_changes)
    text = json.dumps(
        {
            "schema": "edenfs-exploratory-run",
            "schema_version": 1,
            "seed": 4,
            "target": "edenfs",
            "environment": {
                "target": {
                    "repository": repository,
                    "channel": "fuse",
                    "case_sensitive": True,
                    "fixture_source": "backing",
                    "setup_profile": profile,
                }
            },
            "actors": [{"id": "a", "actions": list(actions)}],
            "actions": actions,
        }
    )
    return from_json(text, VOCAB_TARGET)


def _op(kind: str, **payload: Any) -> dict[str, Any]:
    return {"kind": kind, "deadline": "operation", **payload}


def _event_bytes(action_id: str, facts: dict[str, Any]) -> int:
    return len(
        event_line(Event(OK, 12.5, {"action": action_id, "actor": "a", **facts}))
    )


def _all_paths(layout: GeneratedRepo) -> set[str]:
    """Return every generated path of every revision, by enumeration."""
    return {
        generated.path
        for revision in range(layout.commit_count)
        for generated in layout.files(revision)
    }


class BoundCoversTheLargestObservationTest(unittest.TestCase):
    """Each bound is at least the largest event its observation can record."""

    def test_status_bound_covers_every_differing_path(self) -> None:
        spec = _spec(
            {
                "m1": _op("mkdir", path="tree/registers/made"),
                "s1": _op("status", channel="sl", revision=0),
            },
            scatter_dirty=1000,
            wide_entries=50,
            type_changes=({"path": _R1, "at_revision": 2},),
        )
        layout = layout_for(spec)
        deleted, added, _changed = manifest_diff(layout, 0, 3)
        paths = (
            {layout.file_at(index, 3).path for index in range(1000)}
            | deleted
            | added
            | {_R0, _R1, f"{_R1}/entry.dat", "tree/registers/made", *CHURN_FILES}
            | {generated.path for generated in layout.wide_files()}
        )
        facts = {
            "status": [{"path": path, "code": "MODIFIED"} for path in sorted(paths)],
            "diff_paths": sorted(paths),
            "parent": "f" * 40,
        }
        bound = observation_bound(spec, spec.action("s1"))
        assert bound is not None
        self.assertLessEqual(_event_bytes("s1", facts), bound)
        # Not wildly loose either: within twice the largest real event.
        self.assertLess(bound, 2 * _event_bytes("s1", facts) + 2048)

    def test_listing_bound_covers_every_child_of_a_generated_directory(self) -> None:
        spec = _spec(
            {"d2": _op("readdir", path=_WIDE, channel="fuse")},
            file_count=5000,
            wide_entries=300,
        )
        layout = layout_for(spec)
        children = {
            PurePosixPath(path).relative_to("tree/a0").parts[0]
            for path in _all_paths(layout)
            if path.startswith("tree/a0/")
        }
        # No spec may list `tree/a0` (it is not a model directory); bound it directly.
        listing = Action(
            "dx", "readdir", "operation", payload={"path": "tree/a0", "channel": "fuse"}
        )
        bound = observation_bound(spec, listing)
        assert bound is not None
        facts = {"channel": "fuse", "entries": sorted(children)}
        self.assertLessEqual(_event_bytes("dx", facts), bound)
        wide = sorted(
            PurePosixPath(generated.path).name for generated in layout.wide_files()
        )
        wide_bound = observation_bound(spec, spec.action("d2"))
        assert wide_bound is not None
        self.assertLessEqual(
            _event_bytes("d2", {"channel": "fuse", "entries": wide}), wide_bound
        )

    def test_manifest_and_journal_bounds_cover_their_largest_events(self) -> None:
        spec = _spec(
            {
                "j1": _op("journal_window", since_sequence=0),
                "m1": _op("manifest_presence", from_revision=0, to_revision=3),
                "n1": _op("rename", source=_R0, destination="tree/registers/moved"),
            },
            scatter_dirty=200,
        )
        layout = layout_for(spec)
        deleted, added, _changed = manifest_diff(layout, 0, 3)
        manifest = {
            "deleted_presence": [
                {"path": path, "present": False} for path in sorted(deleted)
            ],
            "added_presence": [
                {"path": path, "present": False} for path in sorted(added)
            ],
        }
        bound = observation_bound(spec, spec.action("m1"))
        assert bound is not None
        self.assertLessEqual(_event_bytes("m1", manifest), bound)
        changed = sorted(
            {layout.file_at(index, 3).path for index in range(200)}
            | {_R0, _R1, "tree/registers/moved", *CHURN_FILES}
        )
        journal = {
            "mount_generation": 1,
            "sequence_number": 10**6,
            "since_sequence": 0,
            "changed_paths": [
                {"path": path, "existed_before": True, "existed_after": True}
                for path in changed
            ],
            "truncated": False,
            "lost_changes": [],
        }
        journal_bound = observation_bound(spec, spec.action("j1"))
        assert journal_bound is not None
        self.assertLessEqual(_event_bytes("j1", journal), journal_bound)

    def test_a_differential_bound_carries_the_reference_copy(self) -> None:
        actions = {"s1": _op("status", channel="sl")}
        single = observation_bound(_spec(actions), _spec(actions).action("s1"))
        paired_spec = _spec(actions, reference_clone=True)
        paired = observation_bound(paired_spec, paired_spec.action("s1"))
        assert single is not None and paired is not None
        self.assertGreaterEqual(paired, 2 * single)
        read = Action(
            "r", "read", "operation", payload={"path": _R0, "channel": "fuse"}
        )
        self.assertIsNone(observation_bound(paired_spec, read))


class OversizedSpecRefusalTest(unittest.TestCase):
    def _refused(self, **kwargs: Any) -> str:
        with self.assertRaises(SpecError) as caught:
            _spec(**kwargs)
        self.assertIn(
            f"over the {MAX_EVENT_BYTES}-byte event limit", str(caught.exception)
        )
        return str(caught.exception)

    def test_differential_status_over_many_dirty_paths_is_refused(self) -> None:
        actions = {"s1": _op("status", channel="sl")}
        self._refused(
            actions=actions,
            file_count=1_000_000,
            scatter_dirty=5000,
            reference_clone=True,
        )
        _spec(actions, file_count=1_000_000, scatter_dirty=100, reference_clone=True)

    def test_a_large_manifest_probe_is_refused_and_a_sparse_one_is_not(self) -> None:
        actions = {"m1": _op("manifest_presence", from_revision=0, to_revision=3)}
        message = self._refused(actions=actions, file_count=1_000_000)
        self.assertIn("manifest_presence 'm1'", message)
        _spec(actions, file_count=100_000)
        _spec({"s1": _op("status", channel="sl")}, file_count=1_000_000)

    def test_only_an_explicit_status_revision_counts_revision_differences(
        self,
    ) -> None:
        """A Thrift checkout leaves a later sl status listing dirt only."""
        thrift = {
            "c1": _op("checkout", revision=0, via="thrift", mode="normal"),
            "s1": _op("status", channel="sl"),
        }
        spec = _spec(thrift, file_count=1_000_000)
        bound = observation_bound(spec, spec.action("s1"))
        assert bound is not None
        self.assertLess(bound, 10_000)
        for channel in ("sl", "thrift"):
            with self.subTest(channel=channel):
                message = self._refused(
                    actions={"s1": _op("status", channel=channel, revision=0)},
                    file_count=1_000_000,
                )
                self.assertIn("status 's1'", message)

    def test_a_wide_listing_is_refused_past_the_limit(self) -> None:
        actions = {"d1": _op("readdir", path=_WIDE, channel="fuse")}
        self._refused(actions=actions, wide_entries=10_000)
        _spec(actions, wide_entries=2000)

    def test_validation_never_enumerates_the_layout(self) -> None:
        def refuse(*_args: Any, **_kwargs: Any) -> Any:
            raise AssertionError("validation enumerated the layout")

        actions = {
            "c1": _op("checkout", revision=0, via="thrift", mode="normal"),
            "d1": _op("readdir", path="tree/registers", channel="fuse"),
            "j1": _op("journal_window", since_sequence=0),
            "m1": _op("manifest_presence", from_revision=2, to_revision=3),
            "s1": _op("status", channel="sl"),
        }
        with (
            mock.patch.object(GeneratedRepo, "files", refuse),
            mock.patch.object(GeneratedRepo, "changed_indices", refuse),
        ):
            with self.assertRaises(SpecError):
                _spec(actions, file_count=1_000_000)
            _spec(actions, file_count=10_000)
