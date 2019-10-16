#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from eden.integration.lib.hgrepo import HgRepository

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class MergeTest(EdenHgTestCase):
    """Note that Mercurial has a number of built-in merge tools:
    https://www.mercurial-scm.org/repo/hg/help/merge-tools
    """

    commit0: str
    commit1: str
    commit2: str

    def populate_backing_repo(self, repo: HgRepository) -> None:
        repo.write_file("foo", "original")
        self.commit0 = repo.commit("root commit")

        repo.write_file("foo", "1")
        self.commit1 = repo.commit("commit1")
        repo.update(self.commit0)

        repo.write_file("foo", "2")
        self.commit2 = repo.commit("commit2")

    def test_merge_local(self) -> None:
        self._do_merge_and_commit(":local")
        self._verify_tip("2")

    def test_merge_other(self) -> None:
        self._do_merge_and_commit(":other")
        self._verify_tip("1")

    def test_merge_union(self) -> None:
        self._do_merge_and_commit(":union")
        self._verify_tip("21")

    def _do_merge_and_commit(self, tool: str) -> None:
        self.hg("merge", "--tool", tool, "-r", self.commit1)
        self.assert_status({"foo": "M"}, op="merge")
        self.repo.commit("merge commit1 into commit2")
        self.assert_status_empty()

    def test_resolve_merge(self) -> None:
        # Perform the merge and let it fail with the file unresolved
        self.hg("merge", "--tool", ":fail", "-r", self.commit1, check=False)
        self.assert_status({"foo": "M"}, op="merge")
        self.assert_unresolved(["foo"])

        self.write_file("foo", "3")
        self.hg("resolve", "--mark", "foo")
        self.assert_unresolved(unresolved=[], resolved=["foo"])
        self.assert_status({"foo": "M"}, op="merge")
        self.repo.commit("merge commit1 into commit2")
        self._verify_tip("3")

    def test_clear_merge_state(self) -> None:
        # Perform the merge and let it fail with the file unresolved
        self.hg("merge", "--tool", ":fail", "-r", self.commit1, check=False)
        self.assert_status({"foo": "M"}, op="merge")
        self.assert_unresolved(["foo"])

        # "hg update --clean ." should reset is back to a clean state
        # with no outstanding merge conflicts.
        self.hg("update", "--clean", ".")
        self.assertEqual(self.commit2, self.repo.get_head_hash())
        self.assert_status_empty()
        self.assert_unresolved([])

    def _verify_tip(self, expected_contents: str) -> None:
        files = self.repo.log(template="{files}", revset="tip")[0]
        self.assertEqual("foo", files)

        p1, p2 = self.repo.log(template="{p1node}\n{p2node}", revset="tip")[0].split(
            "\n"
        )
        self.assertEqual(self.commit2, p1)
        self.assertEqual(self.commit1, p2)
        self.assertEqual(expected_contents, self.read_file("foo"))
