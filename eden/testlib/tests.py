# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import os
from pathlib import Path

from .base import BaseTest, hgtest
from .repo import Repo
from .types import PathLike
from .workingcopy import WorkingCopy


class TestLibTests(BaseTest):
    @hgtest
    def test_repo_setup(self, repo: Repo, wc: WorkingCopy) -> None:
        self.assertEqual(repo.root, wc.root)
        self.assertTrue(os.path.exists(os.path.join(repo.root, ".hg")))

    @hgtest
    def test_working_copy_edits(self, repo: Repo, wc: WorkingCopy) -> None:
        def join(path: PathLike) -> Path:
            return os.path.join(wc.root, path)

        def exists(path: PathLike) -> bool:
            return os.path.exists(os.path.join(wc.root, path))

        def read(path: PathLike) -> str:
            return open(join(path)).read()

        # Test auto-generating path and content, with hg add
        file = wc.file()
        self.assertTrue(exists(file.path))
        self.assertEqual(read(file.path), file.path)
        self.assertEquals(wc.hg.status().stdout, f"A {file.path}\n")

        # Test remove
        file.remove()
        self.assertFalse(file.exists())
        wc.remove(file)
        self.assertEquals(wc.hg.status().stdout, f"")

        # Test adding a file in a directory
        file = wc.file(path="subdir/file")
        self.assertTrue(exists("subdir/file"))
        file.remove()
        wc.remove(file)

        # Test manual path and content, without hg add
        file = wc.file(path="foo", content="bar", add=False)
        self.assertTrue(exists("foo"))
        self.assertEqual(read("foo"), "bar")
        self.assertEquals(wc.hg.status().stdout, "? foo\n")

        # Test wc.add()
        wc.add(file)
        self.assertEquals(wc.hg.status().stdout, f"A foo\n")

        # Test reads
        self.assertEqual(file.content(), "bar")
        self.assertEqual(file.binary(), b"bar")

        # Test writes
        file.write("bar2")
        self.assertEqual(read(file.path), "bar2")
        file.append("3")
        self.assertEqual(read(file.path), "bar23")

    @hgtest
    def test_working_copy_commit(self, repo: Repo, wc: WorkingCopy) -> None:
        file = wc.file()
        commit = wc.commit()
        self.assertEquals(wc.hg.status().stdout, "")
        self.assertEquals(wc.hg.status(change=commit.hash).stdout, f"A {file.path}\n")

        file = wc.file(add=False)
        commit = wc.commit(
            message="my message",
            author="my author",
            date="1980-1-1 UTC",
            addremove=True,
        )
        self.assertEqual(
            wc.hg.log(
                rev=commit.hash, template="{desc}\n{author}\n{date|isodate}"
            ).stdout,
            "my message\nmy author\n1980-01-01 00:00 +0000",
        )
        self.assertEquals(wc.hg.status(change=commit.hash).stdout, f"A {file.path}\n")

    @hgtest
    def test_working_copy_bookmark(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file()
        commit = wc.commit()

        wc.hg.bookmark("foo")
        self.assertEqual(repo.bookmarks()["foo"], commit)

    @hgtest
    def test_drawdag(self, repo: Repo, wc: WorkingCopy) -> None:
        repo.drawdag(
            """
C
|
B
|
A
"""
        )

        self.assertEqual(
            repo.hg.smartlog(template="{desc}").stdout,
            """o  C
│
o  B
│
o  A

""",
        )


if __name__ == "__main__":
    import unittest

    unittest.main()
