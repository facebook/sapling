# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

from pathlib import Path

from eden.testlib.base import BaseTest, hgtest
from eden.testlib.repo import Repo
from eden.testlib.status import Status
from eden.testlib.workingcopy import WorkingCopy


class TestDirSync(BaseTest):
    def setUp(self) -> None:
        super().setUp()
        self.config.enable("dirsync")

    @hgtest
    def test_commit(self, repo: Repo, wc: WorkingCopy) -> None:
        fileA = wc.file(path="dirA/file.txt")
        wc.file(
            path=".hgdirsync",
            content="""
foo.A = dirA
foo.B = dirB
""",
        )
        fileB = wc[f"dirB/file.txt"]
        self.assertFalse(fileB.exists())

        wc.commit()
        self.assertEquals(fileA.content(), fileB.content())

    @hgtest
    def test_amend(self, repo: Repo, wc: WorkingCopy) -> None:
        fileA = wc.file(path="dirA/file.txt")
        wc.file(
            path=".hgdirsync",
            content="""
foo.A = dirA
foo.B = dirB
""",
        )
        fileB = wc[f"dirB/file.txt"]
        wc.commit()
        self.assertEquals(fileA.content(), fileB.content())

        fileA.write("modified")
        self.assertNotEquals(fileA.content(), fileB.content())
        wc.amend()
        self.assertEquals(fileA.content(), fileB.content())
        self.assertEquals(fileB.content(), "modified")

    @hgtest
    def test_amend_permutations(self, repo: Repo, wc: WorkingCopy) -> None:
        dirA = Path("dirA")
        dirB = Path("dirB")
        # Add an initial file, so we can express modified and removed states
        # later.
        fileA = wc.file(path=dirA / "fileA")
        original_content = fileA.content()
        wc.file(
            path=".hgdirsync",
            content=f"""
foo.A = {dirA}
foo.B = {dirB}
""",
        )
        base = wc.commit()

        ADD = "add"
        MODIFY = "modify"
        REMOVE = "remove"
        REVERT = "revert"
        options = [ADD, MODIFY, REMOVE, REVERT]
        actions = []
        for option1 in options:
            for option2 in options:
                actions.append((option1, option2))

        # Certain combinations are no-ops or invalid, and can be removed.
        # Supporting them would make the loop below more complicated.
        actions.remove((ADD, ADD))
        actions.remove((REMOVE, REMOVE))
        actions.remove((MODIFY, ADD))
        actions.remove((REMOVE, ADD))

        for (initial, amend) in actions:
            print(f"Initial Action: {initial}, Amend Action: {amend}")

            # Setup the initial pre-amend state of the file.
            if initial == ADD:
                file = wc.file(path=dirA / "newfile")
            elif initial == MODIFY:
                fileA.append("modify")
                file = fileA
            elif initial == REMOVE:
                wc.remove(fileA)
                file = fileA
            elif initial == REVERT:
                # A revert only makes sense during the amend phase.
                continue
            else:
                raise ValueError(f"unknown initial action {initial}")

            wc.commit()

            # Modify the file before the amend.
            if amend == ADD:
                file = wc.file(path=file.path, content="new content")
                new_state = Status.ADDED
                new_content = file.content()
            elif amend == MODIFY:
                file.append("modify more")
                new_state = Status.ADDED if initial == ADD else Status.MODIFIED
                new_content = file.content()
            elif amend == REMOVE:
                wc.remove(file)
                new_state = None if initial == ADD else Status.REMOVED
                new_content = None
            elif amend == REVERT:
                wc.hg.revert(file, rev=base)
                new_state = None
                new_content = None if initial == ADD else original_content
            else:
                raise ValueError(f"unknown amend action {amend}")

            amend = wc.amend(addremove=True)

            # Verify that the status for each file matches, and the
            # contents/existence matches.
            status = amend.status()
            other_file = wc[dirB / file.basename()]
            self.assertEquals(status[file.path], new_state)
            self.assertEquals(status[file.path], status[other_file.path])

            if new_content is not None:
                self.assertEquals(file.content(), new_content)
                self.assertEquals(file.content(), other_file.content())
            else:
                self.assertFalse(file.exists())
                self.assertFalse(other_file.exists())

            # Reset the working copy
            wc.hg.hide(rev=".")
