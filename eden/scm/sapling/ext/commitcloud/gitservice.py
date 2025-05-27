# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from . import baseservice, saplingremoteapiservice


class GitService(
    saplingremoteapiservice.SaplingRemoteAPIService, baseservice.BaseService
):
    """Remote commit-cloud service for git repos"""

    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo

    def check(self):
        return True

    def getsmartlog(self, reponame, workspace, repo, limit, flags=[]):
        raise NotImplementedError  # Not supported for git repos yet

    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        raise NotImplementedError  # Not supported for git repos yet

    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        raise NotImplementedError  # Not supported for git repos yet

    def getworkspaces(self, reponame, prefix):
        raise NotImplementedError  # Not supported for git repos yet

    def getworkspace(self, reponame, workspacename):
        self.ui.debug("Calling 'cloudworkspace' on edenapi\n", component="commitcloud")
        stream = self.repo.edenapi.cloudworkspace(workspacename, reponame)
        return list(stream)

    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace"""
        raise NotImplementedError  # Not supported for git repos yet

    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""
        raise NotImplementedError  # Not supported for git repos yet

    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""
        raise NotImplementedError  # Since auth is disabled in tests

    def rollbackworkspace(self, reponame, workspace, version):
        """Rollback the given workspace to a specific version"""
        raise NotImplementedError  # Since commit cloud history is not supported in the tests yet

    def cleanupworkspace(self, reponame, workspace):
        """Cleanup unnecessary remote bookmarks from the given workspace"""
        raise NotImplementedError  # Not supported in the tests yet
