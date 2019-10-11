#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test
from .lib.histedit_command import HisteditCommand


@hg_test
# pyre-fixme[38]: `HisteditTest` does not implement all inherited abstract methods.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class HisteditTest(EdenHgTestCase):
    # pyre-fixme[13]: Attribute `_commit1` is never initialized.
    _commit1: str
    # pyre-fixme[13]: Attribute `_commit2` is never initialized.
    _commit2: str
    # pyre-fixme[13]: Attribute `_commit3` is never initialized.
    _commit3: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("first", "")
        self._commit1 = repo.commit("first commit")

        repo.write_file("second", "")
        self._commit2 = repo.commit("second commit")

        repo.write_file("third", "")
        self._commit3 = repo.commit("third commit")

    def test_stop_at_earlier_commit_in_the_stack_without_reordering(self) -> None:
        commits = self.repo.log()
        self.assertEqual([self._commit1, self._commit2, self._commit3], commits)

        # histedit, stopping in the middle of the stack.
        histedit = HisteditCommand()
        histedit.pick(self._commit1)
        histedit.stop(self._commit2)
        histedit.pick(self._commit3)

        # We expect histedit to terminate with a nonzero exit code in this case.
        with self.assertRaises(hgrepo.HgError) as context:
            histedit.run(self)
        head = self.repo.log(revset=".")[0]
        expected_msg = (
            "Changes committed as %s. " "You may amend the changeset now." % head[:12]
        )
        self.assertIn(expected_msg, str(context.exception))

        # Verify the new commit stack and the histedit termination state.
        # Note that the hash of commit[0] is unpredictable because Hg gives it a
        # new hash in anticipation of the user amending it.
        parent = self.repo.log(revset=".^")[0]
        self.assertEqual(self._commit1, parent)
        self.assertEqual(["first commit", "second commit"], self.repo.log("{desc}"))

        # Make sure the working copy is in the expected state.
        self.assert_status_empty(op="histedit")
        self.assertSetEqual(
            {".eden", ".hg", "first", "second"},
            set(os.listdir(self.repo.get_canonical_root())),
        )

        self.hg("histedit", "--continue")
        self.assertEqual(
            ["first commit", "second commit", "third commit"], self.repo.log("{desc}")
        )
        self.assert_status_empty()
        self.assertSetEqual(
            {".eden", ".hg", "first", "second", "third"},
            set(os.listdir(self.repo.get_canonical_root())),
        )

    def test_reordering_commits_without_merge_conflicts(self) -> None:
        self.assertEqual(
            ["first commit", "second commit", "third commit"], self.repo.log("{desc}")
        )

        # histedit, reordering the stack in a conflict-free way.
        histedit = HisteditCommand()
        histedit.pick(self._commit2)
        histedit.pick(self._commit3)
        histedit.pick(self._commit1)
        histedit.run(self)

        self.assertEqual(
            ["second commit", "third commit", "first commit"], self.repo.log("{desc}")
        )
        self.assert_status_empty()
        self.assertSetEqual(
            {".eden", ".hg", "first", "second", "third"},
            set(os.listdir(self.repo.get_canonical_root())),
        )

    def test_drop_commit_without_merge_conflicts(self) -> None:
        self.assertEqual(
            ["first commit", "second commit", "third commit"], self.repo.log("{desc}")
        )

        # histedit, reordering the stack in a conflict-free way.
        histedit = HisteditCommand()
        histedit.pick(self._commit1)
        histedit.drop(self._commit2)
        histedit.pick(self._commit3)
        histedit.run(self)

        self.assertEqual(["first commit", "third commit"], self.repo.log("{desc}"))
        self.assert_status_empty()
        self.assertSetEqual(
            {".eden", ".hg", "first", "third"},
            set(os.listdir(self.repo.get_canonical_root())),
        )

    def test_roll_two_commits_into_parent(self) -> None:
        self.assertEqual(
            ["first commit", "second commit", "third commit"], self.repo.log("{desc}")
        )

        # histedit, reordering the stack in a conflict-free way.
        histedit = HisteditCommand()
        histedit.pick(self._commit1)
        histedit.roll(self._commit2)
        histedit.roll(self._commit3)
        histedit.run(self)

        self.assertEqual(["first commit"], self.repo.log("{desc}"))
        self.assert_status_empty()
        self.assertSetEqual(
            {".eden", ".hg", "first", "second", "third"},
            set(os.listdir(self.repo.get_canonical_root())),
        )

    def test_abort_after_merge_conflict(self) -> None:
        self.write_file("will_have_confict.txt", "original\n")
        self.hg("add", "will_have_confict.txt")
        commit4 = self.repo.commit("commit4")
        self.write_file("will_have_confict.txt", "1\n")
        commit5 = self.repo.commit("commit5")
        self.write_file("will_have_confict.txt", "2\n")
        commit6 = self.repo.commit("commit6")

        histedit = HisteditCommand()
        histedit.pick(commit4)
        histedit.pick(commit6)
        histedit.pick(commit5)
        original_commits = self.repo.log()

        with self.assertRaises(hgrepo.HgError) as context:
            histedit.run(self, ancestor=commit4)
        expected_msg = (
            "Fix up the change (pick %s)\n" % commit6[:12]
        ) + "  (hg histedit --continue to resume)"
        self.assertIn(expected_msg, str(context.exception))
        self.assert_status({"will_have_confict.txt": "M"}, op="histedit")
        self.assert_file_regex(
            "will_have_confict.txt",
            """\
            <<<<<<< local: .*
            original
            =======
            2
            >>>>>>> histedit: .*
            """,
        )

        self.hg("histedit", "--abort")
        self.assertEqual("2\n", self.read_file("will_have_confict.txt"))
        self.assertListEqual(
            original_commits,
            self.repo.log(),
            msg="The original commit hashes should be restored by the abort.",
        )
        self.assert_status_empty()
