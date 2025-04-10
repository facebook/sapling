# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import functools
import os
import unittest

import bindings
import silenttestrunner


class PyDagTests(unittest.TestCase):
    def _tmpdir(self, name: str) -> str:
        return os.path.join(os.getenv("TESTTMP") or "", name)

    def test_torevs_should_follow_id_reassignment(self):
        commits = bindings.dag.commits.opensegments(
            self._tmpdir("segments"), self._tmpdir("commits"), "git"
        )
        # Add a single commit. By default, it is in the "NON_MASTER" group.
        a_text = b"Commit A"
        a_node = git_commit_hash(a_text)
        commits.addcommits([(a_node, [], a_text)])
        a_nodes = commits.dag().sort([a_node])
        self.assertEqual(list(commits.torevs(a_nodes)), [non_master_id(0)])
        # Flush the commit to the MASTER group. This reassigns the node to "MASTER".
        commits.flush([a_node])
        self.assertEqual(list(commits.torevs([a_node])), [0])
        # `torevs(old_nodes)` respects the id reassignment too.
        self.assertEqual(list(commits.torevs(a_nodes)), [0])


git_commit_hash = functools.partial(bindings.formatutil.git_sha1_digest, kind="commit")


def non_master_id(id: int) -> int:
    # See Rust dag's "Group". Ids in the "NON_MASTER" group (u64) have the top byte set to 1.
    return id + (1 << 56)


if __name__ == "__main__":
    silenttestrunner.main(__name__)
