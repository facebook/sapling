# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import annotations

import configparser
import hashlib
import logging
import subprocess
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from eden.integration.lib import hgrepo
    from eden.test_support.temporary_directory import TempFileManager


class RepoShape(StrEnum):
    """Generated repository scale and directory layout."""

    S10K = "s10k"
    M100K = "m100k"
    L1M = "l1m"
    W200K = "w200k"


DEFAULT_FILE_COUNT = 10_000
DEFAULT_COMMIT_COUNT = 4
DEFAULT_REPO_SHAPE = RepoShape.S10K
_SHAPE_FILE_COUNTS = {
    RepoShape.S10K: 10_000,
    RepoShape.M100K: 100_000,
    RepoShape.L1M: 1_000_000,
    RepoShape.W200K: 400_002,
}
_FILES_PER_DIRECTORY = 100
# Every generated path, in every shape, lives under one of these.
_GENERATED_ROOTS = ("tree/a0", "tree/a1")
_REPLACEMENT_STRIDE = 97
# Long names keep a wide directory from fitting one readdir response.
_WIDE_ENTRY_SUFFIX = (
    "-entry-whose-long-name-spreads-the-listing-over-several-responses.dat"
)

logger: logging.Logger = logging.getLogger(__name__)


class RepoAuthorTimeout(RuntimeError):
    """Raised when generated backing-repo authoring exceeds its command budget."""

    def __init__(self, phase: str, timeout_seconds: float | None) -> None:
        super().__init__(f"{phase} exceeded {timeout_seconds}s")
        self.phase = phase
        self.timeout_seconds = timeout_seconds


class RepoAuthorUntracked(RuntimeError):
    """Raised when ``sl add`` leaves generated paths untracked."""


@dataclass(frozen=True)
class GeneratedFile:
    """One deterministic regular file in an authored snapshot."""

    path: str
    executable: bool


@dataclass(frozen=True)
class WideDir:
    """One flat directory of long-named entries, rewritten every revision when `churn_all` is set."""

    name: str
    entries: int
    churn_all: bool


@dataclass(frozen=True)
class TypeChange:
    """Commit ``at_revision`` toggles ``path`` between a file and a directory."""

    path: str
    at_revision: int


def wide_entry_path(name: str, index: int) -> str:
    """Return the path of one wide-directory entry; names never change."""
    return f"{name}/{index:06d}{_WIDE_ENTRY_SUFFIX}"


# The single generated file inside a path that a type change turned into a directory.
_TYPE_CHANGE_CHILD = "entry.dat"


@dataclass(frozen=True)
class CommittedImages:
    """Revision-specific committed bytes for a small set of reserved file paths.

    `content(path, revision)` returns the exact bytes the commit of
    `revision` holds at `path`. A path that a type change turns into a
    directory holds no image at those revisions and gets one again when it
    returns to a file.
    """

    paths: tuple[str, ...]
    content: Callable[[str, int], bytes]


@dataclass(frozen=True)
class AuthoredCommit:
    """An authored commit and the generated revision it represents."""

    node: str
    revision: int


def generated_content(seed: int, path: str, blob_size: int | None = None) -> bytes:
    """Return stable backing-repo bytes for `(seed, path)`, padded to `blob_size`."""
    key = f"{seed}\0{path}".encode()
    line = f"{hashlib.sha256(key).hexdigest()}\n".encode()
    if blob_size is None:
        return line
    return line + hashlib.shake_256(key).digest(blob_size - len(line))


# The smallest `blob_size`: the digest line that every generated file starts with.
MIN_BLOB_SIZE = len(generated_content(0, ""))


def _hgrepo() -> ModuleType:
    """Return Sapling's test repository module, imported only to author a repo.

    It needs buck-generated configuration, so a module that reads only the
    layout (the checker's pinned facts) imports without buck.
    """
    from eden.integration.lib import hgrepo

    return hgrepo


# Sapling runs as `sl` (`SL_IDENTITY`); a repository it creates keeps the
# `.hg` dot directory that EdenFS and the harness read (`SL_REPO_IDENTITY`).
SL_ENVIRONMENT: dict[str, str] = {"SL_IDENTITY": "sl", "SL_REPO_IDENTITY": "hg"}


def system_config_path(repos_dir: Path) -> Path:
    """Return the config shared by generated backing repos and their clones."""
    return repos_dir / "sapling.conf"


def write_system_config(repos_dir: Path) -> Path:
    """Write and return the config shared by generated backing repos."""
    system_config = system_config_path(repos_dir)
    system_config.write_text(
        _hgrepo().HgRepository.get_system_hgrc_contents(), encoding="utf-8"
    )
    return system_config


def default_file_count(repo_shape: RepoShape) -> int:
    """Return the default file count for an opt-in generated repo shape."""
    return _SHAPE_FILE_COUNTS[repo_shape]


def create_backing_repo(
    repos_dir: Path, temp_mgr: TempFileManager, name: str = "backing_repo"
) -> hgrepo.HgRepository:
    """Create an empty Sapling repository ready for deterministic authoring."""
    system_config = write_system_config(repos_dir)
    repo_path = repos_dir / name
    repo_path.mkdir()
    repo = sapling_repository(repo_path, system_config, temp_mgr)
    # Under load Watchman can miss a file created in a new directory, and
    # `sl add` then skips it without an error; the disk walk does not miss it.
    config = configparser.ConfigParser()
    config["fsmonitor"] = {"mode": "off"}
    repo.init(hgrc=config)
    return repo


class GeneratedRepo:
    """Pure description of the repository snapshots authored for one seed."""

    def __init__(
        self,
        seed: int,
        file_count: int | None = None,
        commit_count: int = DEFAULT_COMMIT_COUNT,
        repo_shape: RepoShape = DEFAULT_REPO_SHAPE,
        wide_dir: WideDir | None = None,
        type_changes: tuple[TypeChange, ...] = (),
        blob_size: int | None = None,
    ) -> None:
        if not isinstance(repo_shape, RepoShape):
            raise ValueError("repo_shape must name a supported generated repo shape")
        if file_count is None:
            file_count = default_file_count(repo_shape)
        if blob_size is not None and blob_size < MIN_BLOB_SIZE:
            raise ValueError(f"blob_size must be at least {MIN_BLOB_SIZE}")
        if file_count < commit_count - 1 or commit_count < 2:
            raise ValueError("file_count must cover every revision after the first")
        if commit_count > _REPLACEMENT_STRIDE + 1:
            raise ValueError(f"commit_count must be at most {_REPLACEMENT_STRIDE + 1}")
        if any(not 1 <= change.at_revision < commit_count for change in type_changes):
            raise ValueError("type_changes must name revisions after the first")
        if len(set(type_changes)) != len(type_changes):
            raise ValueError("type_changes must not repeat a (path, at_revision) pair")
        self.seed = seed
        self.file_count = file_count
        self.commit_count = commit_count
        self.repo_shape = repo_shape
        self.wide_dir = wide_dir
        self.type_changes = type_changes
        self.blob_size = blob_size

    def files(self, revision: int) -> Iterator[GeneratedFile]:
        """Yield one snapshot in stable path-index order without storing content.

        A type-change path appears only as its directory child: its file form
        is a committed image, so `CommittedImages` supplies it.
        """
        self._check_revision(revision)
        for index in range(self.file_count):
            yield self.file_at(index, revision)
        yield from self.wide_files()
        for path in sorted({change.path for change in self.type_changes}):
            if self.is_directory(path, revision):
                yield self.type_change_child(path)

    def is_directory(self, path: str, revision: int) -> bool:
        """Return whether type changes up to ``revision`` leave ``path`` a directory."""
        self._check_revision(revision)
        toggles = sum(
            change.path == path and change.at_revision <= revision
            for change in self.type_changes
        )
        return toggles % 2 == 1

    @staticmethod
    def type_change_child(path: str) -> GeneratedFile:
        """Return the one generated file inside a type-changed directory."""
        return GeneratedFile(path=f"{path}/{_TYPE_CHANGE_CHILD}", executable=False)

    def wide_files(self) -> Iterator[GeneratedFile]:
        """Yield the wide directory's entries; their names never change."""
        if self.wide_dir is None:
            return
        for index in range(self.wide_dir.entries):
            yield GeneratedFile(
                path=wide_entry_path(self.wide_dir.name, index), executable=False
            )

    def content(self, generated: GeneratedFile, revision: int) -> bytes:
        """Return the committed bytes of one generated file at a revision."""
        self._check_revision(revision)
        wide = self.wide_dir
        if (
            wide is not None
            and wide.churn_all
            and generated.path.startswith(f"{wide.name}/")
        ):
            return generated_content(
                self.seed, f"{generated.path}@{revision}", self.blob_size
            )
        return generated_content(self.seed, generated.path, self.blob_size)

    def file_at(self, index: int, revision: int) -> GeneratedFile:
        """Return the generated file for one stable index at a revision."""
        self._check_revision(revision)
        if not 0 <= index < self.file_count:
            raise IndexError(index)
        replacement = index % _REPLACEMENT_STRIDE + 1
        actor = index % 2
        if self.repo_shape is RepoShape.W200K:
            stem = (
                f"r{replacement}-{index:06d}"
                if replacement <= revision
                else f"{index:06d}"
            )
            return GeneratedFile(
                path=f"tree/a{actor}/wide/{stem}.dat",
                executable=index % 251 == 0,
            )
        if replacement <= revision:
            path = f"tree/a{actor}/r{replacement}/{index:05d}.dat"
        else:
            path = f"tree/a{actor}/{index // _FILES_PER_DIRECTORY:03d}/{index:05d}.dat"
        return GeneratedFile(path=path, executable=index % 251 == 0)

    def changed_indices(self, revision: int) -> Iterator[int]:
        """Yield indexes whose path changes when entering `revision`."""
        self._check_revision(revision)
        if revision == 0:
            return
        yield from range(revision - 1, self.file_count, _REPLACEMENT_STRIDE)

    def _check_revision(self, revision: int) -> None:
        if not 0 <= revision < self.commit_count:
            raise IndexError(revision)


def sapling_repository(
    path: Path, system_config: Path, temp_mgr: TempFileManager
) -> hgrepo.HgRepository:
    """Open the repository at `path`; its Sapling processes run as `sl`."""
    repo = _hgrepo().HgRepository(
        str(path), system_hgrc=str(system_config), temp_mgr=temp_mgr
    )
    repo.hg_environment.update(SL_ENVIRONMENT)
    return repo


def _check_reserved_paths(layout: GeneratedRepo, committed: frozenset[str]) -> None:
    """Reject reserved paths that collide with generated paths or with each other.

    Every generated path lives under `_GENERATED_ROOTS`, so a reserved path
    (committed image, type-change child, wide directory) collides with one
    exactly when it is a root, an ancestor of one, or under one. Among
    reserved paths, only a type-change path may hold another: its child.

    Raises:
        ValueError: If a reserved path collides.
    """
    retyped = {change.path for change in layout.type_changes}
    children = {layout.type_change_child(path).path for path in retyped}
    wide = () if layout.wide_dir is None else (layout.wide_dir.name,)
    reserved = [*committed, *children, *wide]
    if len(set(reserved)) != len(reserved):
        raise ValueError("reserved paths must be distinct")
    for path in reserved:
        for root in _GENERATED_ROOTS:
            if (
                path == root
                or root.startswith(f"{path}/")
                or path.startswith(f"{root}/")
            ):
                raise ValueError(
                    f"reserved path {path!r} collides with generated {root!r}"
                )
        parts = path.split("/")
        for depth in range(1, len(parts)):
            parent = "/".join(parts[:depth])
            if parent in reserved and not (
                parent in retyped and path == layout.type_change_child(parent).path
            ):
                raise ValueError(f"reserved path {parent!r} is a directory of {path!r}")


class RepoAuthor:
    """Materialize a `GeneratedRepo` as a deterministic Sapling commit series.

    `committed` paths hold their revision-specific images in every revision
    where they are files; generated files keep their revision-stable content.
    Every type-change path must be a committed path.
    """

    def __init__(
        self,
        layout: GeneratedRepo,
        command_timeout_seconds: float | None = None,
        *,
        committed: CommittedImages | None = None,
    ) -> None:
        if command_timeout_seconds is not None and command_timeout_seconds <= 0:
            raise ValueError("command_timeout_seconds must be positive")
        if committed is not None and len(set(committed.paths)) != len(committed.paths):
            raise ValueError("committed image paths must be distinct")
        self._layout = layout
        self._command_timeout_seconds = command_timeout_seconds
        self._committed = committed
        self._committed_paths = (
            frozenset() if committed is None else frozenset(committed.paths)
        )
        uncommitted = sorted(
            {change.path for change in layout.type_changes} - self._committed_paths
        )
        if uncommitted:
            raise ValueError(
                f"type-change paths are not committed images: {', '.join(uncommitted)}"
            )
        _check_reserved_paths(layout, self._committed_paths)

    def author(self, repo: hgrepo.HgRepository) -> tuple[AuthoredCommit, ...]:
        """Write every generated revision and return its commit identity."""
        commits = [self._write_initial(repo)]
        for revision in range(1, self._layout.commit_count):
            commits.append(self._write_transition(repo, revision))
        return tuple(commits)

    def _committed_files(self, revision: int) -> list[GeneratedFile]:
        """Return the committed image paths that are files at `revision`."""
        if self._committed is None:
            return []
        return [
            GeneratedFile(path, executable=False)
            for path in self._committed.paths
            if not self._layout.is_directory(path, revision)
        ]

    def _write_initial(self, repo: hgrepo.HgRepository) -> AuthoredCommit:
        added_paths: list[str] = []
        for generated in (*self._layout.files(0), *self._committed_files(0)):
            self._write(repo, generated, 0)
            added_paths.append(generated.path)
        self._add(repo, added_paths)
        return self._commit(repo, 0)

    def _write_transition(
        self, repo: hgrepo.HgRepository, revision: int
    ) -> AuthoredCommit:
        indices = list(self._layout.changed_indices(revision))
        removed_paths = [
            self._layout.file_at(index, revision - 1).path for index in indices
        ]
        # A type change removes the file (or its child) before the new entry lands.
        retyped = [
            (change.path, self._layout.is_directory(change.path, revision))
            for change in self._layout.type_changes
            if change.at_revision == revision
        ]
        child = self._layout.type_change_child
        removed_paths.extend(
            path if to_directory else child(path).path for path, to_directory in retyped
        )
        added: list[GeneratedFile] = [
            self._layout.file_at(index, revision) for index in indices
        ]
        added.extend(
            child(path) if to_directory else GeneratedFile(path, executable=False)
            for path, to_directory in retyped
        )
        self._sl_path_list(repo, "remove", removed_paths, self._command_timeout_seconds)
        for path, to_directory in retyped:
            if not to_directory and Path(repo.path, path).is_dir():
                Path(repo.path, path).rmdir()
        for generated in added:
            self._write(repo, generated, revision)
        self._add(repo, [f.path for f in added])
        fresh = {generated.path for generated in added}
        for generated in self._committed_files(revision):
            if generated.path not in fresh:
                self._write(repo, generated, revision)
        wide = self._layout.wide_dir
        if wide is not None and wide.churn_all:
            for generated in self._layout.wide_files():
                self._write(repo, generated, revision)
        return self._commit(repo, revision)

    def _write(
        self, repo: hgrepo.HgRepository, generated: GeneratedFile, revision: int
    ) -> None:
        repo.write_file(
            generated.path,
            self._content(generated, revision),
            mode=0o755 if generated.executable else 0o644,
            add=False,
        )

    def _content(self, generated: GeneratedFile, revision: int) -> bytes:
        """Return a file's bytes at `revision`: its committed image, else generated."""
        if self._committed is not None and generated.path in self._committed_paths:
            return self._committed.content(generated.path, revision)
        return self._layout.content(generated, revision)

    def _add(self, repo: hgrepo.HgRepository, paths: list[str]) -> None:
        self._sl_path_list(repo, "add", paths, self._command_timeout_seconds)
        self._check_tracked(repo, paths)

    def _check_tracked(self, repo: hgrepo.HgRepository, paths: list[str]) -> None:
        """Raise unless every path in ``paths`` is tracked in the dirstate.

        ``sl add`` skips, without an error, any file that status did not
        report. ``sl files`` reads the dirstate rather than walking the disk.
        """
        try:
            listed = self._sl_path_list(
                repo, "files", paths, self._command_timeout_seconds
            )
        except _hgrepo().HgError as error:
            # `sl files` exits 1 when no tracked file matches.
            if error.returncode != 1:
                raise
            listed = ""
        missing = sorted(set(paths).difference(listed.splitlines()))
        if missing:
            raise RepoAuthorUntracked(
                f"sl add left {len(missing)} of {len(paths)} paths untracked: "
                f"{', '.join(missing[:5])}"
            )

    @staticmethod
    def _sl_path_list(
        repo: hgrepo.HgRepository,
        command: str,
        paths: list[str],
        timeout_seconds: float | None,
    ) -> str:
        """Run a path-based Sapling command with a compact listfile pattern."""
        if not paths:
            return ""
        with repo.temp_mgr.make_temp_file(prefix="sl_paths.") as path_file:
            path_file.write("\n".join(f"path:{path}" for path in paths))
            path_file.write("\n")
            path_file.flush()
            try:
                return repo.hg(
                    command,
                    f"listfile:{path_file.name}",
                    timeout=timeout_seconds,
                )
            except subprocess.TimeoutExpired as error:
                raise RepoAuthorTimeout(f"sl {command}", timeout_seconds) from error

    def _commit(self, repo: hgrepo.HgRepository, revision: int) -> AuthoredCommit:
        try:
            node = repo.commit(
                f"Correctness fixture revision {revision}.",
                timeout=self._command_timeout_seconds,
            )
        except subprocess.TimeoutExpired as error:
            raise RepoAuthorTimeout(
                "sl commit", self._command_timeout_seconds
            ) from error
        return AuthoredCommit(node=node, revision=revision)
