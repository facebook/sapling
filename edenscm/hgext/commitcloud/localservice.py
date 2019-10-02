# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import json
import os

from edenscm.mercurial import error

from . import baseservice, error as ccerror


class LocalService(baseservice.BaseService):
    """Local commit-cloud service implemented using files on disk.

    There is no locking, so this is suitable only for use in unit tests.
    """

    def __init__(self, ui):
        self._ui = ui
        self.path = ui.config("commitcloud", "servicelocation")
        if not self.path or not os.path.isdir(self.path):
            msg = "Invalid commitcloud.servicelocation: %s" % self.path
            raise error.Abort(msg)

    def _load(self):
        filename = os.path.join(self.path, "commitcloudservicedb")
        if os.path.exists(filename):
            with open(filename) as f:
                data = json.load(f)
                return data
        else:
            return {
                "version": 0,
                "heads": [],
                "bookmarks": {},
                "obsmarkers": {},
                "remotebookmarks": {},
            }

    def _save(self, data):
        filename = os.path.join(self.path, "commitcloudservicedb")
        with open(filename, "w") as f:
            json.dump(data, f)

    def _filteredobsmarkers(self, data, baseversion):
        """filter the obmarkers since the baseversion

        This includes (baseversion, data[version]] obsmarkers
        """
        versions = range(baseversion, data["version"])
        data["new_obsmarkers_data"] = sum(
            (data["obsmarkers"][str(n + 1)] for n in versions), []
        )
        del data["obsmarkers"]
        return data

    def _injectheaddates(self, data):
        """inject a head_dates field into the data"""
        data["head_dates"] = {}
        heads = set(data["heads"])
        filename = os.path.join(self.path, "nodedata")
        if os.path.exists(filename):
            with open(filename) as f:
                nodes = json.load(f)
                for node in nodes:
                    if node["node"] in heads:
                        data["head_dates"][node["node"]] = node["date"][0]
        return data

    def requiresauthentication(self):
        return False

    def check(self):
        return True

    def getreferences(self, reponame, workspace, baseversion):
        data = self._load()
        version = data["version"]
        if version == baseversion:
            self._ui.debug(
                "commitcloud local service: "
                "get_references for current version %s\n" % version
            )
            return baseservice.References(version, None, None, None, None, None, None)
        else:
            self._ui.debug(
                "commitcloud local service: "
                "get_references for versions from %s to %s\n" % (baseversion, version)
            )
            data = self._filteredobsmarkers(data, baseversion)
            data = self._injectheaddates(data)
            return self._makereferences(data)

    def updatereferences(
        self,
        reponame,
        workspace,
        version,
        oldheads,
        newheads,
        oldbookmarks,
        newbookmarks,
        newobsmarkers,
        oldremotebookmarks=[],
        newremotebookmarks={},
        oldsnapshots=[],
        newsnapshots=[],
    ):
        data = self._load()
        if version != data["version"]:
            return False, self._makereferences(self._filteredobsmarkers(data, version))

        newversion = data["version"] + 1
        data["version"] = newversion
        data["heads"] = newheads
        data["bookmarks"] = newbookmarks
        data["obsmarkers"][str(newversion)] = self._encodedmarkers(newobsmarkers)
        if not data.get("remote_bookmarks", {}) or newremotebookmarks:
            data["remote_bookmarks"] = self._makeremotebookmarks(newremotebookmarks)
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
        data["snapshots"] = newsnapshots
        self._save(data)
        return (
            True,
            baseservice.References(newversion, None, None, None, None, None, None),
        )

    def getsmartlog(self, reponame, workspace, repo, limit):
        filename = os.path.join(self.path, "usersmartlogdata")
        if not os.path.exists(filename):
            nodes = {}
        else:
            with open(filename) as f:
                data = json.load(f)
                nodes = self._makenodes(data["smartlog"])
        try:
            return self._makefakedag(nodes, repo)
        except Exception as e:
            raise ccerror.UnexpectedError(self._ui, e)

    def getsmartlogbyversion(self, reponame, workspace, repo, date, version, limit):
        filename = os.path.join(self.path, "usersmartlogbyversiondata")
        if not os.path.exists(filename):
            nodes = {}
        else:
            with open(filename) as f:
                data = json.load(f)
                nodes = self._makenodes(data["smartlog"])
        try:
            return self._makefakedag(nodes, repo), 42, 1562690787
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
        filename = os.path.join(self.path, "checkoutlocations")
        with open(filename, "w+") as f:
            json.dump(data, f)
