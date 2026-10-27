# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import subprocess
import unittest
from pathlib import Path
from typing import cast
from unittest import mock

from eden.integration.correctness.repo import (
    CommittedImages,
    create_backing_repo,
    DEFAULT_COMMIT_COUNT,
    DEFAULT_FILE_COUNT,
    default_file_count,
    generated_content,
    GeneratedRepo,
    RepoAuthor,
    RepoAuthorTimeout,
    RepoAuthorUntracked,
    RepoShape,
    TypeChange,
    WideDir,
)
from eden.integration.lib import hgrepo
from eden.test_support.temporary_directory import TempFileManager


def _sl_tracks_every_path(*args: str, **kwargs: object) -> str:
    """Mock `repo.hg`: `files` lists every path in its listfile, as after a full add."""
    if args[0] != "files":
        return ""
    listfile = Path(args[1].removeprefix("listfile:"))
    return "".join(
        line.removeprefix("path:") + "\n" for line in listfile.read_text().splitlines()
    )


def _images(paths: tuple[str, ...]) -> CommittedImages:
    """Return test images naming each path and revision."""
    return CommittedImages(
        paths=paths,
        content=lambda path, revision: f"{path}@{revision}\n".encode(),
    )


class GeneratedRepoTest(unittest.TestCase):
    def test_default_layout_has_ten_thousand_unique_paths(self) -> None:
        layout = GeneratedRepo(seed=7)
        paths = [entry.path for entry in layout.files(0)]

        self.assertEqual(DEFAULT_FILE_COUNT, len(paths))
        self.assertEqual(DEFAULT_FILE_COUNT, len(set(paths)))
        self.assertEqual(DEFAULT_COMMIT_COUNT, layout.commit_count)
        self.assertEqual(RepoShape.S10K, layout.repo_shape)

    def test_shape_defaults_name_adversarial_scale_cells(self) -> None:
        self.assertEqual(10_000, default_file_count(RepoShape.S10K))
        self.assertEqual(100_000, default_file_count(RepoShape.M100K))
        self.assertEqual(1_000_000, default_file_count(RepoShape.L1M))
        self.assertEqual(400_002, default_file_count(RepoShape.W200K))

    def test_content_is_exactly_a_function_of_seed_and_path(self) -> None:
        path = GeneratedRepo(seed=7, file_count=4).file_at(2, 0).path

        self.assertEqual(
            b"59a69b2441e34bae90b6be003c14192213787ba15a7d1f88025f5cf8633871ac\n",
            generated_content(7, path),
        )
        self.assertNotEqual(generated_content(7, path), generated_content(8, path))

    def test_every_revision_keeps_size_and_changes_paths(self) -> None:
        layout = GeneratedRepo(seed=1, file_count=32, commit_count=4)
        snapshots = [tuple(entry.path for entry in layout.files(i)) for i in range(4)]

        for snapshot in snapshots:
            self.assertEqual(32, len(snapshot))
            self.assertEqual(32, len(set(snapshot)))
        for before, after in zip(snapshots, snapshots[1:]):
            self.assertNotEqual(before, after)

    def test_wide_shape_keeps_actor_files_in_one_hot_directory(self) -> None:
        layout = GeneratedRepo(
            seed=1,
            file_count=8,
            commit_count=4,
            repo_shape=RepoShape.W200K,
        )
        snapshots = [tuple(entry.path for entry in layout.files(i)) for i in range(4)]

        for snapshot in snapshots:
            self.assertEqual(8, len(snapshot))
            self.assertTrue(
                all(
                    path.startswith(("tree/a0/wide/", "tree/a1/wide/"))
                    for path in snapshot
                )
            )
        self.assertNotEqual(snapshots[0], snapshots[-1])

    def test_wide_dir_churns_every_entry(self) -> None:
        """Every wide entry keeps its long name and changes content per revision."""
        wide = WideDir(name="tree/wide", entries=2000, churn_all=True)
        layout = GeneratedRepo(seed=1, file_count=8, commit_count=3, wide_dir=wide)
        snapshots = [
            [f for f in layout.files(r) if f.path.startswith("tree/wide/")]
            for r in range(3)
        ]
        for entries in snapshots:
            self.assertEqual(2000, len(entries))
            self.assertEqual(2000, len({f.path for f in entries}))
            self.assertTrue(all(len(Path(f.path).name) >= 64 for f in entries))
        self.assertEqual(snapshots[0], snapshots[1])
        for entry in snapshots[0]:
            contents = {layout.content(entry, r) for r in range(3)}
            self.assertEqual(3, len(contents), entry.path)
        repo = mock.Mock(spec=hgrepo.HgRepository)
        repo.hg.side_effect = _sl_tracks_every_path
        repo.commit.side_effect = ("node0", "node1", "node2")
        repo.temp_mgr = TempFileManager()
        try:
            RepoAuthor(layout).author(cast(hgrepo.HgRepository, repo))
            written = [call.args[0] for call in repo.write_file.call_args_list]
            wide_paths = [f.path for f in snapshots[0]]
            self.assertEqual(3 * 2000, sum(path in wide_paths for path in written))
        finally:
            repo.temp_mgr.cleanup()

    def test_blob_size_pads_content(self) -> None:
        """`blob_size` pads every generated file to that many hash-derived bytes, and the layout agrees."""
        layout = GeneratedRepo(seed=7, file_count=4, blob_size=4096)
        entry, other = layout.file_at(2, 0), layout.file_at(3, 0)
        content = layout.content(entry, 0)
        self.assertEqual(4096, len(content))
        self.assertEqual(content, generated_content(7, entry.path, 4096))
        self.assertTrue(content.startswith(generated_content(7, entry.path)))
        self.assertNotEqual(content[65:], generated_content(7, other.path, 4096)[65:])
        with self.assertRaises(ValueError):
            GeneratedRepo(seed=7, file_count=4, blob_size=64)

    def test_type_change_commit(self) -> None:
        """A type change replaces file p with directory p/ holding one generated file, and back."""
        path = "tree/registers/r0.reg"
        changes = (TypeChange(path, at_revision=1), TypeChange(path, at_revision=2))
        layout = GeneratedRepo(
            seed=1, file_count=4, commit_count=3, type_changes=changes
        )
        self.assertEqual(
            (False, True, False), tuple(layout.is_directory(path, r) for r in range(3))
        )
        for revision, under in ((0, 0), (1, 1), (2, 0)):
            paths = [f.path for f in layout.files(revision)]
            self.assertEqual(4 + under, len(paths), revision)
            self.assertEqual(
                under, sum(p.startswith(path + "/") for p in paths), revision
            )
            self.assertNotIn(path, paths)
        for bad in (0, 3):
            with self.assertRaises(ValueError):
                GeneratedRepo(
                    seed=1,
                    file_count=4,
                    commit_count=3,
                    type_changes=(TypeChange(path, bad),),
                )
        with self.assertRaisesRegex(ValueError, "repeat a .path, at_revision. pair"):
            GeneratedRepo(
                seed=1,
                file_count=4,
                commit_count=3,
                type_changes=(TypeChange(path, 1), TypeChange(path, 1)),
            )
        with self.assertRaisesRegex(ValueError, f"not committed images: {path}"):
            RepoAuthor(layout)
        RepoAuthor(layout, committed=_images((path,)))

    def test_rejects_reserved_paths_that_collide(self) -> None:
        """A committed image, type-change child, or wide directory may not land on another path."""
        retyped = (TypeChange("tree/registers/r0.reg", 1),)
        cases = {
            "generated path": ((), None, ("tree/a0/000/00000.dat",)),
            "generated root": ((), None, ("tree/a1",)),
            "generated ancestor": ((), None, ("tree",)),
            "wide under generated": ((), WideDir("tree/a0/wide", 2, False), ()),
            "inside wide": ((), WideDir("tree/wide", 2, False), ("tree/wide/x",)),
            "file above file": ((), None, ("reg/a", "reg/a/b")),
            "child is committed": (
                retyped,
                None,
                ("tree/registers/r0.reg", "tree/registers/r0.reg/entry.dat"),
            ),
        }
        for name, (changes, wide, paths) in cases.items():
            with self.subTest(name):
                layout = GeneratedRepo(
                    seed=1,
                    file_count=4,
                    commit_count=3,
                    wide_dir=wide,
                    type_changes=changes,
                )
                with self.assertRaises(ValueError):
                    RepoAuthor(layout, committed=_images(paths))
        layout = GeneratedRepo(
            seed=1,
            file_count=4,
            commit_count=3,
            wide_dir=WideDir("tree/wide", 2, False),
            type_changes=retyped,
        )
        RepoAuthor(
            layout,
            committed=_images(("tree/registers/r0.reg", "tree/registers/r1.reg")),
        )

    def test_author_uses_explicit_sl_path_listfiles(self) -> None:
        layout = GeneratedRepo(seed=1, file_count=5, commit_count=2)
        repo = mock.Mock(spec=hgrepo.HgRepository)
        repo.hg.side_effect = _sl_tracks_every_path
        repo.commit.side_effect = ("node0", "node1")
        repo.temp_mgr = TempFileManager()
        try:
            RepoAuthor(layout).author(cast(hgrepo.HgRepository, repo))

            sl_calls = [call.args for call in repo.hg.call_args_list]
            self.assertEqual(
                ["add", "files", "remove", "add", "files"],
                [args[0] for args in sl_calls],
            )
            self.assertNotIn(("addremove",), sl_calls)
            for args in sl_calls:
                self.assertEqual(2, len(args))
                self.assertTrue(args[1].startswith("listfile:"))
                listfile = args[1].removeprefix("listfile:")
                self.assertTrue(
                    all(
                        line.startswith("path:")
                        for line in Path(listfile).read_text().splitlines()
                    )
                )
        finally:
            repo.temp_mgr.cleanup()

    def test_author_forwards_command_timeout_to_sl_commands(self) -> None:
        layout = GeneratedRepo(seed=1, file_count=5, commit_count=2)
        repo = mock.Mock(spec=hgrepo.HgRepository)
        repo.hg.side_effect = _sl_tracks_every_path
        repo.commit.side_effect = ("node0", "node1")
        repo.temp_mgr = TempFileManager()
        try:
            RepoAuthor(layout, command_timeout_seconds=12.5).author(
                cast(hgrepo.HgRepository, repo)
            )

            self.assertTrue(
                all(call.kwargs == {"timeout": 12.5} for call in repo.hg.mock_calls)
            )
            self.assertTrue(
                all(call.kwargs == {"timeout": 12.5} for call in repo.commit.mock_calls)
            )
        finally:
            repo.temp_mgr.cleanup()

    def test_author_reports_sl_command_timeout(self) -> None:
        layout = GeneratedRepo(seed=1, file_count=2, commit_count=2)
        repo = mock.Mock(spec=hgrepo.HgRepository)
        repo.hg.side_effect = subprocess.TimeoutExpired(["sl", "add"], 0.25)
        repo.temp_mgr = TempFileManager()
        try:
            with self.assertRaisesRegex(RepoAuthorTimeout, "sl add exceeded 0.25s"):
                RepoAuthor(layout, command_timeout_seconds=0.25).author(
                    cast(hgrepo.HgRepository, repo)
                )
        finally:
            repo.temp_mgr.cleanup()

    def test_committed_images_change_every_revision_and_after_a_type_change(
        self,
    ) -> None:
        """Committed paths hold their per-revision image whenever they are files.

        r1 is a directory at revision 1 and returns to a file at revision 2,
        holding that revision's image; generated files keep their stable bytes.
        """
        r0, r1 = "tree/registers/r0.reg", "tree/registers/r1.reg"
        changes = (TypeChange(r1, at_revision=1), TypeChange(r1, at_revision=2))
        layout = GeneratedRepo(
            seed=1, file_count=4, commit_count=4, type_changes=changes
        )
        images = _images((r0, r1))
        repo = mock.Mock(spec=hgrepo.HgRepository)
        repo.path = "/nonexistent-committed-image-repo"
        repo.hg.side_effect = _sl_tracks_every_path
        writes: list[list[tuple[str, bytes]]] = [[]]

        def write_file(path: str, data: bytes, **_kwargs: object) -> None:
            writes[-1].append((path, data))

        def commit(*_args: object, **_kwargs: object) -> str:
            writes.append([])
            return f"node{len(writes) - 2}"

        repo.write_file.side_effect = write_file
        repo.commit.side_effect = commit
        repo.temp_mgr = TempFileManager()
        try:
            RepoAuthor(layout, committed=images).author(cast(hgrepo.HgRepository, repo))
            adds = [
                Path(args[1].removeprefix("listfile:")).read_text().split()
                for args in (call.args for call in repo.hg.call_args_list)
                if args[0] == "add"
            ]
        finally:
            repo.temp_mgr.cleanup()
        by_revision = [dict(revision) for revision in writes[:4]]
        expected = {
            0: {r0: images.content(r0, 0), r1: images.content(r1, 0)},
            1: {r0: images.content(r0, 1)},
            2: {r0: images.content(r0, 2), r1: images.content(r1, 2)},
            3: {r0: images.content(r0, 3), r1: images.content(r1, 3)},
        }
        for revision, committed in expected.items():
            with self.subTest(revision=revision):
                written = {
                    path: data
                    for path, data in by_revision[revision].items()
                    if path in (r0, r1)
                }
                self.assertEqual(committed, written)
        self.assertEqual(4, len({images.content(r0, r) for r in range(4)}))
        self.assertIn(f"path:{r0}", adds[0])
        self.assertIn(f"path:{r1}", adds[2])
        self.assertIn(f"{r1}/entry.dat", by_revision[1])
        generated = layout.file_at(0, 0)
        self.assertEqual(layout.content(generated, 0), by_revision[0][generated.path])

    def test_rejects_invalid_sizes_and_revisions(self) -> None:
        with self.assertRaises(ValueError):
            GeneratedRepo(seed=1, file_count=1, commit_count=4)
        with self.assertRaises(ValueError):
            GeneratedRepo(seed=1, repo_shape=cast(RepoShape, "w200k"))
        self.assertEqual(
            400_002, GeneratedRepo(seed=1, repo_shape=RepoShape.W200K).file_count
        )
        layout = GeneratedRepo(seed=1, file_count=4)
        with self.assertRaises(IndexError):
            layout.file_at(0, 4)
        with self.assertRaises(ValueError):
            RepoAuthor(layout, command_timeout_seconds=-1.0)


class RepoAuthorSlTest(unittest.TestCase):
    def test_committed_images_are_what_each_commit_holds(self) -> None:
        """A real backing repo holds each revision's image, including after a type change."""
        r0, r1 = "tree/registers/r0.reg", "tree/registers/r1.reg"
        changes = (TypeChange(r1, at_revision=1), TypeChange(r1, at_revision=2))
        layout = GeneratedRepo(
            seed=3, file_count=8, commit_count=3, type_changes=changes
        )
        images = _images((r0, r1))
        temp_mgr = TempFileManager()
        try:
            repos_dir = temp_mgr.make_temp_dir(prefix="repos.")
            repo = create_backing_repo(repos_dir, temp_mgr)
            commits = RepoAuthor(layout, committed=images).author(repo)
            for commit in commits:
                with self.subTest(revision=commit.revision):
                    files = set(repo.hg("files", "-r", commit.node).split())
                    self.assertEqual(
                        images.content(r0, commit.revision).decode(),
                        repo.hg("cat", "-r", commit.node, r0),
                    )
                    if commit.revision == 1:
                        self.assertNotIn(r1, files)
                        self.assertIn(f"{r1}/entry.dat", files)
                    else:
                        self.assertEqual(
                            images.content(r1, commit.revision).decode(),
                            repo.hg("cat", "-r", commit.node, r1),
                        )
                    generated = layout.file_at(0, commit.revision)
                    self.assertEqual(
                        layout.content(generated, commit.revision).decode(),
                        repo.hg("cat", "-r", commit.node, generated.path),
                    )
        finally:
            temp_mgr.cleanup()

    def test_commit_bounds_its_node_lookup_by_the_timeout(self) -> None:
        temp_mgr = TempFileManager()
        try:
            repo = create_backing_repo(
                temp_mgr.make_temp_dir(prefix="repos."), temp_mgr
            )
            repo.write_file("a.dat", "a\n")
            with mock.patch.object(repo, "hg", wraps=repo.hg) as hg:
                node = repo.commit("a", timeout=30.0)
            hg.assert_called_with("log", "-T{node}", "-r.", timeout=30.0)
            self.assertEqual(40, len(node))
        finally:
            temp_mgr.cleanup()

    def test_add_that_skips_a_path_fails_at_the_add(self) -> None:
        """A path `sl add` skips raises at that add, not at a later `sl remove`."""
        layout = GeneratedRepo(seed=6201, file_count=8, commit_count=2)
        dropped = layout.file_at(3, 0).path
        add_path_list = RepoAuthor._sl_path_list
        adds: list[list[str]] = []

        def drop_from_first_add(
            repo: hgrepo.HgRepository,
            command: str,
            paths: list[str],
            timeout_seconds: float | None,
        ) -> str:
            if command == "add":
                adds.append(paths)
                if len(adds) == 1:
                    paths = [path for path in paths if path != dropped]
            return add_path_list(repo, command, paths, timeout_seconds)

        temp_mgr = TempFileManager()
        try:
            repos_dir = temp_mgr.make_temp_dir(prefix="repos.")
            repo = create_backing_repo(repos_dir, temp_mgr)
            self.assertEqual("off", repo.hg("config", "fsmonitor.mode").strip())
            with mock.patch.object(
                RepoAuthor, "_sl_path_list", staticmethod(drop_from_first_add)
            ):
                with self.assertRaisesRegex(
                    RepoAuthorUntracked,
                    f"^sl add left 1 of 8 paths untracked: {dropped}$",
                ):
                    RepoAuthor(layout).author(repo)
            self.assertEqual(1, len(adds))
            self.assertIn(dropped, adds[0])
        finally:
            temp_mgr.cleanup()
