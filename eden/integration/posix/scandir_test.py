#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Directory iteration on EdenFS: listdir, scandir, walk, fwalk, makedirs
and removedirs.

Ported from CPython's ``Lib/test/test_posix.py`` (``PosixTester``) and
``Lib/test/test_os.py`` (``TestScandir``, ``WalkTests``, ``FwalkTests``,
``MakedirTests``, ``RemoveDirsTests``), PSF license, Python 3.12: one test
method per CPython test, named after it, with the CPython class and test in
the docstring. ``listdir`` and ``scandir`` are ``posix`` functions; ``walk``,
``fwalk``, ``makedirs`` and ``removedirs`` exist only in ``os`` (built on
``posix.scandir``, ``mkdir`` and ``rmdir``) and are the only ``os`` functions
the tests call. ``WalkTests`` and ``FwalkTests`` share their methods through
``_WalkTestsMixin``, the way ``FwalkTests`` subclasses ``WalkTests`` in
CPython. ``README.md`` in this directory lists every CPython test with its
method and the checks that were left out.
"""

from __future__ import annotations

import errno
import os
import posix
import stat
from typing import Any, Iterator

from eden.integration.lib import testcase

from .base import PosixTestBase


@testcase.eden_nfs_repo_test
class ListdirTest(PosixTestBase):
    def test_listdir(self) -> None:
        """test_posix.PosixTester.test_listdir: listdir of '.'"""
        self.create_file("n0")
        self.assertIn("n0", posix.listdir("."))

    def test_listdir_default(self) -> None:
        """test_posix.PosixTester.test_listdir_default: listdir without an argument"""
        self.create_file("n0")
        self.assertIn("n0", posix.listdir())

    def test_listdir_bytes(self) -> None:
        """test_posix.PosixTester.test_listdir_bytes: a bytes path gives bytes names"""
        self.create_file("n0")
        self.assertIn(b"n0", posix.listdir(b"."))

    def test_listdir_fd(self) -> None:
        """test_posix.PosixTester.test_listdir_fd: listdir of a directory fd, twice"""
        self.create_file("n0")
        posix.mkdir("n1", 0o755)
        with self.open_fd(posix.getcwd(), posix.O_RDONLY) as fd:
            self.assertEqual(sorted(posix.listdir(".")), sorted(posix.listdir(fd)))
            # the fd offset is reset (issue #13739)
            self.assertEqual(sorted(posix.listdir(".")), sorted(posix.listdir(fd)))


@testcase.eden_nfs_repo_test
class ScandirTest(PosixTestBase):
    """``test_os.TestScandir``: ``n0`` is the directory."""

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        self.path = f"{posix.getcwd()}/n0"
        posix.mkdir(self.path, 0o755)

    def create_entry(self, name: str | bytes = "file.txt") -> str | bytes:
        """``TestScandir.create_file``: a 6-byte file in the directory."""
        if isinstance(name, bytes):
            filename: str | bytes = self.path.encode() + b"/" + name
        else:
            filename = f"{self.path}/{name}"
        self.create_file(filename, b"python")
        return filename

    def get_entries(self, names: list[str]) -> dict[str, posix.DirEntry]:
        entries = {entry.name: entry for entry in posix.scandir(self.path)}
        self.assertEqual(names, sorted(entries.keys()))
        return entries

    def get_entry(self, name: str | bytes) -> posix.DirEntry:
        path = self.path.encode() if isinstance(name, bytes) else self.path
        entries = list(posix.scandir(path))
        self.assertEqual(1, len(entries))
        self.assertEqual(name, entries[0].name)
        return entries[0]

    def check_entry(
        self,
        entry: posix.DirEntry,
        name: str,
        is_dir: bool,
        is_file: bool,
        is_symlink: bool,
    ) -> None:
        self.assertIsInstance(entry, posix.DirEntry)
        self.assertEqual(name, entry.name)
        self.assertEqual(f"{self.path}/{name}", entry.path)
        self.assertEqual(posix.lstat(entry.path).st_ino, entry.inode())
        entry_stat = posix.stat(entry.path)
        self.assertEqual(is_dir, entry.is_dir())
        self.assertEqual(stat.S_ISDIR(entry_stat.st_mode), entry.is_dir())
        self.assertEqual(is_file, entry.is_file())
        self.assertEqual(stat.S_ISREG(entry_stat.st_mode), entry.is_file())
        self.assertEqual(is_symlink, entry.is_symlink())
        entry_lstat = posix.lstat(entry.path)
        self.assertEqual(stat.S_ISLNK(entry_lstat.st_mode), entry.is_symlink())
        self.assertEqual(
            stat.S_ISDIR(entry_lstat.st_mode), entry.is_dir(follow_symlinks=False)
        )
        self.assertEqual(
            stat.S_ISREG(entry_lstat.st_mode), entry.is_file(follow_symlinks=False)
        )
        self.assertFalse(entry.is_junction())
        self.assertEqual(entry_stat, entry.stat())
        self.assertEqual(entry_lstat, entry.stat(follow_symlinks=False))

    def test_attributes(self) -> None:
        """test_os.TestScandir.test_attributes: d_type, inode and stat of every entry

        The hard link ``link_file.txt`` is left out (EdenFS by design: no
        hard links; CPython skips the whole test on PermissionError).
        """
        dirname = f"{self.path}/dir"
        posix.mkdir(dirname, 0o755)
        filename = self.create_entry("file.txt")
        posix.symlink(dirname, f"{self.path}/symlink_dir", target_is_directory=True)
        posix.symlink(filename, f"{self.path}/symlink_file.txt")
        entries = self.get_entries(
            ["dir", "file.txt", "symlink_dir", "symlink_file.txt"]
        )
        self.check_entry(entries["dir"], "dir", True, False, False)
        self.check_entry(entries["file.txt"], "file.txt", False, True, False)
        self.check_entry(entries["symlink_dir"], "symlink_dir", True, False, True)
        self.check_entry(
            entries["symlink_file.txt"], "symlink_file.txt", False, True, True
        )

    def test_current_directory(self) -> None:
        """test_os.TestScandir.test_current_directory: scandir() lists the cwd"""
        self.create_entry()
        old_dir = posix.getcwd()
        try:
            posix.chdir(self.path)
            entries = {entry.name: entry for entry in posix.scandir()}
            self.assertEqual(["file.txt"], sorted(entries.keys()))
        finally:
            posix.chdir(old_dir)

    def test_removed_dir(self) -> None:
        """test_os.TestScandir.test_removed_dir: an entry whose directory was removed"""
        path = f"{self.path}/dir"
        posix.mkdir(path, 0o755)
        entry = self.get_entry("dir")
        posix.rmdir(path)
        self.assertFalse(entry.is_file())
        self.assertFalse(entry.is_symlink())
        self.assertGreater(entry.inode(), 0)
        with self.assert_errno(errno.ENOENT):
            entry.stat()
        with self.assert_errno(errno.ENOENT):
            entry.stat(follow_symlinks=False)

    def test_removed_file(self) -> None:
        """test_os.TestScandir.test_removed_file: an entry whose file was unlinked"""
        self.create_entry()
        entry = self.get_entry("file.txt")
        posix.unlink(entry.path)
        self.assertFalse(entry.is_dir())
        self.assertFalse(entry.is_symlink())
        self.assertGreater(entry.inode(), 0)
        with self.assert_errno(errno.ENOENT):
            entry.stat()
        with self.assert_errno(errno.ENOENT):
            entry.stat(follow_symlinks=False)

    def test_broken_symlink(self) -> None:
        """test_os.TestScandir.test_broken_symlink: a symlink whose target was unlinked"""
        filename = self.create_entry("file.txt")
        posix.symlink(filename, f"{self.path}/symlink.txt")
        entries = self.get_entries(["file.txt", "symlink.txt"])
        entry = entries["symlink.txt"]
        posix.unlink(filename)
        self.assertGreater(entry.inode(), 0)
        self.assertFalse(entry.is_dir())
        self.assertFalse(entry.is_file())
        self.assertFalse(entry.is_dir(follow_symlinks=False))
        self.assertFalse(entry.is_file(follow_symlinks=False))
        self.assertTrue(entry.is_symlink())
        with self.assert_errno(errno.ENOENT):
            entry.stat()
        entry.stat(follow_symlinks=False)

    def test_bytes(self) -> None:
        """test_os.TestScandir.test_bytes: scandir of a bytes path gives bytes"""
        self.create_entry("file.txt")
        entries = list(posix.scandir(self.path.encode()))
        self.assertEqual(1, len(entries))
        self.assertEqual(b"file.txt", entries[0].name)
        self.assertEqual((f"{self.path}/file.txt").encode(), entries[0].path)

    def test_fd(self) -> None:
        """test_os.TestScandir.test_fd: scandir of a directory fd"""
        self.create_entry("file.txt")
        posix.symlink("file.txt", f"{self.path}/link")
        with self.open_fd(self.path, posix.O_RDONLY) as fd:
            with posix.scandir(fd) as it:
                entries = list(it)
            names = [entry.name for entry in entries]
            self.assertEqual(["file.txt", "link"], sorted(names))
            self.assertEqual(posix.listdir(fd), names)
            for entry in entries:
                self.assertEqual(entry.name, entry.path)
                self.assertEqual(entry.name, posix.fspath(entry))
                self.assertEqual(entry.name == "link", entry.is_symlink())
                st = posix.stat(entry.name, dir_fd=fd)
                self.assertEqual(st, entry.stat())
                st = posix.stat(entry.name, dir_fd=fd, follow_symlinks=False)
                self.assertEqual(st, entry.stat(follow_symlinks=False))


class _WalkTestsMixin:
    """``test_os.WalkTests``: the tree under ``n0``.

    ``n0/TEST1/`` has ``tmp1``, ``SUB1/`` (``tmp2``, ``SUB11/``) and
    ``SUB2/`` (``tmp3``, ``SUB21/tmp5`` made unreadable, ``link`` to
    ``n0/TEST2``, ``broken_link``, ``broken_link2``, ``broken_link3``);
    ``n0/TEST2/`` has ``tmp4``.
    """

    is_fwalk: bool = False
    walk_path: str
    sub1_path: str
    sub11_path: str
    tmp1_path: str
    link_path: str
    broken_link_path: str
    sub2_tree: tuple[str, list[str], list[str]]

    # The methods below come from PosixTestBase / unittest.TestCase at runtime.
    assertEqual: Any
    assertNotIn: Any
    assertIn: Any
    assertTrue: Any
    assertIsInstance: Any
    assertRaises: Any
    fail: Any
    addCleanup: Any
    create_file: Any
    use_nfs: Any

    def walk(
        self, top: str, **kwargs: Any
    ) -> Iterator[tuple[str, list[str], list[str]]]:
        if self.is_fwalk:
            for root, dirs, files, _root_fd in os.fwalk(top, **kwargs):
                yield (root, dirs, files)
        else:
            if "follow_symlinks" in kwargs:
                kwargs["followlinks"] = kwargs.pop("follow_symlinks")
            yield from os.walk(top, **kwargs)

    def setup_walk_tree(self) -> None:
        self.walk_path = "n0/TEST1"
        self.sub1_path = f"{self.walk_path}/SUB1"
        self.sub11_path = f"{self.sub1_path}/SUB11"
        sub2_path = f"{self.walk_path}/SUB2"
        sub21_path = f"{sub2_path}/SUB21"
        self.tmp1_path = f"{self.walk_path}/tmp1"
        tmp2_path = f"{self.sub1_path}/tmp2"
        tmp3_path = f"{sub2_path}/tmp3"
        tmp5_path = f"{sub21_path}/tmp3"
        self.link_path = f"{sub2_path}/link"
        t2_path = "n0/TEST2"
        tmp4_path = "n0/TEST2/tmp4"
        self.broken_link_path = f"{sub2_path}/broken_link"
        broken_link2_path = f"{sub2_path}/broken_link2"
        broken_link3_path = f"{sub2_path}/broken_link3"

        os.makedirs(self.sub11_path)
        os.makedirs(sub2_path)
        os.makedirs(sub21_path)
        os.makedirs(t2_path)
        for path in self.tmp1_path, tmp2_path, tmp3_path, tmp4_path, tmp5_path:
            self.create_file(
                path, f"I'm {path} and proud of it.  Blame test_os.\n".encode()
            )
        posix.symlink(f"{posix.getcwd()}/{t2_path}", self.link_path)
        posix.symlink("broken", self.broken_link_path, True)
        posix.symlink("tmp3/broken", broken_link2_path, True)
        posix.symlink("SUB21/tmp5", broken_link3_path, True)
        self.sub2_tree = (
            sub2_path,
            ["SUB21", "link"],
            ["broken_link", "broken_link2", "broken_link3", "tmp3"],
        )
        posix.chmod(sub21_path, 0)
        try:
            posix.listdir(sub21_path)
        except PermissionError:
            self.addCleanup(posix.chmod, sub21_path, stat.S_IRWXU)
        else:
            # EdenFS by design: the NFS server does not enforce permission
            # bits, so the unreadable directory is readable and, as in
            # CPython, drops out of the tree.
            self.assertTrue(self.use_nfs())
            posix.chmod(sub21_path, stat.S_IRWXU)
            posix.unlink(tmp5_path)
            posix.rmdir(sub21_path)
            del self.sub2_tree[1][:1]

    def test_walk_topdown(self) -> None:
        """test_os.WalkTests.test_walk_topdown, FwalkTests.test_walk_topdown: the tree, top-down"""
        self.setup_walk_tree()
        all_entries = list(self.walk(self.walk_path))
        self.assertEqual(4, len(all_entries))
        flipped = all_entries[0][1][0] != "SUB1"
        all_entries[0][1].sort()
        all_entries[3 - 2 * flipped][-1].sort()
        all_entries[3 - 2 * flipped][1].sort()
        self.assertEqual((self.walk_path, ["SUB1", "SUB2"], ["tmp1"]), all_entries[0])
        self.assertEqual(
            (self.sub1_path, ["SUB11"], ["tmp2"]), all_entries[1 + flipped]
        )
        self.assertEqual((self.sub11_path, [], []), all_entries[2 + flipped])
        self.assertEqual(self.sub2_tree, all_entries[3 - 2 * flipped])

    def test_walk_prune(self) -> None:
        """test_os.WalkTests.test_walk_prune, FwalkTests.test_walk_prune: removing a name from dirs prunes it"""
        self.setup_walk_tree()
        all_entries = []
        for root, dirs, files in self.walk(self.walk_path):
            all_entries.append((root, dirs, files))
            if "SUB1" in dirs:
                dirs.remove("SUB1")
        self.assertEqual(2, len(all_entries))
        self.assertEqual((self.walk_path, ["SUB2"], ["tmp1"]), all_entries[0])
        all_entries[1][-1].sort()
        all_entries[1][1].sort()
        self.assertEqual(self.sub2_tree, all_entries[1])

    def test_walk_bottom_up(self) -> None:
        """test_os.WalkTests.test_walk_bottom_up, FwalkTests.test_walk_bottom_up: the tree, bottom-up"""
        self.setup_walk_tree()
        all_entries = list(self.walk(self.walk_path, topdown=False))
        self.assertEqual(4, len(all_entries), all_entries)
        flipped = all_entries[3][1][0] != "SUB1"
        all_entries[3][1].sort()
        all_entries[2 - 2 * flipped][-1].sort()
        all_entries[2 - 2 * flipped][1].sort()
        self.assertEqual((self.walk_path, ["SUB1", "SUB2"], ["tmp1"]), all_entries[3])
        self.assertEqual((self.sub11_path, [], []), all_entries[flipped])
        self.assertEqual(
            (self.sub1_path, ["SUB11"], ["tmp2"]), all_entries[flipped + 1]
        )
        self.assertEqual(self.sub2_tree, all_entries[2 - 2 * flipped])

    def test_walk_symlink(self) -> None:
        """test_os.WalkTests.test_walk_symlink, FwalkTests.test_walk_symlink: follow_symlinks enters the link"""
        self.setup_walk_tree()
        walk_it = self.walk(self.walk_path, follow_symlinks=True)
        for root, dirs, files in walk_it:
            if root == self.link_path:
                self.assertEqual([], dirs)
                self.assertEqual(["tmp4"], files)
                break
        else:
            self.fail("Didn't follow symlink with followlinks=True")
        walk_it = self.walk(self.broken_link_path, follow_symlinks=True)
        if self.is_fwalk:
            self.assertRaises(FileNotFoundError, next, walk_it)
        self.assertRaises(StopIteration, next, walk_it)

    def test_walk_bad_dir(self) -> None:
        """test_os.WalkTests.test_walk_bad_dir, FwalkTests.test_walk_bad_dir: a directory renamed during the walk"""
        self.setup_walk_tree()
        errors = []
        walk_it = self.walk(self.walk_path, onerror=errors.append)
        root, dirs, files = next(walk_it)
        self.assertEqual([], errors)
        dir1 = "SUB1"
        path1 = f"{root}/{dir1}"
        path1new = f"{root}/{dir1}.new"
        posix.rename(path1, path1new)
        try:
            roots = [r for r, d, f in walk_it]
            self.assertTrue(errors)
            self.assertNotIn(path1, roots)
            self.assertNotIn(path1new, roots)
            for dir2 in dirs:
                if dir2 != dir1:
                    self.assertIn(f"{root}/{dir2}", roots)
        finally:
            posix.rename(path1new, path1)

    def test_walk_bad_dir2(self) -> None:
        """test_os.WalkTests.test_walk_bad_dir2, FwalkTests.test_walk_bad_dir2: walking a missing path and a file"""
        self.setup_walk_tree()
        walk_it = self.walk("nonexisting")
        if self.is_fwalk:
            self.assertRaises(FileNotFoundError, next, walk_it)
        self.assertRaises(StopIteration, next, walk_it)
        walk_it = self.walk("nonexisting", follow_symlinks=True)
        if self.is_fwalk:
            self.assertRaises(FileNotFoundError, next, walk_it)
        self.assertRaises(StopIteration, next, walk_it)
        walk_it = self.walk(self.tmp1_path)
        self.assertRaises(StopIteration, next, walk_it)
        walk_it = self.walk(self.tmp1_path, follow_symlinks=True)
        if self.is_fwalk:
            self.assertRaises(NotADirectoryError, next, walk_it)
        self.assertRaises(StopIteration, next, walk_it)

    def test_walk_named_pipe(self) -> None:
        """test_os.WalkTests.test_walk_named_pipe, FwalkTests.test_walk_named_pipe: walking a FIFO yields nothing"""
        posix.mkfifo("n0-pipe", 0o644)
        walk_it = self.walk("n0-pipe")
        self.assertRaises(StopIteration, next, walk_it)
        walk_it = self.walk("n0-pipe", follow_symlinks=True)
        if self.is_fwalk:
            self.assertRaises(NotADirectoryError, next, walk_it)
        self.assertRaises(StopIteration, next, walk_it)

    def test_walk_named_pipe2(self) -> None:
        """test_os.WalkTests.test_walk_named_pipe2, FwalkTests.test_walk_named_pipe2: a FIFO in a directory is a file"""
        posix.mkdir("n0-dir", 0o755)
        posix.mkfifo("n0-dir/mypipe", 0o644)
        errors: list[OSError] = []
        walk_it = self.walk("n0-dir", onerror=errors.append)
        next(walk_it)
        self.assertRaises(StopIteration, next, walk_it)
        self.assertEqual([], errors)
        errors = []
        walk_it = self.walk("n0-dir", onerror=errors.append)
        root, dirs, files = next(walk_it)
        self.assertEqual("n0-dir", root)
        self.assertEqual([], dirs)
        self.assertEqual(["mypipe"], files)
        dirs.extend(files)
        files.clear()
        if self.is_fwalk:
            self.assertRaises(NotADirectoryError, next, walk_it)
        self.assertRaises(StopIteration, next, walk_it)
        if self.is_fwalk:
            self.assertEqual([], errors)
        else:
            self.assertEqual(1, len(errors), errors)
            self.assertIsInstance(errors[0], NotADirectoryError)

    def test_walk_many_open_files(self) -> None:
        """test_os.WalkTests.test_walk_many_open_files, FwalkTests.test_walk_many_open_files: 100 walks of a deep tree at once"""
        depth = 30
        base = "n0/deep"
        p = "/".join([base] + ["d"] * depth)
        os.makedirs(p)
        iters = [self.walk(base, topdown=False) for _ in range(100)]
        for i in range(depth + 1):
            expected = (p, ["d"] if i else [], [])
            for it in iters:
                self.assertEqual(expected, next(it))
            p = p.rsplit("/", 1)[0]
        iters = [self.walk(base, topdown=True) for _ in range(100)]
        p = base
        for i in range(depth + 1):
            expected = (p, ["d"] if i < depth else [], [])
            for it in iters:
                self.assertEqual(expected, next(it))
            p = f"{p}/d"


@testcase.eden_nfs_repo_test
class WalkTest(_WalkTestsMixin, PosixTestBase):
    """``test_os.WalkTests`` with ``os.walk``."""

    is_fwalk = False


@testcase.eden_nfs_repo_test
class FwalkTest(_WalkTestsMixin, PosixTestBase):
    """``test_os.FwalkTests``: ``WalkTests`` with ``os.fwalk``, plus its own tests."""

    is_fwalk = True

    # fwalk() keeps file descriptors open (CPython sets the test to None too).
    # pyrefly: ignore[bad-override]
    test_walk_many_open_files = None

    def _compare_to_walk(
        self, walk_kwargs: dict[str, Any], fwalk_kwargs: dict[str, Any]
    ) -> None:
        walk_kwargs = walk_kwargs.copy()
        fwalk_kwargs = fwalk_kwargs.copy()
        for topdown in (True, False):
            for follow_symlinks in (True, False):
                walk_kwargs.update(topdown=topdown, followlinks=follow_symlinks)
                fwalk_kwargs.update(topdown=topdown, follow_symlinks=follow_symlinks)
                expected = {}
                for root, dirs, files in os.walk(**walk_kwargs):
                    expected[root] = (set(dirs), set(files))
                for root, dirs, files, _rootfd in os.fwalk(**fwalk_kwargs):
                    self.assertIn(root, expected)
                    self.assertEqual(expected[root], (set(dirs), set(files)))

    def test_compare_to_walk(self) -> None:
        """test_os.FwalkTests.test_compare_to_walk: fwalk and walk agree"""
        self.setup_walk_tree()
        kwargs: dict[str, Any] = {"top": "n0"}
        self._compare_to_walk(kwargs, kwargs)

    def test_dir_fd(self) -> None:
        """test_os.FwalkTests.test_dir_fd: fwalk relative to a directory fd"""
        self.setup_walk_tree()
        with self.open_fd(".", posix.O_RDONLY) as fd:
            walk_kwargs: dict[str, Any] = {"top": "n0"}
            fwalk_kwargs = walk_kwargs.copy()
            fwalk_kwargs["dir_fd"] = fd
            self._compare_to_walk(walk_kwargs, fwalk_kwargs)

    def test_yields_correct_dir_fd(self) -> None:
        """test_os.FwalkTests.test_yields_correct_dir_fd: the yielded fd lists the entries"""
        self.setup_walk_tree()
        for topdown in (True, False):
            for follow_symlinks in (True, False):
                for _root, dirs, files, rootfd in os.fwalk(
                    "n0", topdown, None, follow_symlinks=follow_symlinks
                ):
                    posix.fstat(rootfd)
                    posix.stat(rootfd)
                    self.assertEqual(set(dirs) | set(files), set(posix.listdir(rootfd)))

    def test_fd_leak(self) -> None:
        """test_os.FwalkTests.test_fd_leak: 256 fwalks leak no descriptor"""
        self.setup_walk_tree()
        minfd = posix.dup(1)
        posix.close(minfd)
        for _ in range(256):
            for _entry in os.fwalk("n0"):
                pass
        newfd = posix.dup(1)
        self.addCleanup(posix.close, newfd)
        self.assertEqual(minfd, newfd)


@testcase.eden_nfs_repo_test
class MakedirTest(PosixTestBase):
    """``test_os.MakedirTests``: ``n0`` is the base directory."""

    def setup_eden_test(self) -> None:
        super().setup_eden_test()
        posix.mkdir("n0", 0o777)

    def test_makedir(self) -> None:
        """test_os.MakedirTests.test_makedir: nested makedirs, with '.' components"""
        os.makedirs("n0/dir1/dir2/dir3")
        os.makedirs("n0/dir1/dir2/dir3/dir4")
        with self.assert_errno(errno.EEXIST):
            os.makedirs(".")
        os.makedirs("n0/dir1/dir2/dir3/dir4/dir5/.")
        os.makedirs("n0/dir1/./dir2/dir3/dir4/dir5/dir6")
        self.assertTrue(
            stat.S_ISDIR(posix.stat("n0/dir1/dir2/dir3/dir4/dir5/dir6").st_mode)
        )

    def test_mode(self) -> None:
        """test_os.MakedirTests.test_mode: the leaf gets the mode, the parents 0o777 & ~umask"""
        posix.umask(0o002)
        try:
            os.makedirs("n0/dir1/dir2", 0o555)
            self.assertTrue(stat.S_ISDIR(posix.stat("n0/dir1/dir2").st_mode))
            self.assertEqual(0o555, posix.stat("n0/dir1/dir2").st_mode & 0o777)
            self.assertEqual(0o775, posix.stat("n0/dir1").st_mode & 0o777)
        finally:
            posix.umask(0)

    def test_exist_ok_existing_directory(self) -> None:
        """test_os.MakedirTests.test_exist_ok_existing_directory: exist_ok"""
        mode = 0o777
        posix.umask(0o022)
        try:
            os.makedirs("n0/dir1", mode)
            with self.assert_errno(errno.EEXIST):
                os.makedirs("n0/dir1", mode)
            with self.assert_errno(errno.EEXIST):
                os.makedirs("n0/dir1", mode, exist_ok=False)
            os.makedirs("n0/dir1", 0o776, exist_ok=True)
            os.makedirs("n0/dir1", mode=mode, exist_ok=True)
        finally:
            posix.umask(0)
        os.makedirs("/", exist_ok=True)

    def test_exist_ok_s_isgid_directory(self) -> None:
        """test_os.MakedirTests.test_exist_ok_s_isgid_directory: makedirs on a setgid dir

        EdenFS by design: no setgid bit over FUSE. ``chmod`` with S_ISGID
        fails with EPERM, so CPython skips the test ('Cannot set S_ISGID for
        dir.'); the port pins the EPERM and runs the rest with the bit left
        out. Over NFS the server accepts the bit and the CPython flow runs.
        (``mkdir(0o2777)`` dropping S_ISGID is Linux: the VFS masks the mode
        of a new directory to 0o1777.)
        """
        posix.umask(0o022)
        try:
            existing_mode = stat.S_IMODE(posix.lstat("n0").st_mode)
            if self.use_nfs():
                # EdenFS by design (NFS): the NFS server accepts the bit.
                posix.chmod("n0", existing_mode | stat.S_ISGID)
                self.assertEqual(
                    existing_mode | stat.S_ISGID,
                    stat.S_IMODE(posix.lstat("n0").st_mode),
                )
            else:
                with self.assert_errno(errno.EPERM):
                    posix.chmod("n0", existing_mode | stat.S_ISGID)
                self.assertEqual(existing_mode, stat.S_IMODE(posix.lstat("n0").st_mode))
            os.makedirs("n0/dir1", 0o777 | stat.S_ISGID)
            self.assertEqual(0o755, stat.S_IMODE(posix.lstat("n0/dir1").st_mode))
            os.makedirs("n0/dir1", 0o777, exist_ok=True)
            if self.use_nfs():
                posix.chmod("n0/dir1", 0o755 & ~stat.S_ISGID)
            os.makedirs("n0/dir1", 0o777 | stat.S_ISGID, exist_ok=True)
        finally:
            posix.umask(0)

    def test_exist_ok_existing_regular_file(self) -> None:
        """test_os.MakedirTests.test_exist_ok_existing_regular_file: a file in the way"""
        self.create_file("n0/dir1", b"abc")
        with self.assert_errno(errno.EEXIST):
            os.makedirs("n0/dir1")
        with self.assert_errno(errno.EEXIST):
            os.makedirs("n0/dir1", exist_ok=False)
        with self.assert_errno(errno.EEXIST):
            os.makedirs("n0/dir1", exist_ok=True)
        posix.unlink("n0/dir1")


@testcase.eden_nfs_repo_test
class RemoveDirsTest(PosixTestBase):
    """``test_os.RemoveDirsTests``: ``n0`` is the base directory."""

    def exists(self, path: str) -> bool:
        try:
            posix.lstat(path)
        except FileNotFoundError:
            return False
        return True

    def test_remove_all(self) -> None:
        """test_os.RemoveDirsTests.test_remove_all: removedirs removes empty parents"""
        os.makedirs("n0/dira/dirb")
        os.removedirs("n0/dira/dirb")
        self.assertFalse(self.exists("n0/dira/dirb"))
        self.assertFalse(self.exists("n0/dira"))
        self.assertFalse(self.exists("n0"))

    def test_remove_partial(self) -> None:
        """test_os.RemoveDirsTests.test_remove_partial: removedirs stops at a non-empty parent"""
        os.makedirs("n0/dira/dirb")
        self.create_file("n0/dira/file.txt")
        os.removedirs("n0/dira/dirb")
        self.assertFalse(self.exists("n0/dira/dirb"))
        self.assertTrue(self.exists("n0/dira"))
        self.assertTrue(self.exists("n0"))

    def test_remove_nothing(self) -> None:
        """test_os.RemoveDirsTests.test_remove_nothing: a non-empty leaf is ENOTEMPTY"""
        os.makedirs("n0/dira/dirb")
        self.create_file("n0/dira/dirb/file.txt")
        with self.assert_errno(errno.ENOTEMPTY):
            os.removedirs("n0/dira/dirb")
        self.assertTrue(self.exists("n0/dira/dirb"))
        self.assertTrue(self.exists("n0/dira"))
        self.assertTrue(self.exists("n0"))
