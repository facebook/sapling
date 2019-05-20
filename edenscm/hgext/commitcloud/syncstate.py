# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib
import json
import time

from edenscm.mercurial.i18n import _

from . import commitcloudcommon


class SyncState(object):
    """
    Stores the local record of what state was stored in the cloud at the
    last sync.
    """

    prefix = "commitcloudstate."

    @classmethod
    def _filename(cls, workspacename):
        # make a unique valid filename
        return (
            cls.prefix
            + "".join(x for x in workspacename if x.isalnum())
            + ".%s" % (hashlib.sha256(workspacename).hexdigest()[0:5])
        )

    @classmethod
    def erasestate(cls, repo, workspacename):
        filename = cls._filename(workspacename)
        # clean up the current state in force recover mode
        repo.svfs.tryunlink(filename)

    def __init__(self, repo, workspacename):
        self.filename = self._filename(workspacename)
        self.repo = repo
        if repo.svfs.exists(self.filename):
            with repo.svfs.open(self.filename, "r") as f:
                try:
                    data = json.load(f)
                except Exception:
                    raise commitcloudcommon.InvalidWorkspaceDataError(
                        repo.ui, _("failed to parse %s") % self.filename
                    )

                self.version = data["version"]
                self.heads = [h.encode() for h in data["heads"]]
                self.bookmarks = {
                    n.encode("utf-8"): v.encode() for n, v in data["bookmarks"].items()
                }
                self.omittedheads = [h.encode() for h in data.get("omittedheads", ())]
                self.omittedbookmarks = [
                    n.encode("utf-8") for n in data.get("omittedbookmarks", ())
                ]
                self.maxage = data.get("maxage", None)
                self.remotepath = data.get("remotepath", None)
                self.lastupdatetime = data.get("lastupdatetime", None)
        else:
            self.version = 0
            self.heads = []
            self.bookmarks = {}
            self.omittedheads = []
            self.omittedbookmarks = []
            self.maxage = None
            self.remotepath = None
            self.lastupdatetime = None

    def update(
        self,
        newversion,
        newheads,
        newbookmarks,
        newomittedheads,
        newomittedbookmarks,
        newmaxage,
        remotepath,
    ):
        data = {
            "version": newversion,
            "heads": newheads,
            "bookmarks": newbookmarks,
            "omittedheads": newomittedheads,
            "omittedbookmarks": newomittedbookmarks,
            "maxage": newmaxage,
            "remotepath": remotepath,
            "lastupdatetime": time.time(),
        }
        with self.repo.svfs.open(self.filename, "w", atomictemp=True) as f:
            json.dump(data, f)
        self.version = newversion
        self.heads = newheads
        self.bookmarks = newbookmarks
        self.omittedheads = newomittedheads
        self.omittedbookmarks = newomittedbookmarks
        self.maxage = newmaxage

    def updateremotepath(self, remotepath):
        self.update(
            self.version,
            self.heads,
            self.bookmarks,
            self.omittedheads,
            self.omittedbookmarks,
            self.maxage,
            remotepath,
        )
