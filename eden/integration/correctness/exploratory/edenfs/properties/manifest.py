# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""The `manifest` property: a presence probe's per-path presence against shared effects.

A `manifest_presence` probe reports, for every path the fixture
manifests delete and add between two revisions, whether FUSE sees it.
Each listed path is one observation of its `path` cell inside the
probe's window (present: a file or a directory; absent: absent), at its
own point: no snapshot claim. The engine decides the probe over every
explanation of the run's contract effects, so a probe after a finished
checkout to its target revision requires deleted paths absent and added
paths present, and a probe overlapping a checkout's window admits any
contract-permitted per-path progress.

Listed paths the contracts does not track (an untouched generated path) are
observed through their equivalence class's representative, whose
contract behavior is identical. A probe whose listed path sets are not
the manifests' diff is not evaluated: its result does not say what the
schema promises.
"""

from __future__ import annotations

from collections.abc import Mapping

from eden.integration.correctness.exploratory.core.effects import (
    ABSENT,
    eq,
    Expect,
    normalize_path,
    Observation,
    of_class,
    Window,
)
from eden.integration.correctness.exploratory.core.property import (
    CheckInput,
    checkout_subject,
    CollectorSink,
    CoverageObligation,
    PropertyDef,
    RuleDef,
    SubjectKind,
)
from eden.integration.correctness.exploratory.edenfs.contracts.checkout import (
    checkout_revisions,
    named_paths,
    relevant_paths,
)
from eden.integration.correctness.exploratory.edenfs.contracts.model import path_cell
from eden.integration.correctness.exploratory.edenfs.encoding import manifest_diff
from eden.integration.correctness.exploratory.edenfs.fixture import DIRECTORY, FILE
from eden.integration.correctness.exploratory.edenfs.properties.common import (
    eden_pinned,
    judged,
)

MANIFEST = "manifest"
CHECKOUT_MISMATCH = f"{MANIFEST}.checkout-mismatch"
UNEXPECTED_PATHS = f"{MANIFEST}.unexpected-paths"
COMPARED = "manifest.paths-compared"
PROBE = "manifest_presence"
_LISTS = ("deleted_presence", "added_presence")
CONSUMES = frozenset(
    (PROBE, f"{name}.{member}") for name in _LISTS for member in ("path", "present")
)
_PRESENT = of_class({FILE, DIRECTORY})


def _part(index: int) -> str:
    return f"manifest:{index}"


def collect_manifest(check_input: CheckInput, sink: CollectorSink) -> None:
    """Judge every `ok` presence probe of the run, in spec order."""
    spec = check_input.spec
    pinned = eden_pinned(spec, check_input.pinned)
    relevant = frozenset(relevant_paths(spec, pinned))
    revisions = checkout_revisions(spec, pinned)
    named = named_paths(spec)
    for recorded in check_input.facts.actions_of(PROBE):
        if not judged(recorded):
            continue
        result = recorded.result
        assert result is not None
        assert recorded.invoked is not None and recorded.completed is not None
        ref = sink.obligation(
            CHECKOUT_MISMATCH, recorded.id, subject=checkout_subject(recorded.id)
        )
        deleted, added, _changed = manifest_diff(
            pinned.fixture.layout,
            recorded.payload["from_revision"],
            recorded.payload["to_revision"],
        )
        listed = {
            name: {
                normalize_path(item["path"]): item["present"]
                for item in result.get(name)
            }
            for name in _LISTS
        }
        expected = {"deleted_presence": deleted, "added_presence": added}
        if any(
            set(listed[name]) != {normalize_path(p) for p in expected[name]}
            for name in _LISTS
        ):
            sink.not_evaluated(
                ref,
                UNEXPECTED_PATHS,
                f"{recorded.id} lists other paths than the manifests' diff",
            )
            continue
        presence = {
            path: present for name in _LISTS for path, present in listed[name].items()
        }
        stand_ins = pinned.fixture.representatives(
            (path for path in presence if path not in relevant), revisions, named
        )
        observed: dict[tuple[str, bool], None] = {}
        for path, present in sorted(presence.items()):
            cell_path = path if path in relevant else stand_ins.get(path)
            if cell_path is None:
                sink.not_evaluated(
                    ref,
                    UNEXPECTED_PATHS,
                    f"{recorded.id} lists {path}, which no contract class covers",
                )
                break
            observed[(cell_path, present)] = None
        else:
            window = Window(recorded.invoked, recorded.completed)
            for index, (cell_path, present) in enumerate(observed):
                sink.observe(
                    Observation(
                        id=f"o:{recorded.id}:{_part(index)}",
                        action=recorded.id,
                        cells=(path_cell(cell_path),),
                        expect=Expect(((_PRESENT if present else eq(ABSENT),),)),
                        window=window,
                        channel="fuse",
                        obligations=(ref,),
                    )
                )
            sink.covered(COMPARED, len(presence))
            if not observed:
                sink.satisfied(ref)


MANIFEST_RULES: Mapping[str, RuleDef] = {
    CHECKOUT_MISMATCH: RuleDef(
        id=CHECKOUT_MISMATCH,
        statement="A manifest presence probe's per-path presence holds at a point "
        "inside its window: after a finished conflict-free checkout every path the "
        "manifests delete is absent and every path they add is present.",
        subject_kind=SubjectKind.CHECKOUT,
    ),
}

MANIFEST_PROPERTY = PropertyDef(
    id=MANIFEST,
    collect=collect_manifest,
    rules=MANIFEST_RULES,
    coverage=(CoverageObligation(COMPARED, "Probe paths observed by the engine"),),
    not_evaluated_reasons={
        UNEXPECTED_PATHS: "A probe listing other paths than the manifests' diff, or "
        "a path no contract class covers",
    },
    consumes=CONSUMES,
)
