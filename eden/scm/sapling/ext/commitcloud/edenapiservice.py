# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from bindings import clientinfo as clientinfomod

# Standard Library

from sapling import error

from . import baseservice


class EdenApiService(baseservice.BaseService):
    """Remote commit-cloud service implemented using edenapi."""

    def __init__(self, ui, repo, fallback_service):
        self.ui = ui
        if repo is None:
            raise error.Abort("Tried to start edenapiservice with no repo object")
        self.repo = repo
        self.repo.edenapi.capabilities()  # Check edenapi is reachable
        self.fallback_service = fallback_service

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
        self.ui.debug("sending 'update_references' request\n", component="commitcloud")
        oldheads = oldheads or []
        newheads = newheads or []
        oldbookmarks = oldbookmarks or []
        newbookmarks = newbookmarks or {}
        oldremotebookmarks = oldremotebookmarks or []
        newremotebookmarks = newremotebookmarks or {}

        # remove duplicates, must preserve order in the newheads list
        newheadsset = set(newheads)
        commonset = set([item for item in oldheads if item in newheadsset])

        newheads = [h for h in newheads if h not in commonset]
        oldheads = [h for h in oldheads if h not in commonset]

        client_correlator = clientinfomod.get_client_correlator().decode()
        client_entry_point = clientinfomod.get_client_entry_point().decode()
        self.ui.log(
            "commitcloud_updates",
            version=version,
            repo=reponame,
            workspace=workspace,
            oldheadcount=len(oldheads),
            newheadcount=len(newheads),
            oldbookmarkcount=len(oldbookmarks),
            newbookmarkcount=len(newbookmarks),
            oldremotebookmarkcount=len(oldremotebookmarks),
            newremotebookmarkcount=len(newremotebookmarks),
            client_correlator=client_correlator,
            client_entry_point=client_entry_point,
            **logopts,
        )

        # send request

        response = self.repo.edenapi.cloudupdatereferences(
            {
                "version": version,
                "reponame": reponame,
                "workspace": workspace,
                "removed_heads": oldheads,
                "new_heads": newheads,
                "removed_bookmarks": oldbookmarks,
                "updated_bookmarks": newbookmarks,
                "removed_remote_bookmarks": oldremotebookmarks,
                "updated_remote_bookmarks": newremotebookmarks,
                "new_snapshots": [],
                "removed_snapshots": [],
                "clientinfo": None,
            }
        )
        newversion = response["version"]

        self.ui.debug(
            "'update_references' accepted update, old version is %d, new version is %d\n"
            % (version, newversion),
            component="commitcloud",
        )

        return (
            True,
            self._makeemptyreferences(newversion),
        )

    def getsmartlog(self, reponame, workspace, repo, limit, flags=[]):
        self.fallback.getsmartlog(reponame, workspace, repo, limit, flags)

    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        self.fallback.getsmartlogbyversion(
            reponame, workspace, repo, date, version, limit, flags=[]
        )

    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        self.fallback.updatecheckoutlocations(
            reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
        )

    def getworkspaces(self, reponame, prefix):
        self.fallback(reponame, prefix)

    def getworkspace(self, reponame, workspacename):
        self.ui.debug("Calling 'cloudworkspace' on edenapi\n", component="commitcloud")
        stream = self.repo.edenapi.cloudworkspace(workspacename, reponame)
        return list(stream)

    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace"""
        self.fallback.updateworkspacearchive(reponame, workspace, archived)

    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""
        self.fallback.renameworkspace(reponame, workspace, new_workspace)

    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""
        self.fallback.shareworkspace(reponame, workspace)

    def rollbackworkspace(self, reponame, workspace, version):
        """Rollback the given workspace to a specific version"""
        self.fallback.rollbackworkspace(reponame, workspace, version)

    def cleanupworkspace(self, reponame, workspace):
        """Cleanup unnecessary remote bookmarks from the given workspace"""
        self.fallback.cleanupworkspace(reponame, workspace)

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
