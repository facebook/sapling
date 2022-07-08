# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.hg import CommandFailure
from eden.testlib.repo import Repo
from eden.testlib.util import new_dir
from eden.testlib.workingcopy import WorkingCopy


class TestEdenClone(BaseTest):
    def setUp(self) -> None:
        super().setUp()
        self.config.add("clone", "use-rust", "True")

    @hgtest
    def test_master_checkouts(self, repo: Repo, wc: WorkingCopy) -> None:

        file = wc.file()
        master_commit = wc.commit()
        wc.hg.push(rev=master_commit.hash, to="master", create=True)

        # create eden checkout in a destination that does not yet exist
        checkout_dir = new_dir()
        eden_checkout_dir1 = checkout_dir.joinpath("checkout1")

        repo.hg.clone(
            repo.url,
            eden_checkout_dir1,
            eden=True,
        )

        eden_repo1 = Repo(eden_checkout_dir1, repo.url, repo.name)
        eden_wc_1 = WorkingCopy(eden_repo1, eden_checkout_dir1)

        # pyre-ignore[16]
        eden_client = wc.eden
        self.assertIn(str(eden_checkout_dir1), eden_client.list_cmd_simple().keys())
        master_hash = repo.hg.book(list_remote="master", template="{node}").stdout
        self.assertEqual(eden_wc_1.current_commit().hash, master_hash)
        self.assertIn(str(file), eden_repo1.hg.files().stdout)

        # test cloning in destination directory that already exists
        eden_checkout_dir2 = new_dir()

        repo.hg.clone(
            repo.url,
            eden_checkout_dir2,
            eden=True,
        )
        self.assertIn(str(eden_checkout_dir2), eden_client.list_cmd_simple().keys())

        # test sharing backing repo
        eden_repo2 = Repo(eden_checkout_dir2, repo.url, repo.name)
        eden_wc_1.file()
        eden_wc_1.commit()
        self.assertEqual(
            set(eden_repo1.commits("all()")), set(eden_repo2.commits("all()"))
        )

    @hgtest
    def test_empty_checkout(self, repo: Repo, wc: WorkingCopy) -> None:
        empty_eden = new_dir()
        empty_clone_stderr = repo.hg.clone(
            repo.url,
            empty_eden,
            eden=True,
        ).stderr

        # check that user is warned about skipped checkout
        self.assertIn(
            "Server has no 'remote/master' bookmark - skipping checkout",
            empty_clone_stderr,
        )

        # pyre-ignore[16]
        eden_checkouts = wc.eden.list_cmd_simple().keys()
        self.assertIn(str(empty_eden), eden_checkouts)

    @hgtest
    def test_missing_executable(self, repo: Repo, wc: WorkingCopy) -> None:
        try:
            repo.hg.clone(
                repo.url, new_dir(), eden=True, config="edenfs.command=missing.exe"
            )
        except CommandFailure as err:
            self.assertIn('failed to execute "missing.exe"', str(err))
            self.assertIn("No such file or directory", str(err))
