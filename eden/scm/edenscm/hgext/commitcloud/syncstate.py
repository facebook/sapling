# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import hashlib
import json
import time

from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import encodeutf8, ensurestr

from . import error as ccerror


NOTSET = object()


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
            + ".%s" % (hashlib.sha256(encodeutf8(workspacename)).hexdigest()[0:5])
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
                self.heads = [ensurestr(h) for h in data["heads"]]
                self.bookmarks = {
                    ensurestr(n): ensurestr(v) for n, v in data["bookmarks"].items()
                }
                self.remotebookmarks = {
                    ensurestr(n): ensurestr(v)
                    for n, v in data.get("remotebookmarks", {}).items()
                }
                self.snapshots = [ensurestr(s) for s in data.get("snapshots", [])]
                self.maxage = data.get("maxage", None)
                self.omittedheads = [ensurestr(h) for h in data.get("omittedheads", ())]
                self.omittedbookmarks = [
                    ensurestr(n) for n in data.get("omittedbookmarks", ())
                ]
                self.omittedremotebookmarks = [
                    ensurestr(n) for n in data.get("omittedremotebookmarks", ())
                ]
                self.lastupdatetime = data.get("lastupdatetime", None)
        else:
            self.version = 0
            self.heads = []
            self.bookmarks = {}
            self.remotebookmarks = {}
            self.snapshots = []
            self.maxage = None
            self.omittedheads = []
            self.omittedbookmarks = []
            self.omittedremotebookmarks = []
            self.lastupdatetime = None

    def update(
        self,
        tr,
        newversion=NOTSET,
        newheads=NOTSET,
        newbookmarks=NOTSET,
        newremotebookmarks=NOTSET,
        newsnapshots=NOTSET,
        newmaxage=NOTSET,
        newomittedheads=NOTSET,
        newomittedbookmarks=NOTSET,
        newomittedremotebookmarks=NOTSET,
    ):
        def update(value, orig):
            return orig if value is NOTSET else value

        version = update(newversion, self.version)
        heads = update(newheads, self.heads)
        bookmarks = update(newbookmarks, self.bookmarks)
        remotebookmarks = update(newremotebookmarks, self.remotebookmarks)
        snapshots = update(newsnapshots, self.snapshots)
        maxage = update(newmaxage, self.maxage)
        omittedheads = update(newomittedheads, self.omittedheads)
        omittedbookmarks = update(newomittedbookmarks, self.omittedbookmarks)
        omittedremotebookmarks = update(
            newomittedremotebookmarks, self.omittedremotebookmarks
        )
        data = {
            "version": version,
            "heads": heads,
            "bookmarks": bookmarks,
            "remotebookmarks": remotebookmarks,
            "snapshots": snapshots,
            "maxage": maxage,
            "omittedheads": omittedheads,
            "omittedbookmarks": omittedbookmarks,
            "omittedremotebookmarks": omittedremotebookmarks,
            "lastupdatetime": time.time(),
        }
        tr.addfilegenerator(
            self.filename,
            [self.filename],
            lambda f: f.write(encodeutf8(json.dumps(data))),
        )
        self.prevstate = (self.version, self.heads, self.bookmarks, self.snapshots)
        self.version = version
        self.heads = heads
        self.bookmarks = bookmarks
        self.remotebookmarks = remotebookmarks
        self.snapshots = snapshots
        self.omittedheads = omittedheads
        self.omittedbookmarks = omittedbookmarks
        self.omittedremotebookmarks = omittedremotebookmarks
        self.maxage = maxage
        self.repo.ui.log(
            "commitcloud_sync",
            "synced to workspace %s version %s: %d heads (%d omitted), %d bookmarks (%d omitted), %d remote bookmarks (%d omitted), %d snapshots\n",
            self.workspacename,
            version,
            len(heads),
            len(omittedheads),
            len(bookmarks),
            len(omittedbookmarks),
            len(remotebookmarks),
            len(omittedremotebookmarks),
            len(snapshots),
        )

    def oscillating(self, newheads, newbookmarks, newsnapshots):
        """detect oscillating workspaces

        Returns true if updating the cloud state to the new heads or bookmarks
        would be equivalent to updating back to the immediate previous
        version.
        """
        if self.prevstate is not None and self.lastupdatetime is not None:
            prevversion, prevheads, prevbookmarks, prevsnapshots = self.prevstate
            return (
                prevversion == self.version - 1
                and prevheads == newheads
                and prevbookmarks == newbookmarks
                and prevsnapshots == newsnapshots
                and self.lastupdatetime > time.time() - 60
            )
        return False
