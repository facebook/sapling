#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import os

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test
from .lib.histedit_command import HisteditCommand


class _Hidden:
    # This _Hidden class exists soley to hide the abstract StorageEngineTest class from
    # the unittest framework, so it does not find it during test discovery.  The
    # unittest code is unfortunately not smart enough to skip abstract classes.

    class StorageEngineTest(EdenHgTestCase, metaclass=abc.ABCMeta):
        _commit1: str
        _commit2: str
        _commit3: str

        # These tests were originally copied from histedit_test.py. It doesn't
        # matter which tests are used as long as commits are created and checked out
        # and a realistic workflow is verified against each storage engine.
        def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
            repo.write_file("first", "")
            self._commit1 = repo.commit("first commit")

            repo.write_file("second", "")
            self._commit2 = repo.commit("second commit")

            repo.write_file("third", "")
            self._commit3 = repo.commit("third commit")

        @abc.abstractmethod
        def select_storage_engine(self) -> str:
            raise NotImplementedError()

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
                "Changes committed as %s. "
                "You may amend the changeset now." % head[:12]
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
                ["first commit", "second commit", "third commit"],
                self.repo.log("{desc}"),
            )
            self.assert_status_empty()
            self.assertSetEqual(
                {".eden", ".hg", "first", "second", "third"},
                set(os.listdir(self.repo.get_canonical_root())),
            )


# Each LocalStore implementation may complete its futures from different
# threads. Verify that Eden works the same with all of them.


@hg_test
# pyre-fixme[38]: `HisteditMemoryStorageEngineTest` does not implement all inherited
#  abstract methods.
# pyre-fixme[13]: Attribute `_commit1` is never initialized.
# pyre-fixme[13]: Attribute `_commit2` is never initialized.
# pyre-fixme[13]: Attribute `_commit3` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class HisteditMemoryStorageEngineTest(_Hidden.StorageEngineTest):
    def select_storage_engine(self) -> str:
        return "memory"


@hg_test
# pyre-fixme[38]: `HisteditSQLiteStorageEngineTest` does not implement all inherited
#  abstract methods.
# pyre-fixme[13]: Attribute `_commit1` is never initialized.
# pyre-fixme[13]: Attribute `_commit2` is never initialized.
# pyre-fixme[13]: Attribute `_commit3` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class HisteditSQLiteStorageEngineTest(_Hidden.StorageEngineTest):
    def select_storage_engine(self) -> str:
        return "sqlite"


@hg_test
# pyre-fixme[38]: `HisteditRocksDBStorageEngineTest` does not implement all
#  inherited abstract methods.
# pyre-fixme[13]: Attribute `_commit1` is never initialized.
# pyre-fixme[13]: Attribute `_commit2` is never initialized.
# pyre-fixme[13]: Attribute `_commit3` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class HisteditRocksDBStorageEngineTest(_Hidden.StorageEngineTest):
    def select_storage_engine(self) -> str:
        return "rocksdb"
