#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import logging

from eden.integration.lib import hgrepo

from .lib.hg_extension_test_base import EdenHgTestCase, hg_test


log = logging.getLogger("eden.test.absorb")


@hg_test
# pyre-fixme[13]: Attribute `backing_repo` is never initialized.
# pyre-fixme[13]: Attribute `backing_repo_name` is never initialized.
# pyre-fixme[13]: Attribute `config_variant_name` is never initialized.
# pyre-fixme[13]: Attribute `repo` is never initialized.
class AbsorbTest(EdenHgTestCase):
    commit1: str

    def populate_backing_repo(self, repo: hgrepo.HgRepository) -> None:
        repo.write_file("readme.txt", "readme\n")
        repo.write_file(
            "src/test.c",
            """\
start of the file
line 1
line 2
line 3
end of the file
""",
        )
        self.commit1 = repo.commit("Initial commit.")
        repo.hg("phase", "--public", self.commit1)
        log.debug("commit1: %s", self.commit1)

    def test_absorb(self) -> None:
        self.assert_status_empty()

        # Update src/test.c in our first draft commit
        self.write_file(
            "src/test.c",
            """\
start of the file
line 1
new line a
line 2
new line b
line 3
end of the file
""",
        )
        self.assert_status({"src/test.c": "M"})
        commit2 = self.repo.commit("new lines in test.c\n")
        self.assert_status_empty()
        log.debug("commit2: %s", commit2)

        # Update src/new.c in our second draft commit
        self.write_file(
            "src/new.c",
            """\
this is a brand new file
with some new contents
last line
""",
        )
        self.hg("add", "src/new.c")
        self.assert_status({"src/new.c": "A"})
        commit3 = self.repo.commit("add new.c\n")
        self.assert_status_empty()
        log.debug("commit2: %s", commit3)

        # Now modify test.c and new.c in the working copy
        self.write_file(
            "src/test.c",
            """\
start of the file
line 1
new line abc
testing
line 2
new line b
line 3
end of the file
""",
        )
        self.write_file(
            "src/new.c",
            """\
this is a brand new file
with some enhanced new contents
last line
""",
        )
        self.assert_status({"src/new.c": "M", "src/test.c": "M"})
        old_commits = self.repo.log()

        # Run "hg absorb" to fold these changes into their respective commits
        out = self.hg("absorb", "-ap")
        log.debug("absorb output:\n%s" % (out,))
        self.assert_status_empty()

        # Verify the results are what we expect
        new_commits = self.repo.log()
        files_changed = self.repo.log(template="{files}")
        self.assertEqual(len(old_commits), len(new_commits))
        self.assertEqual(old_commits[0], new_commits[0])
        self.assertNotEqual(old_commits[1], new_commits[1])
        self.assertNotEqual(old_commits[2], new_commits[2])
        self.assertEqual(files_changed[0], "readme.txt src/test.c")
        self.assertEqual(files_changed[1], "src/test.c")
        self.assertEqual(files_changed[2], "src/new.c")

        self.assertEqual(
            self.read_file("src/test.c"),
            """\
start of the file
line 1
new line abc
testing
line 2
new line b
line 3
end of the file
""",
        )
        self.assertEqual(
            self.read_file("src/new.c"),
            """\
this is a brand new file
with some enhanced new contents
last line
""",
        )
