# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import unittest
from collections import defaultdict
from pathlib import PurePosixPath
from typing import Any

from eden.integration.correctness.exploratory.edenfs.encoding import (
    encode_committed,
    initial_content,
    SCATTER_PAYLOAD,
)
from eden.integration.correctness.exploratory.edenfs.fixture import (
    CHURN_FILES,
    DIRECTORY,
    DIRECTORY_ENTRY,
    Entry,
    FILE,
    generated_index,
    GeneratedFixture,
    in_layout,
    PathClass,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import CountingRepo
from eden.integration.correctness.repo import (
    GeneratedRepo,
    RepoShape,
    TypeChange,
    WideDir,
)

_R0 = "tree/registers/r0.reg"
_R1 = "tree/registers/r1.reg"


def _brute_entries(fixture: GeneratedFixture, revision: int) -> dict[str, str]:
    """Return every path's entry kind at a revision by enumerating the layout."""
    layout = fixture.layout
    files = {file.path for file in layout.files(revision)}
    files |= set(fixture.registers) | set(CHURN_FILES)
    files -= {path for path in fixture.registers if layout.is_directory(path, revision)}
    kinds = {path: FILE for path in files}
    for path in list(files):
        for parent in PurePosixPath(path).parents:
            if str(parent) != ".":
                kinds[str(parent)] = DIRECTORY
    return kinds


def _fixture(layout: GeneratedRepo, dirty: int = 0) -> GeneratedFixture:
    return GeneratedFixture(layout, (_R0, _R1), dirty)


class PointQueryTest(unittest.TestCase):
    def test_entry_kinds_match_the_enumerated_layout(self) -> None:
        layouts = (
            GeneratedRepo(
                4, 450, 5, type_changes=(TypeChange(_R1, 2), TypeChange(_R1, 4))
            ),
            GeneratedRepo(5, 260, 4, repo_shape=RepoShape.W200K),
            GeneratedRepo(6, 20, 3, wide_dir=WideDir("tree/wide", 5, True)),
        )
        for layout in layouts:
            fixture = _fixture(layout)
            every = {
                path
                for revision in range(layout.commit_count)
                for path in _brute_entries(fixture, revision)
            }
            probes = every | {
                "tree/a0/999",
                "tree/a2",
                "tree/a0/r99",
                "nope",
                "tree/registers/x",
            }
            for revision in range(layout.commit_count):
                expected = _brute_entries(fixture, revision)
                for path in sorted(probes):
                    entry = fixture.committed(path, revision)
                    with self.subTest(
                        shape=layout.repo_shape, revision=revision, path=path
                    ):
                        self.assertEqual(
                            expected.get(path), None if entry is None else entry.kind
                        )

    def test_file_bytes_and_modes(self) -> None:
        layout = GeneratedRepo(4, 600, 4, wide_dir=WideDir("tree/wide", 3, True))
        fixture = _fixture(layout)
        self.assertEqual(
            Entry(FILE, encode_committed(4, _R0, 2)), fixture.committed(_R0, 2)
        )
        self.assertNotEqual(fixture.committed(_R0, 1), fixture.committed(_R0, 2))
        self.assertEqual(
            Entry(FILE, initial_content(4, CHURN_FILES[0])),
            fixture.committed(CHURN_FILES[0], 3),
        )
        executable = layout.file_at(502, 3)
        self.assertEqual(
            Entry(FILE, layout.content(executable, 3), True),
            fixture.committed(executable.path, 3),
        )
        wide = next(layout.wide_files())
        self.assertNotEqual(
            fixture.committed(wide.path, 0), fixture.committed(wide.path, 1)
        )
        self.assertEqual(DIRECTORY_ENTRY, fixture.committed("tree/registers", 0))

    def test_indexes_invert_the_path_grammar(self) -> None:
        for shape in (RepoShape.S10K, RepoShape.W200K):
            layout = GeneratedRepo(1, 300, 4, repo_shape=shape)
            for index in (0, 5, 96, 97, 299):
                for revision in range(4):
                    path = layout.file_at(index, revision).path
                    with self.subTest(shape=shape, index=index, revision=revision):
                        self.assertEqual(index, generated_index(layout, path))
                        self.assertTrue(in_layout(layout, path, revision))
            for path in (
                "tree/a0/000/00300.dat",
                "tree/a1/000/00000.dat",
                _R0,
                "x.dat",
            ):
                with self.subTest(shape=shape, path=path):
                    self.assertIsNone(generated_index(layout, path))

    def test_starting_dirt_covers_the_scattered_tip_paths(self) -> None:
        layout = GeneratedRepo(4, 300, 3)
        fixture = _fixture(layout, dirty=3)
        tip = 2
        for index in range(5):
            path = layout.file_at(index, tip).path
            with self.subTest(index=index):
                # Setup overwrites the first 64 bytes in place; the committed
                # file's trailing newline survives.
                expected = SCATTER_PAYLOAD + b"\n" if index < 3 else None
                self.assertEqual(expected, fixture.dirt(path))
        old = layout.file_at(1, 0).path
        self.assertIsNone(fixture.dirt(old))
        dirty_path = layout.file_at(0, tip).path
        self.assertEqual(
            Entry(FILE, SCATTER_PAYLOAD + b"\n", layout.file_at(0, tip).executable),
            fixture.initial(dirty_path),
        )
        self.assertEqual(fixture.committed(_R0, tip), fixture.initial(_R0))

    def test_point_queries_do_not_enumerate_one_million_files(self) -> None:
        layout = CountingRepo(9, 1_000_000, 4)
        fixture = GeneratedFixture(layout, (_R0,), 1000)
        paths = [layout.file_at(index, 3).path for index in (0, 999_999, 524_287)]
        layout.calls.clear()
        for path in (*paths, _R0, "tree/a0/r2", "tree/a1/123", "tree/registers/new"):
            fixture.initial(path)
        self.assertLessEqual(layout.calls["file_at"], 200)


def _brute_classes(
    fixture: GeneratedFixture, revisions: set[int], exclude: frozenset[str]
) -> dict[tuple[Any, ...], list[tuple[int, str]]]:
    layout = fixture.layout
    points = sorted(revisions | {fixture.initial_revision})
    groups: dict[tuple[Any, ...], list[tuple[int, str]]] = defaultdict(list)
    for index in range(layout.file_count):
        paths = {layout.file_at(index, revision).path for revision in points}
        for path in paths:
            if path in exclude:
                continue
            presence = tuple(
                layout.file_at(index, revision).path == path for revision in points
            )
            key = (presence, index % 251 == 0, index < fixture.scatter_dirty)
            groups[key].append((index, path))
    return groups


class ClassTest(unittest.TestCase):
    def test_classes_match_the_enumerated_partition(self) -> None:
        layout = GeneratedRepo(4, 30_000, 7)
        fixture = _fixture(layout, dirty=500)
        exclude = frozenset(
            {
                layout.file_at(0, 6).path,
                layout.file_at(97, 6).path,
                layout.file_at(251 * 3, 0).path,
            }
        )
        for revisions in ({6}, {0, 3}, {1, 2, 5}):
            with self.subTest(revisions=revisions):
                expected = _brute_classes(fixture, revisions, exclude)
                classes = fixture.classes(revisions, exclude)
                self.assertEqual(
                    sorted(
                        (min(members)[1], len(members)) for members in expected.values()
                    ),
                    sorted((item.representative, item.count) for item in classes),
                )
                self.assertFalse({item.representative for item in classes} & exclude)

    def test_classes_cost_does_not_grow_with_one_million_files(self) -> None:
        layout = CountingRepo(9, 1_000_000, 5)
        fixture = GeneratedFixture(layout, (_R0,), 10)
        touched = frozenset({layout.file_at(0, 4).path})
        classes = fixture.classes({0, 2}, touched)
        self.assertEqual(
            1_000_000 * 1
            + sum(1 for i in range(1, 5) for _ in range(i - 1, 1_000_000, 97))
            - 1,
            sum(item.count for item in classes),
        )
        self.assertIsInstance(classes[0], PathClass)
        self.assertLessEqual(layout.calls["file_at"], 97 * 8 * 2 + 50)
