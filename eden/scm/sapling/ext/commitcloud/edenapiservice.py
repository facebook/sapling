# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Standard Library

from sapling import error

from . import baseservice


class EdenApiService(baseservice.BaseService):
    """Remote commit-cloud service implemented using edenapi."""

    def __init__(self, ui, repo):
        self.ui = ui
        if repo is None:
            raise error.Abort("Tried to start edenapiservice with no repo object")
        self.repo = repo
        self.repo.edenapi.capabilities()  # Check edenapi is reachable

    def check(self):
        return True

    def getreferences(
        self,
        reponame,
        workspace,
        baseversion,
        clientinfo=None,
    ):
        self.ui.debug("Calling 'get_references' on edenapi\n", component="commitcloud")
        response = self.repo.edenapi.cloudreferences(
            {
                "workspace": workspace,
                "reponame": reponame,
                "version": baseversion,
                "client_info": None,
            }
        )
        version = response["version"]

        if version == 0:
            self.ui.debug(
                "'get_references' returns that workspace '%s' is not known by server\n"
                % workspace,
                component="commitcloud",
            )
            return self._makeemptyreferences(version)

        if version == baseversion:
            self.ui.debug(
                "'get_references' confirms the current version %s is the latest\n"
                % version,
                component="commitcloud",
            )
            return self._makeemptyreferences(version)

        self.ui.debug(
            "'get_references' returns version %s, current version %s\n"
            % (version, baseversion),
            component="commitcloud",
        )
        return self._makereferences(self._castreferences(response))

    def updatereferences(
        self,
        reponame,
        workspace,
        version,
        oldheads=None,
        newheads=None,
        oldbookmarks=None,
        newbookmarks=None,
        oldremotebookmarks=None,
        newremotebookmarks=None,
        clientinfo=None,
        logopts={},
    ):
        raise NotImplementedError  # Not supported in edenapi service yet

    def getsmartlog(self, reponame, workspace, repo, limit, flags=[]):
        raise NotImplementedError  # Not supported in edenapi service yet

    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        raise NotImplementedError  # Not supported in edenapi service yet

    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        raise NotImplementedError  # Not supported in edenapi service yet

    def getworkspaces(self, reponame, prefix):
        raise NotImplementedError  # Not supported in edenapi service yet

    def getworkspace(self, reponame, workspacename):
        self.ui.debug("Calling 'cloudworkspace' on edenapi\n", component="commitcloud")
        stream = self.repo.edenapi.cloudworkspace(workspacename, reponame)
        return list(stream)

    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace"""
        raise NotImplementedError  # Not supported in edenapi service yet

    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""
        raise NotImplementedError  # Not supported in edenapi service yet

    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""
        raise NotImplementedError  # Since auth is disabled in tests

    def rollbackworkspace(self, reponame, workspace, version):
        """Rollback the given workspace to a specific version"""
        raise NotImplementedError  # Since commit cloud history is not supported in the tests yet

    def cleanupworkspace(self, reponame, workspace):
        """Cleanup unnecessary remote bookmarks from the given workspace"""
        raise NotImplementedError  # Not supported in the tests yet

    def _castreferences(self, refs):
        """
        1. Create list of heads from head_dates data. Server may omit heads to reduce data transmission.
        2. The server returns changeset ids as hex encoded bytes, but we need them as str, so we convert them here.
        """
        if not refs.get("heads") and refs.get("head_dates"):
            refs["heads"] = refs.get("head_dates", {}).keys()

        local_bookmarks = dict(
            map(lambda item: (item[0], item[1].hex()), refs["bookmarks"].items())
        )
        heads_dates = dict(
            map(lambda item: (item[0].hex(), item[1]), refs["heads_dates"].items())
        )
        heads = list(map(lambda item: item.hex(), refs["heads"]))
        snapshots = list(map(lambda item: item.hex(), refs["snapshots"]))
        remote_bookmarks = []
        for remote_bookmark in refs["remote_bookmarks"]:
            if remote_bookmark["node"] is not None:
                remote_bookmark["node"] = remote_bookmark["node"].hex()
            remote_bookmarks.append(remote_bookmark)

        refs["remote_bookmarks"] = remote_bookmarks
        refs["bookmarks"] = local_bookmarks
        refs["heads_date"] = heads_dates
        refs["snapshots"] = snapshots
        refs["heads"] = heads
        return refs
