# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib
import json
import time

from edenscm.mercurial.i18n import _

from . import error as ccerror


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
        self.workspacename = workspacename
        self.filename = self._filename(workspacename)
        self.repo = repo
        self.prevstate = None
        if repo.svfs.exists(self.filename):
            with repo.svfs.open(self.filename, "r") as f:
                try:
                    data = json.load(f)
                except Exception:
                    raise ccerror.InvalidWorkspaceDataError(
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
                self.lastupdatetime = data.get("lastupdatetime", None)
        else:
            self.version = 0
            self.heads = []
            self.bookmarks = {}
            self.omittedheads = []
            self.omittedbookmarks = []
            self.maxage = None
            self.lastupdatetime = None

    def update(
        self,
        newversion,
        newheads,
        newbookmarks,
        newomittedheads,
        newomittedbookmarks,
        newmaxage,
    ):
        data = {
            "version": newversion,
            "heads": newheads,
            "bookmarks": newbookmarks,
            "omittedheads": newomittedheads,
            "omittedbookmarks": newomittedbookmarks,
            "maxage": newmaxage,
            "lastupdatetime": time.time(),
        }
        with self.repo.svfs.open(self.filename, "w", atomictemp=True) as f:
            json.dump(data, f)
        self.prevstate = (self.version, self.heads, self.bookmarks)
        self.version = newversion
        self.heads = newheads
        self.bookmarks = newbookmarks
        self.omittedheads = newomittedheads
        self.omittedbookmarks = newomittedbookmarks
        self.maxage = newmaxage
        self.repo.ui.log(
            "commitcloud_sync",
            "synced to workspace %s version %s: %d heads (%d omitted), %d bookmarks (%d omitted)\n",
            self.workspacename,
            newversion,
            len(newheads),
            len(newomittedheads),
            len(newbookmarks),
            len(newomittedbookmarks),
        )

    def oscillating(self, newheads, newbookmarks):
        """detect oscillating workspaces

        Returns true if updating the cloud state to the new heads or bookmarks
        would be equivalent to updating back to the immediate previous
        version.
        """
        if self.prevstate is not None and self.lastupdatetime is not None:
            prevversion, prevheads, prevbookmarks = self.prevstate
            return (
                prevversion == self.version - 1
                and prevheads == newheads
                and prevbookmarks == newbookmarks
                and self.lastupdatetime > time.time() - 60
            )
        return False
