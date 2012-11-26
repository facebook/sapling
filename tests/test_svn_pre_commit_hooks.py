import os
import sys
import test_util
import unittest

from mercurial import hg
from mercurial import commands
from mercurial import util


class TestSvnPreCommitHooks(test_util.TestBase):
    def setUp(self):
        super(TestSvnPreCommitHooks, self).setUp()
        self.repo_path = self.load_and_fetch('single_rev.svndump')[1]
        # creating pre-commit hook that doesn't allow any commit
        hook_file_name = os.path.join(
			self.repo_path, 'hooks', 'pre-commit'
        )
        hook_file = open(hook_file_name, 'w')
        hook_file.write(
        	'#!/bin/sh\n'
        	'echo "Commits are not allowed" >&2; exit 1;\n'
        )
        hook_file.close()
        os.chmod(hook_file_name, 0755)

    def test_push_with_pre_commit_hooks(self):
        changes = [('narf/a', 'narf/a', 'ohai',),
                   ]
        self.commitchanges(changes)
        self.assertRaises(util.Abort, self.pushrevisions)

def suite():
    return unittest.findTestCases(sys.modules[__name__])
