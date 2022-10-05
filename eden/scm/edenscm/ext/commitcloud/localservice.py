# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Standard Library
import os

from edenscm import error, json, pycompat
from edenscm.i18n import _

from . import baseservice, error as ccerror, workspace as ccworkspace


class _LocalService(baseservice.BaseService):
    """Local commit-cloud service implemented using files on disk.

    There is no locking, so this is suitable only for use in unit tests.
    """

    def __init__(self, ui):
        self._ui = ui
        self.path = ui.config("commitcloud", "servicelocation")
        if not self.path or not os.path.isdir(self.path):
            msg = "Invalid commitcloud.servicelocation: %s" % self.path
            raise error.Abort(msg)

    def _workspacefilename(self, prefix, workspacename):
        if workspacename == ccworkspace.defaultworkspace(self._ui):
            return prefix
        else:
            return prefix + "".join(x for x in workspacename if x.isalnum())

    def _load(self, workspace):
        filename = os.path.join(
            self.path, self._workspacefilename("commitcloudservicedb", workspace)
        )
        if os.path.exists(filename):
            with open(filename, "rb") as f:
                data = json.load(f)
                return data
        else:
            return {
                "version": 0,
                "heads": [],
                "bookmarks": {},
                "remotebookmarks": {},
            }

    def _save(self, data, workspace):
        filename = os.path.join(
            self.path, self._workspacefilename("commitcloudservicedb", workspace)
        )
        with open(filename, "wb") as f:
            f.write(pycompat.encodeutf8(json.dumps(data)))

    def _injectheaddates(self, data, workspace):
        """inject a head_dates field into the data"""
        data["head_dates"] = {}
        heads = set(data["heads"])
        filename = os.path.join(
            self.path, self._workspacefilename("nodedata", workspace)
        )
        if os.path.exists(filename):
            with open(filename, "rb") as f:
                nodes = json.load(f)
                for node in nodes:
                    if node["node"] in heads:
                        data["head_dates"][node["node"]] = node["date"][0]
        return data

    def requiresauthentication(self):
        return False

    def check(self):
        return True

    def getreferences(
        self,
        reponame,
        workspace,
        baseversion,
        clientinfo=None,
    ):
        data = self._load(workspace)
        version = data["version"]
        if version == baseversion:
            self._ui.debug(
                "commitcloud local service: "
                "get_references for current version %s\n" % version
            )
            return self._makeemptyreferences(version)
        else:
            if baseversion > version:
                raise error.Abort(
                    _(
                        "'get_references' failed, the workspace has been renamed or client has an invalid state"
                    )
                )
            self._ui.debug(
                "commitcloud local service: "
                "get_references for versions from %s to %s\n" % (baseversion, version)
            )
            data = self._injectheaddates(data, workspace)
            return self._makereferences(data)

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
        data = self._load(workspace)
        if version != data["version"]:
            return False, self._makereferences(data)

        oldheads = set(oldheads or [])
        newheads = newheads or []
        oldbookmarks = set(oldbookmarks or [])
        newbookmarks = newbookmarks or {}
        oldremotebookmarks = set(oldremotebookmarks or [])
        newremotebookmarks = newremotebookmarks or {}

        heads = [head for head in data["heads"] if head not in oldheads]
        heads.extend(newheads)
        bookmarks = {
            name: node
            for name, node in pycompat.iteritems(data["bookmarks"])
            if name not in oldbookmarks
        }
        bookmarks.update(newbookmarks)
        remotebookmarks = {
            name: node
            for name, node in pycompat.iteritems(
                self._decoderemotebookmarks(data.get("remote_bookmarks", []))
            )
            if name not in oldremotebookmarks
        }
        remotebookmarks.update(newremotebookmarks)

        newversion = data["version"] + 1
        data["version"] = newversion
        data["heads"] = heads
        data["bookmarks"] = bookmarks
        data["remote_bookmarks"] = self._makeremotebookmarks(remotebookmarks)
        self._ui.debug(
            "commitcloud local service: "
            "update_references to %s (%s heads, %s bookmarks, %s remote bookmarks)\n"
            % (
                newversion,
                len(data["heads"]),
                len(data["bookmarks"]),
                len(data["remote_bookmarks"]),
            )
        )
        self._save(data, workspace)
        allworkspaces = [
            winfo
            for winfo in self.getworkspaces(reponame, None)
            if winfo.name != workspace
        ]
        allworkspaces.append(
            baseservice.WorkspaceInfo(
                name=workspace, archived=False, version=newversion
            )
        )
        self._saveworkspaces(allworkspaces)
        return (
            True,
            self._makeemptyreferences(newversion),
        )

    def getsmartlog(self, reponame, workspace, repo, limit, flags=[]):
        filename = os.path.join(
            self.path, self._workspacefilename("usersmartlogdata", workspace)
        )
        if not os.path.exists(filename):
            return None
        try:
            with open(filename, "rb") as f:
                data = json.load(f)
                return self._makesmartloginfo(data["smartlog"])
        except Exception as e:
            raise ccerror.UnexpectedError(self._ui, e)

    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        filename = os.path.join(
            self.path, self._workspacefilename("usersmartlogbyversiondata", workspace)
        )
        if not os.path.exists(filename):
            return None
        try:
            with open(filename, "rb") as f:
                data = json.load(f)
                data = data["smartlog"]
                data["version"] = 42
                data["timestamp"] = 1562690787
                return self._makesmartloginfo(data)
        except Exception as e:
            raise ccerror.UnexpectedError(self._ui, e)

    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        data = {
            "repo_name": reponame,
            "workspace": workspace,
            "hostname": hostname,
            "commit": commit,
            "checkout_path": checkoutpath,
            "shared_path": sharedpath,
            "unixname": unixname,
        }
        filename = os.path.join(
            self.path, self._workspacefilename("checkoutlocations", workspace)
        )
        with open(filename, "w+") as f:
            json.dump(data, f)

    def getworkspaces(self, reponame, prefix):
        if prefix is None:
            prefix = ""
        filename = os.path.join(self.path, "workspacesdata")
        if not os.path.exists(filename):
            return []
        try:
            with open(filename, "rb") as f:
                data = json.load(f)
                return [
                    winfo
                    for winfo in self._makeworkspacesinfo(data["workspaces_data"])
                    if winfo.name.startswith(prefix)
                ]
        except Exception as e:
            raise ccerror.UnexpectedError(self._ui, e)

    def _saveworkspaces(self, data):
        filename = os.path.join(self.path, "workspacesdata")
        with open(filename, "wb") as f:
            f.write(
                pycompat.encodeutf8(
                    json.dumps(
                        {
                            "workspaces_data": {
                                "workspaces": [
                                    {
                                        "name": item.name,
                                        "archived": item.archived,
                                        "version": item.version,
                                    }
                                    for item in data
                                ]
                            }
                        }
                    )
                )
            )

    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace"""
        allworkspaces = self.getworkspaces(reponame, None)
        found = next(
            (winfo for winfo in allworkspaces if winfo.name == workspace), None
        )
        if found:
            allworkspaces = [
                winfo for winfo in allworkspaces if winfo.name != workspace
            ]
            allworkspaces.append(
                baseservice.WorkspaceInfo(
                    name=found.name, archived=archived, version=found.version
                )
            )
            self._saveworkspaces(allworkspaces)
        else:
            raise error.Abort(_("unknown workspace: %s") % workspace)

    def renameworkspace(self, reponame, workspace, new_workspace):
        """Rename the given workspace"""
        allworkspaces = self.getworkspaces(reponame, None)
        if next(
            (winfo for winfo in allworkspaces if winfo.name == new_workspace), None
        ):
            raise error.Abort(_("workspace: '%s' already exists") % new_workspace)

        found = next(
            (winfo for winfo in allworkspaces if winfo.name == workspace), None
        )
        if found:
            # update the list of workspaces
            allworkspaces = [
                winfo for winfo in allworkspaces if winfo.name != workspace
            ]
            allworkspaces.append(
                baseservice.WorkspaceInfo(
                    name=new_workspace, archived=found.archived, version=found.version
                )
            )
            # rename bunch of files:
            for name in [
                "usersmartlogdata",
                "checkoutlocations",
                "usersmartlogbyversiondata",
                "nodedata",
                "commitcloudservicedb",
            ]:
                src = os.path.join(self.path, self._workspacefilename(name, workspace))
                dst = os.path.join(
                    self.path, self._workspacefilename(name, new_workspace)
                )
                if os.path.exists(src):
                    os.rename(src, dst)
            self._saveworkspaces(allworkspaces)
        else:
            raise error.Abort(_("unknown workspace: %s") % workspace)

    def shareworkspace(self, reponame, workspace):
        """Enable sharing for the given workspace"""
        raise NotImplementedError  # Since auth is disabled in tests


# Make sure that the LocalService is a singleton
LocalService = baseservice.SingletonDecorator(_LocalService)
