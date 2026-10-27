# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import hashlib
import unittest
from typing import Any, cast

from eden.integration.correctness.exploratory.edenfs.encoding import (
    content_token,
    encode_committed,
    encode_value,
    initial_content,
    is_reserved_writer,
    manifest_diff,
    REGISTER_SIZE,
)
from eden.integration.correctness.exploratory.edenfs.test.testing import CountingRepo
from eden.integration.correctness.repo import GeneratedRepo, RepoShape, TypeChange


def _brute_diff(
    layout: GeneratedRepo, old: int, new: int
) -> tuple[set[str], set[str], set[str]]:
    before = {layout.file_at(i, old).path for i in range(layout.file_count)}
    after = {layout.file_at(i, new).path for i in range(layout.file_count)}
    changed: set[str] = set()
    for change in layout.type_changes:
        to_directory = layout.is_directory(change.path, new)
        if layout.is_directory(change.path, old) == to_directory:
            continue
        changed.add(change.path)
        (after if to_directory else before).add(
            layout.type_change_child(change.path).path
        )
    return before - after, after - before, changed


class ValueEncodingTest(unittest.TestCase):
    def test_write_values_are_fixed_size_and_bounded(self) -> None:
        value = encode_value(7, "a", "w1")
        self.assertEqual(REGISTER_SIZE, len(value))
        self.assertTrue(value.startswith(b"REG1 seed=00000007 writer=a action=w1."))
        with self.assertRaisesRegex(ValueError, "too long"):
            encode_value(7, "a", "v" * REGISTER_SIZE)
        self.assertEqual(
            encode_value(7, "init", "tree/churn/c0"),
            initial_content(7, "tree/churn/c0"),
        )

    def test_value_fields_cannot_alias_each_other(self) -> None:
        """A separator inside a field would let two values encode alike."""
        for writer, value_id in (
            ("a action=b", "c"),
            ("a", "b action=c"),
            ("a b", "c"),
            ("a", "b\tc"),
            ("", "c"),
            ("a", ""),
        ):
            with self.subTest(writer=writer, value_id=value_id):
                with self.assertRaisesRegex(ValueError, "empty or ambiguous"):
                    encode_value(7, writer, value_id)
        self.assertNotEqual(encode_value(7, "a.b", "c"), encode_value(7, "a", "b.c"))

    def test_committed_images_are_unique_per_path_and_revision(self) -> None:
        paths = [f"tree/registers/r{index}.reg" for index in range(50)]
        paths.append("tree/registers/" + "deep/" * 200 + "r.reg")
        images = {
            encode_committed(3, path, revision)
            for path in paths
            for revision in range(98)
        }
        self.assertEqual(len(paths) * 98, len(images))
        self.assertEqual({REGISTER_SIZE}, {len(image) for image in images})
        identity = hashlib.sha256(paths[0].encode()).hexdigest()[:16]
        self.assertEqual(
            f"REG1 seed=00000003 writer=commit5 action={identity}".encode(),
            encode_committed(3, paths[0], 5).rstrip(b"."),
        )
        for bad in (-1, True, "1"):
            with self.subTest(revision=bad), self.assertRaises(ValueError):
                encode_committed(3, paths[0], cast(Any, bad))

    def test_reserved_writers_keep_writes_apart_from_fixture_images(self) -> None:
        for name in ("init", "commit", "commit0", "commitx"):
            self.assertTrue(is_reserved_writer(name))
        for name in ("a", "w1", "initial", "commi", "xcommit"):
            self.assertFalse(is_reserved_writer(name))
        writes = {
            encode_value(3, writer, value)
            for writer in ("a", "b")
            for value in ("w1", "w2")
        }
        committed = {
            encode_committed(3, "tree/registers/r0.reg", rev) for rev in range(4)
        }
        self.assertFalse(writes & committed)

    def test_content_tokens_are_sha256(self) -> None:
        self.assertEqual(hashlib.sha256(b"abc").hexdigest(), content_token(b"abc"))


class ManifestDiffTest(unittest.TestCase):
    def test_sparse_diff_equals_the_enumerated_diff(self) -> None:
        layouts = (
            GeneratedRepo(
                1, 500, 6, type_changes=(TypeChange("tree/registers/r0.reg", 2),)
            ),
            GeneratedRepo(2, 300, 98),
            GeneratedRepo(3, 400, 5, repo_shape=RepoShape.W200K),
        )
        for layout in layouts:
            for old in range(layout.commit_count):
                for new in (
                    0,
                    layout.commit_count - 1,
                    (old + 1) % layout.commit_count,
                ):
                    with self.subTest(layout=layout.repo_shape, old=old, new=new):
                        self.assertEqual(
                            _brute_diff(layout, old, new),
                            manifest_diff(layout, old, new),
                        )

    def test_diff_cost_tracks_changed_indexes_at_one_million_files(self) -> None:
        layout = CountingRepo(9, 1_000_000, 3, walks_changes=True)
        deleted, added, _changed = manifest_diff(layout, 0, 1)
        changed = len(range(0, 1_000_000, 97))
        self.assertEqual(changed, len(deleted))
        self.assertEqual(changed, len(added))
        self.assertEqual(2 * changed, layout.calls["file_at"])
