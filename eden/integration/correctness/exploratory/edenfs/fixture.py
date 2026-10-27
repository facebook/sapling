# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Sparse, indexed queries over the generated EdenFS fixture.

A RunSpec pins a deterministic fixture: the generated layout of
`correctness/repo.py`, the reserved register and churn paths, optional
type changes and a wide directory, and the starting dirt that setup
scatters over the tip revision. Every query here answers one path at one
revision in O(1) (a generated directory costs one bucket of at most 100
indexes), and equivalence classes of untouched generated paths are
computed arithmetically per replacement residue. Nothing enumerates the
layout, so the cost of a query never depends on `file_count`.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from pathlib import PurePosixPath
from typing import Any

from eden.integration.correctness.exploratory.core.spec_types import RunSpec
from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    initial_content,
    SCATTER_PAYLOAD,
)
from eden.integration.correctness.repo import (
    GeneratedFile,
    GeneratedRepo,
    RepoShape,
    TypeChange,
    WideDir,
)

CHURN_FILES = ("tree/churn/c0", "tree/churn/c1", "tree/churn/c2", "tree/churn/c3")
FILE = "file"
DIRECTORY = "dir"
REPLACEMENT_STRIDE = 97
EXECUTABLE_STRIDE = 251
FILES_PER_DIRECTORY = 100
_CLASS_PERIOD = REPLACEMENT_STRIDE * EXECUTABLE_STRIDE


@dataclass(frozen=True)
class Entry:
    """One committed or initial tree entry: a file with bytes and mode, or a directory."""

    kind: str
    data: bytes = b""
    executable: bool = False


DIRECTORY_ENTRY = Entry(DIRECTORY, executable=True)


@dataclass(frozen=True)
class PathClass:
    """Untouched generated paths whose contract behavior is identical.

    `representative` is the class's smallest-index path; `count` is how
    many paths the class holds. Members share their presence at every
    relevant revision, their executable bit, and their starting dirt.
    """

    representative: str
    count: int


def layout_for(spec: RunSpec) -> GeneratedRepo:
    """Return the generated layout described by the spec's repository and setup."""
    target = spec.environment.target
    repo = target["repository"]
    wide = target["setup_profile"].get("wide_dir")
    return GeneratedRepo(
        spec.seed,
        repo["file_count"],
        repo["commit_count"],
        wide_dir=None if wide is None else WideDir(**wide),
        type_changes=tuple(
            TypeChange(**change) for change in repo.get("type_changes", ())
        ),
        blob_size=repo.get("blob_size"),
    )


def _digits(text: str) -> int | None:
    return int(text) if text.isascii() and text.isdigit() else None


def generated_index(layout: GeneratedRepo, path: str) -> int | None:
    """Return the stable index whose generated path is `path` at some revision.

    The basename carries the index, so this inverts the path grammar and
    confirms it against the first and last revisions: an index's path
    changes at most once, when entering its replacement revision.
    """
    name = PurePosixPath(path).name
    if not name.endswith(".dat"):
        return None
    index = _digits(name[: -len(".dat")].rsplit("-", 1)[-1])
    if index is None or index >= layout.file_count:
        return None
    tip = layout.commit_count - 1
    for revision in (0, tip):
        if layout.file_at(index, revision).path == path:
            return index
    return None


def in_layout(layout: GeneratedRepo, path: str, revision: int) -> bool:
    """Return whether the generated layout holds file `path` at `revision`."""
    index = generated_index(layout, path)
    return index is not None and layout.file_at(index, revision).path == path


def _replacement(index: int) -> int:
    return index % REPLACEMENT_STRIDE + 1


class GeneratedFixture:
    """Point queries over one run's pinned fixture; no query enumerates the layout.

    `registers` hold revision-specific committed images; churn paths hold
    revision-stable images; every other file is generated. The fixture
    starts at the tip revision with the first `scatter_dirty` generated
    paths dirtied.
    """

    def __init__(
        self,
        layout: GeneratedRepo,
        registers: Iterable[str],
        scatter_dirty: int = 0,
    ) -> None:
        self.layout = layout
        self.registers = frozenset(registers)
        self.scatter_dirty = scatter_dirty
        self.initial_revision = layout.commit_count - 1
        self._changing = {change.path for change in layout.type_changes}
        self._stable_directories = frozenset(self._ancestors())

    @classmethod
    def for_spec(
        cls, spec: RunSpec, layout: GeneratedRepo | None = None
    ) -> GeneratedFixture:
        """Return the fixture a RunSpec pins, optionally over a supplied layout."""
        target = spec.environment.target
        return cls(
            layout_for(spec) if layout is None else layout,
            target["repository"]["registers"],
            target["setup_profile"]["scatter_dirty"],
        )

    def _ancestors(self) -> set[str]:
        """Return every directory that holds a reserved path at every revision."""
        roots = [*self.registers, *CHURN_FILES]
        wide = self.layout.wide_dir
        if wide is not None:
            roots.append(f"{wide.name}/entry")
        found: set[str] = set()
        for path in roots:
            found.update(str(parent) for parent in PurePosixPath(path).parents)
        found.discard(".")
        return found

    def _wide_entry(self, path: str) -> GeneratedFile | None:
        wide = self.layout.wide_dir
        if wide is None or str(PurePosixPath(path).parent) != wide.name:
            return None
        first = next(self.layout.wide_files())
        suffix = first.path[len(wide.name) + 1 + 6 :]
        name = PurePosixPath(path).name
        index = _digits(name[:6])
        if index is None or index >= wide.entries or name[6:] != suffix:
            return None
        return GeneratedFile(path=path, executable=False)

    def _type_change_child(self, path: str, revision: int) -> GeneratedFile | None:
        parent = str(PurePosixPath(path).parent)
        if parent not in self._changing:
            return None
        child = self.layout.type_change_child(parent)
        if child.path != path or not self.layout.is_directory(parent, revision):
            return None
        return child

    def _generated_file(self, path: str, revision: int) -> Entry | None:
        index = generated_index(self.layout, path)
        if index is None:
            return None
        generated = self.layout.file_at(index, revision)
        if generated.path != path:
            return None
        return Entry(
            FILE, self.layout.content(generated, revision), generated.executable
        )

    def committed(self, path: str, revision: int) -> Entry | None:
        """Return the entry a fixture revision holds at `path`, or None when absent."""
        if path in self._changing and self.layout.is_directory(path, revision):
            return DIRECTORY_ENTRY
        if path in self.registers:
            return Entry(FILE, encode_committed(self.layout.seed, path, revision))
        if path in CHURN_FILES:
            return Entry(FILE, initial_content(self.layout.seed, path))
        special = self._wide_entry(path) or self._type_change_child(path, revision)
        if special is not None:
            return Entry(FILE, self.layout.content(special, revision))
        if self._is_directory(path, revision):
            return DIRECTORY_ENTRY
        return self._generated_file(path, revision)

    def _is_directory(self, path: str, revision: int) -> bool:
        """Return whether `path` is a directory of the fixture at `revision`."""
        if path in self._stable_directories:
            return True
        parts = path.split("/")
        if parts[0] != "tree" or len(parts) > 3:
            return False
        if len(parts) == 1:
            return True
        actor = {"a0": 0, "a1": 1}.get(parts[1])
        if actor is None or actor >= self.layout.file_count:
            return False
        if len(parts) == 2:
            return True
        return self._is_generated_directory(actor, parts[2], revision)

    def _is_generated_directory(self, actor: int, name: str, revision: int) -> bool:
        if self.layout.repo_shape is RepoShape.W200K:
            return name == "wide"
        if name.startswith("r") and _digits(name[1:]) is not None:
            replacement = int(name[1:])
            first = replacement - 1
            if first % 2 != actor:
                first += REPLACEMENT_STRIDE
            return (
                1 <= replacement <= min(revision, REPLACEMENT_STRIDE)
                and first < self.layout.file_count
            )
        bucket = _digits(name)
        if bucket is None or len(name) != 3:
            return False
        start = bucket * FILES_PER_DIRECTORY
        stop = min(start + FILES_PER_DIRECTORY, self.layout.file_count)
        return any(
            index % 2 == actor and _replacement(index) > revision
            for index in range(start, stop)
        )

    def dirt(self, path: str) -> bytes | None:
        """Return the bytes setup scatters over `path`, or None when it stays clean."""
        index = generated_index(self.layout, path)
        if index is None or index >= self.scatter_dirty:
            return None
        if self.layout.file_at(index, self.initial_revision).path != path:
            return None
        committed = self.committed(path, self.initial_revision)
        tail = b"" if committed is None else committed.data[len(SCATTER_PAYLOAD) :]
        return SCATTER_PAYLOAD + tail

    def initial(self, path: str) -> Entry | None:
        """Return the entry at `path` when the run starts: committed tip plus dirt."""
        committed = self.committed(path, self.initial_revision)
        data = self.dirt(path)
        if data is None or committed is None:
            return committed
        return Entry(FILE, data, committed.executable)

    def classes(
        self, revisions: Iterable[int], exclude: frozenset[str] = frozenset()
    ) -> tuple[PathClass, ...]:
        """Return one class per distinct behavior of untouched generated paths.

        A generated index's old path exists before its replacement revision
        and its new path from that revision on, so paths whose presence at
        every one of `revisions` (and the initial revision) agrees behave
        alike. Classes
        also split by the executable bit and the starting dirt. Paths in
        `exclude` (touched by some action) belong to no class. The work is
        O(REPLACEMENT_STRIDE * (1 + len(exclude))), whatever `file_count` is.
        """
        points = sorted(set(revisions) | {self.initial_revision})
        excluded = Counter(
            signature
            for path in exclude
            if (signature := self._signature(path, points)) is not None
        )
        counts: Counter[tuple[Any, ...]] = Counter()
        best: dict[tuple[Any, ...], tuple[int, str]] = {}
        for residue in range(min(REPLACEMENT_STRIDE, self.layout.file_count)):
            for key, (low, high, count) in self._residue_classes(residue).items():
                for side in ("old", "new"):
                    signature = self._side_signature(residue, side, key, points)
                    if signature is None:
                        continue
                    counts[signature] += count
                    member = self._first_untouched(
                        residue, key[0], low, high, side, exclude
                    )
                    if member is not None and (
                        signature not in best or member < best[signature]
                    ):
                        best[signature] = member
        classes = [
            (best[signature], counts[signature] - excluded[signature])
            for signature in best
        ]
        return tuple(
            PathClass(path, count)
            for (_index, path), count in sorted(classes)
            if count > 0
        )

    def representatives(
        self,
        paths: Iterable[str],
        revisions: Iterable[int],
        exclude: frozenset[str] = frozenset(),
    ) -> dict[str, str]:
        """Return the representative of the untouched class each of `paths` belongs to.

        The classes are those `classes(revisions, exclude)` returns, plus
        one class of untouched generated paths absent at every relevant
        revision: an index's old path removed at a revision below every
        one of `revisions`. Nothing creates such a path, so its members
        behave alike whatever their executable bit or dirt. A path that is
        not an untouched generated path has no entry. The work is that of
        one `classes` call plus O(len(paths)).
        """
        revisions = tuple(revisions)
        points = sorted(set(revisions) | {self.initial_revision})
        by_signature = {
            self._signature(item.representative, points): item.representative
            for item in self.classes(revisions, exclude)
        }
        absent: str | None = None
        found = {}
        for path in paths:
            if path in exclude:
                continue
            representative = by_signature.get(self._signature(path, points))
            if representative is None and self._never_present(path, points):
                if absent is None:
                    absent = self._first_never_present(points[0], exclude)
                representative = absent
            if representative is not None:
                found[path] = representative
        return found

    def _never_present(self, path: str, points: list[int]) -> bool:
        """Return whether generated `path` is an old path removed before every point."""
        index = generated_index(self.layout, path)
        return (
            index is not None
            and self.layout.file_at(index, 0).path == path
            and _replacement(index) <= points[0]
        )

    def _first_never_present(self, first: int, exclude: frozenset[str]) -> str | None:
        """Return the smallest-index untouched old path replaced at or before `first`.

        The work is O(REPLACEMENT_STRIDE * (1 + len(exclude))).
        """
        best: tuple[int, str] | None = None
        for residue in range(min(first, REPLACEMENT_STRIDE, self.layout.file_count)):
            for exec_bit in (False, True):
                member = self._first_untouched(
                    residue, exec_bit, 0, self.layout.file_count, "old", exclude
                )
                if member is not None and (best is None or member < best):
                    best = member
        return None if best is None else best[1]

    def _side_signature(
        self, residue: int, side: str, key: tuple[bool, bool], points: list[int]
    ) -> tuple[Any, ...] | None:
        """Return the class signature of one side of a residue, or None when absent.

        The signature is the side's presence at every relevant revision plus
        the executable bit and starting dirt.
        """
        replacement = residue + 1
        if side == "old":
            presence = tuple(point < replacement for point in points)
        else:
            presence = tuple(point >= replacement for point in points)
        if not any(presence):
            return None
        return (presence, *key)

    def _signature(self, path: str, points: list[int]) -> tuple[Any, ...] | None:
        """Return the class signature of one generated path, or None for any other path."""
        index = generated_index(self.layout, path)
        if index is None:
            return None
        side = "old" if self.layout.file_at(index, 0).path == path else "new"
        key = (index % EXECUTABLE_STRIDE == 0, index < self.scatter_dirty)
        return self._side_signature(index % REPLACEMENT_STRIDE, side, key, points)

    def _first_untouched(
        self,
        residue: int,
        exec_bit: bool,
        low: int,
        high: int,
        side: str,
        exclude: frozenset[str],
    ) -> tuple[int, str] | None:
        """Return the smallest member of a residue class whose side path is untouched."""
        revision = 0 if side == "old" else residue + 1
        for index in _members(residue, exec_bit, low, high):
            path = self.layout.file_at(index, revision).path
            if path not in exclude:
                return index, path
        return None

    def _residue_classes(
        self, residue: int
    ) -> dict[tuple[bool, bool], tuple[int, int, int]]:
        """Split one replacement residue's indexes by executable bit and dirt.

        Returns each non-empty class's index range and size, by arithmetic
        over the progression `residue + 97 * m`.
        """
        limit = self.layout.file_count
        dirty_limit = min(self.scatter_dirty, limit)
        classes: dict[tuple[bool, bool], tuple[int, int, int]] = {}
        for exec_bit in (False, True):
            for dirty in (False, True):
                low, high = (0, dirty_limit) if dirty else (dirty_limit, limit)
                count = _class_size(residue, exec_bit, low, high)
                if count > 0:
                    classes[(exec_bit, dirty)] = (low, high, count)
        return classes


def _progression_count(start: int, step: int, limit: int) -> int:
    """Return how many of `start, start + step, ...` are below `limit`."""
    return 0 if start >= limit else (limit - 1 - start) // step + 1


def _executable_start(residue: int) -> int:
    """Return the smallest index congruent to `residue` mod 97 and to 0 mod 251."""
    inverse = pow(EXECUTABLE_STRIDE, -1, REPLACEMENT_STRIDE)
    return EXECUTABLE_STRIDE * (residue * inverse % REPLACEMENT_STRIDE)


def _class_size(residue: int, exec_bit: bool, low: int, high: int) -> int:
    """Return how many of a residue's indexes in `[low, high)` have the executable bit.

    `exec_bit` selects the executable indexes (multiples of 251) or the rest.
    """
    executables = _range_count(_executable_start(residue), _CLASS_PERIOD, low, high)
    if exec_bit:
        return executables
    return _range_count(residue, REPLACEMENT_STRIDE, low, high) - executables


def _members(residue: int, exec_bit: bool, low: int, high: int) -> Iterator[int]:
    """Yield a residue's indexes in `[low, high)` with the given executable bit, in order."""
    if exec_bit:
        index = _first_at_least(_executable_start(residue), _CLASS_PERIOD, low)
        step = _CLASS_PERIOD
    else:
        index = _first_at_least(residue, REPLACEMENT_STRIDE, low)
        step = REPLACEMENT_STRIDE
    while index < high:
        if exec_bit or index % EXECUTABLE_STRIDE != 0:
            yield index
        index += step


def _first_at_least(start: int, step: int, low: int) -> int:
    """Return the smallest member of `start, start + step, ...` that is at least `low`."""
    if start >= low:
        return start
    return start + -(-(low - start) // step) * step


def _range_count(start: int, step: int, low: int, high: int) -> int:
    """Return how many of `start, start + step, ...` lie in `[low, high)`."""
    if high <= low:
        return 0
    return _progression_count(start, step, high) - _progression_count(start, step, low)
