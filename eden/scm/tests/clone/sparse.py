# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.repo import Repo
from eden.testlib.util import new_dir
from eden.testlib.workingcopy import WorkingCopy


class TestSparseClone(BaseTest):
    def setUp(self) -> None:
        super().setUp()
        self.config.enable("sparse")
        self.config.add("remotenames", "selectivepulldefault", "master")
        self.config.add("commands", "force-rust", "clone")

    @hgtest
    def test_simple(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file(path="sparse/base", content="sparse/\ninc/\n")
        wc.file(path="inc/foo")
        wc.file(path="inc/bar")
        wc.file(path="exc/foo")
        commit1 = wc.commit()

        wc.hg.push(rev=commit1.hash, to="master", create=True)

        sparse_wc = WorkingCopy(repo, new_dir())
        repo.hg.clone(repo.url, sparse_wc.root, enable_profile="sparse/base")

        self.assertEqual(sparse_wc.files(), ["inc/bar", "inc/foo", "sparse/base"])
        self.assertTrue(sparse_wc.status().empty())

        # Make sure Python agrees what files we should have.
        sparse_wc.hg.sparse("refresh")
        self.assertEqual(sparse_wc.files(), ["inc/bar", "inc/foo", "sparse/base"])

    @hgtest
    def test_config_override(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file(path="sparse/base", content="sparse/\n")
        wc.file(path="a")
        wc.file(path="b")
        commit1 = wc.commit()

        wc.hg.push(rev=commit1.hash, to="master", create=True)

        # We support sparse rules coming from dynamic config.
        self.config.add("sparseprofile", "include.blah.sparse/base", "a")

        sparse_wc = WorkingCopy(repo, new_dir())
        repo.hg.clone(repo.url, sparse_wc.root, enable_profile="sparse/base")

        self.assertEqual(sparse_wc.files(), ["a", "sparse/base"])
        self.assertTrue(sparse_wc.status().empty())

        # Make sure Python agrees what files we should have.
        sparse_wc.hg.sparse("refresh")
        self.assertEqual(sparse_wc.files(), ["a", "sparse/base"])

    @hgtest
    def test_multiple_profiles(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file(path="sparse/a", content="sparse/a\na\n")
        wc.file(path="sparse/b", content="sparse/b\nb\n")
        wc.file(path="a")
        wc.file(path="b")
        wc.file(path="c")
        commit1 = wc.commit()

        wc.hg.push(rev=commit1.hash, to="master", create=True)

        sparse_wc = WorkingCopy(repo, new_dir())
        repo.hg.clone(repo.url, sparse_wc.root, enable_profile=["sparse/a", "sparse/b"])

        self.assertEqual(sparse_wc.files(), ["a", "b", "sparse/a", "sparse/b"])
        self.assertTrue(sparse_wc.status().empty())

    @hgtest
    def test_clone_within_repo(self, repo: Repo, wc: WorkingCopy) -> None:
        repo.config.add("commands", "force-rust", "\0oops")

        other_repo = self.server.clone(1)
        other_wc = WorkingCopy(other_repo, new_dir())

        # clone repo1 from within repo0
        # we shouldn't see the invalid config value from repo0's config
        wc.hg.clone(other_repo.url, other_wc.root)

    @hgtest
    def test_no_working_copy(self, repo: Repo, wc: WorkingCopy) -> None:
        other_wc = WorkingCopy(repo, new_dir())
        other_wc.hg.clone(
            repo.url, other_wc.root, noupdate=True, enable_profile="banana"
        )

        # Make sure we have a dirstate file and a null parent.
        self.assertTrue(other_wc[".hg/dirstate"].exists())
        self.assertEqual(
            other_wc.hg.whereami().stdout.strip(),
            "0000000000000000000000000000000000000000",
        )

        # Make sure we wrote out the sparse config.
        self.assertIn("banana", other_wc[".hg/sparse"].content())

    @hgtest
    def test_matcher(self, repo: Repo, wc: WorkingCopy) -> None:
        wc.file(
            path="sparse/base",
            content="""
sparse/
a////////////x
a/**y
b/*
""",
        )
        wc.file(path="a/x")
        wc.file(path="a/y")
        wc.file(path="b/1")
        wc.file(path="b/2")
        commit1 = wc.commit()

        wc.hg.push(rev=commit1.hash, to="master", create=True)

        sparse_wc = WorkingCopy(repo, new_dir())
        repo.hg.clone(repo.url, sparse_wc.root, enable_profile="sparse/base")

        self.assertEqual(sparse_wc.files(), ["a/x", "a/y", "b/1", "b/2", "sparse/base"])
        self.assertTrue(sparse_wc.status().empty())

        # Make sure Python agrees what files we should have.
        sparse_wc.hg.sparse("refresh")
        self.assertEqual(sparse_wc.files(), ["a/x", "a/y", "b/1", "b/2", "sparse/base"])
