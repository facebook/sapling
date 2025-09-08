# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from . import baseservice, error, saplingremoteapiservice


class GitService(
    saplingremoteapiservice.SaplingRemoteAPIService, baseservice.BaseService
):
    """Remote commit-cloud service for git repos"""

    def __init__(self, ui, repo):
        super().__init__(ui, repo)

    def check(self):
        return True

    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        raise error.GitUnsupportedError(self.ui)

    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        raise error.GitUnsupportedError(self.ui)

    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace"""
        raise error.GitUnsupportedError(self.ui)

    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""
        raise error.GitUnsupportedError(self.ui)

    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""
        raise NotImplementedError  # Since auth is disabled in tests

    def rollbackworkspace(self, reponame, workspace, version):
        """Rollback the given workspace to a specific version"""
        raise NotImplementedError  # Since commit cloud history is not supported in the tests yet

    def cleanupworkspace(self, reponame, workspace):
        """Cleanup unnecessary remote bookmarks from the given workspace"""
        raise NotImplementedError  # Not supported in the tests yet
