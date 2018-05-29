# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import json
import os

from mercurial import error

from . import baseservice


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
            return {"version": 0, "heads": [], "bookmarks": {}, "obsmarkers": {}}

    def _save(self, data):
        filename = os.path.join(self.path, "commitcloudservicedb")
        with open(filename, "w") as f:
            json.dump(data, f)

    """ filter the obmarkers since the baseversion,
        this includes (baseversion, data[version]] obsmarkers
    """

    def _filteredobsmarkers(self, data, baseversion):
        versions = range(baseversion, data["version"])
        data["new_obsmarkers_data"] = sum(
            (data["obsmarkers"][str(n + 1)] for n in versions), []
        )
        del data["obsmarkers"]
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
            return baseservice.References(version, None, None, None)
        else:
            self._ui.debug(
                "commitcloud local service: "
                "get_references for versions from %s to %s\n" % (baseversion, version)
            )

            return self._makereferences(self._filteredobsmarkers(data, baseversion))

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
    ):
        data = self._load()
        if version != data["version"]:
            return False, self._makereferences(self._filteredobsmarkers(data, version))

        newversion = data["version"] + 1
        data["version"] = newversion
        data["heads"] = newheads
        data["bookmarks"] = newbookmarks
        data["obsmarkers"][str(newversion)] = self._encodedmarkers(newobsmarkers)
        self._ui.debug(
            "commitcloud local service: "
            "update_references to %s (%s heads, %s bookmarks)\n"
            % (newversion, len(data["heads"]), len(data["bookmarks"]))
        )
        self._save(data)
        return True, baseservice.References(newversion, None, None, None)
