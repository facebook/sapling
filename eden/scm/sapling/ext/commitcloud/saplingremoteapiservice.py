# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import time

from bindings import clientinfo as clientinfomod

# Standard Library

from sapling import error
from sapling.pycompat import ensurestr

from . import baseservice


class SaplingRemoteAPIService(baseservice.BaseService):
    """Remote commit-cloud service implemented using Sapling Remote API."""

    def __init__(self, ui, repo, fallback):
        self.ui = ui
        if repo is None:
            raise error.Abort(
                "Tried to start Sapling Remote API service with no repo object"
            )
        self.repo = repo
        self.repo.edenapi.capabilities()  # Check Sapling Remote API is reachable
        self.fallback = fallback

    def check(self):
        return True

    def getreferences(
        self,
        reponame,
        workspace,
        baseversion,
        clientinfo=None,
    ):
        self.ui.debug(
            "sending 'get_references' request on Sapling Remote API\n",
            component="commitcloud",
        )
        response = self.repo.edenapi.cloudreferences(
            {
                "workspace": workspace,
                "reponame": reponame,
                "version": baseversion,
                "client_info": clientinfo,
            }
        )
        version = self._getversionfromdata(response)

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
        if "data" in response:
            response = response["data"]["Ok"]

        refs = self._castreferences(response)
        return self._makereferences(refs)

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
        self.ui.debug(
            "sending 'update_references' request on Sapling Remote API\n",
            component="commitcloud",
        )
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
                "removed_remote_bookmarks": self._makeremotebookmarks(
                    oldremotebookmarks
                ),
                "updated_remote_bookmarks": self._makeremotebookmarks(
                    newremotebookmarks
                ),
                "new_snapshots": [],
                "removed_snapshots": [],
                "clientinfo": clientinfo,
            }
        )
        newversion = self._getversionfromdata(response)

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
        self.ui.debug(
            "sending 'get_smartlog' request on SaplingRemoteAPI\n",
            component="commitcloud",
        )

        data = {
            "reponame": reponame,
            "workspace": workspace,
            "flags": self._map_legacy_flags(flags),
        }
        response = self.repo.edenapi.cloudsmartlog(data)

        smartlog = self._getdatafromresponse(response)
        if limit != 0:
            cutoff = int(time.time()) - limit
            smartlog["nodes"] = list(
                filter(lambda x: x["date"] >= cutoff, smartlog["nodes"])
            )
        self.ui.debug(
            "'get_smartlog' returns %d entries\n" % len(smartlog["nodes"]),
            component="commitcloud",
        )

        smartlog["nodes"] = self._decode_smartlog_nodes(smartlog["nodes"])
        try:
            return self._makesmartloginfo(smartlog)
        except Exception as e:
            raise error.UnexpectedError(self.ui, e)

    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        self.ui.debug(
            "sending 'get_old_smartlog' request on SaplingRemoteAPI\n",
            component="commitcloud",
        )
        if date:
            data = {
                "reponame": reponame,
                "workspace": workspace,
                "filter": {"Timestamp": date[0]},
                "flags": self._map_legacy_flags(flags),
            }
        else:
            data = {
                "reponame": reponame,
                "workspace": workspace,
                "filter": {"Version": int(version)},
                "flags": self._map_legacy_flags(flags),
            }

        response = self.repo.edenapi.cloudsmartlogbyversion(data)

        smartlog = self._getdatafromresponse(response)
        if limit != 0:
            cutoff = smartlog["timestamp"] - limit
            smartlog["nodes"] = list(
                filter(lambda x: x["date"] >= cutoff, smartlog["nodes"])
            )

        smartlog["nodes"] = self._decode_smartlog_nodes(smartlog["nodes"])
        self.ui.debug(
            "'get_smartlog' returns %d entries\n" % len(smartlog["nodes"]),
            component="commitcloud",
        )

        try:
            return self._makesmartloginfo(smartlog)
        except Exception as e:
            raise error.UnexpectedError(self.ui, e)

    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        return self.fallback.updatecheckoutlocations(
            reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
        )

    def getworkspaces(self, reponame, prefix):
        """Fetch Commit Cloud workspaces for the given prefix"""
        self.ui.debug(
            "sending 'get_workspaces' request on Sapling Remote API\n",
            component="commitcloud",
        )

        response = self.repo.edenapi.cloudworkspaces(prefix, reponame)
        # Put everything into "workspaces" key, so it's easier to parse in the client
        workspaces = {"workspaces": self._getdatafromresponse(response)}
        return self._makeworkspacesinfo(workspaces)

    def getworkspace(self, reponame, workspacename):
        self.ui.debug(
            "sending 'get_workspace' request on Sapling Remote API\n",
            component="commitcloud",
        )
        response = self.repo.edenapi.cloudworkspace(workspacename, reponame)

        if "data" in response:
            if "Ok" in response["data"]:
                response = response["data"]["Ok"]
            else:
                raise error.Abort(response["data"]["Err"]["message"])

        return baseservice.WorkspaceInfo(
            name=ensurestr(response["name"]),
            archived=bool(response["archived"]),
            version=int(response["version"]),
        )

    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace"""
        self.ui.debug(
            "sending 'update_workspace_archive' request on SaplingRemoteAPI\n",
            component="commitcloud",
        )

        data = {"reponame": reponame, "workspace": workspace, "archived": archived}
        self.repo.edenapi.cloudupdatearchive(data)

    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""
        self.ui.debug(
            "sending 'rename_workspace' request on SaplingRemoteAPI\n",
            component="commitcloud",
        )

        data = {
            "reponame": reponame,
            "workspace": workspace,
            "new_workspace": new_workspace,
        }
        self._getdatafromresponse(self.repo.edenapi.cloudrenameworkspace(data))

    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""
        self.ui.debug(
            "sending 'share_workspace' request through Sapling Remote API\n",
            component="commitcloud",
        )
        data = {
            "reponame": reponame,
            "workspace": workspace,
        }
        response = self.repo.edenapi.cloudshareworkspace(data)
        return self._getdatafromresponse(response)

    def rollbackworkspace(self, reponame, workspace, version):
        """Rollback the given workspace to a specific version"""
        return self.fallback.rollbackworkspace(reponame, workspace, version)

    def cleanupworkspace(self, reponame, workspace):
        """Cleanup unnecessary remote bookmarks from the given workspace"""
        return self.fallback.cleanupworkspace(reponame, workspace)

    def gethistoricalversions(self, reponame, workspace):
        self.ui.debug(
            "sending 'get_historical_versions' request on SaplingRemoteAPI\n",
            component="commitcloud",
        )

        data = {"reponame": reponame, "workspace": workspace}

        response = self.repo.edenapi.cloudhistoricalversions(data)
        versions = self._getdatafromresponse(response)["versions"]

        self.ui.debug(
            "'get_historical_versions' returns %d entries\n" % len(versions),
            component="commitcloud",
        )

        return versions

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
        refs["head_dates"] = heads_dates
        refs["snapshots"] = snapshots
        refs["heads"] = heads
        return refs

    def _getversionfromdata(self, response):
        if "data" in response:
            if "Ok" in response["data"]:
                version = response["data"]["Ok"]["version"]
            else:
                raise error.Abort(response["data"]["Err"]["message"])
        else:
            version = response["version"]
        return version

    def _getdatafromresponse(self, response):
        if "data" in response:
            if "Ok" in response["data"]:
                return response["data"]["Ok"]
            else:
                raise error.Abort(response["data"]["Err"]["message"])

        raise error.Abort("No data revceived from server")

    def _map_legacy_flags(self, strings):
        mapping = {
            "ADD_REMOTE_BOOKMARKS": "AddRemoteBookmarks",
            "ADD_ALL_BOOKMARKS": "AddAllBookmarks",
            "SKIP_PUBLIC_COMMITS_METADATA": "SkipPublicCommitsMetadata",
        }
        return [mapping[s] for s in strings]

    def _decode_smartlog_nodes(self, nodes):
        for nodeinfo in nodes:
            nodeinfo["node"] = nodeinfo["node"].hex()
            nodeinfo["parents"] = list(map(lambda x: x.hex(), nodeinfo["parents"]))
            if nodeinfo["remote_bookmarks"] is not None:
                for bookmark in nodeinfo["remote_bookmarks"]:
                    bookmark["node"] = bookmark["node"].hex()
        return nodes
