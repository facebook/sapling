# Copyright (c) Meta Platforms, Inc. and affiliates.
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

    # Version 2: a single JSON file "cloudsyncstate" for all workspaces.
    # format: {workspacename: state}. More compatible with metalog which works
    # best with fixed filenames.
    v2filename = "cloudsyncstate"

    # Version 1: a JSON file "commitcloudstate.<workspace>.<hash>" per
    # workspace.
    v1prefix = "commitcloudstate."

    @classmethod
    def _v1filename(cls, workspacename):
        """filename for workspace, only for compatibility"""
        # make a unique valid filename
        return (
            cls.v1prefix
            + "".join(x for x in workspacename if x.isalnum())
            + ".%s" % (hashlib.sha256(encodeutf8(workspacename)).hexdigest()[0:5])
        )

    @classmethod
    def erasestate(cls, repo, workspacename):
        # update v2 states
        with repo.lock(), repo.transaction("cloudstate"):
            states = cls.loadv2states(repo.svfs)
            if workspacename in states:
                del states[workspacename]
                cls.savev2states(repo.svfs, states)
        # update v1 states
        filename = cls._v1filename(workspacename)
        # clean up the current state in force recover mode
        repo.svfs.tryunlink(filename)

    @classmethod
    def movestate(cls, repo, workspacename, new_workspacename):
        # update v2 states
        with repo.lock(), repo.transaction("cloudstate"):
            states = cls.loadv2states(repo.svfs)
            if workspacename in states:
                states[new_workspacename] = states[workspacename]
                cls.savev2states(repo.svfs, states)
        # update v1 states
        src = cls._v1filename(workspacename)
        dst = cls._v1filename(new_workspacename)
        repo.svfs.rename(src, dst)

    @classmethod
    def loadv2states(cls, svfs):
        """load states for all workspaces, return {workspacename: state}"""
        content = svfs.tryread(cls.v2filename) or "{}"
        try:
            return json.loads(content)
        except json.JSONDecodeError:
            return {}

    @classmethod
    def savev2states(cls, svfs, states):
        data = json.dumps(states)
        with svfs.open(cls.v2filename, "wb", atomictemp=True) as f:
            f.write(data.encode())

    def __init__(self, repo, workspacename):
        self.workspacename = workspacename
        self.v1filename = self._v1filename(workspacename)
        self.repo = repo
        self.prevstate = None

        # Try v2 state first.
        states = self.loadv2states(repo.svfs)
        data = states.get(workspacename)

        # If v2 state is missing, try load from v1 state.
        if data is None and repo.svfs.exists(self.v1filename):
            # Migra v1 state to v2 so v2 state gets used going forward.
            with repo.lock(), repo.transaction("cloudstate"):
                # Reload since v2 states might have changed.
                states = self.loadv2states(repo.svfs)
                with repo.svfs.open(self.v1filename, "r") as f:
                    try:
                        data = json.load(f)
                    except Exception:
                        raise ccerror.InvalidWorkspaceDataError(
                            repo.ui, _("failed to parse %s") % self.v1filename
                        )
                    states[workspacename] = data
                self.savev2states(repo.svfs, states)

        if data is not None:
            self.version = data["version"]
            self.heads = [ensurestr(h) for h in data["heads"]]
            self.bookmarks = {
                ensurestr(n): ensurestr(v) for n, v in data["bookmarks"].items()
            }
            self.remotebookmarks = {
                ensurestr(n): ensurestr(v)
                for n, v in data.get("remotebookmarks", {}).items()
            }
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
            "maxage": maxage,
            "omittedheads": omittedheads,
            "omittedbookmarks": omittedbookmarks,
            "omittedremotebookmarks": omittedremotebookmarks,
            "lastupdatetime": time.time(),
        }
        tr.addfilegenerator(
            self.v1filename,
            [self.v1filename],
            lambda f, data=data: f.write(encodeutf8(json.dumps(data))),
        )
        svfs = tr._vfsmap[""]
        states = self.loadv2states(svfs)
        states[self.workspacename] = data
        tr.addfilegenerator(
            self.v2filename,
            [self.v2filename],
            lambda f, states=states: f.write(encodeutf8(json.dumps(states))),
        )
        self.prevstate = (self.version, self.heads, self.bookmarks)
        self.version = version
        self.heads = heads
        self.bookmarks = bookmarks
        self.remotebookmarks = remotebookmarks
        self.omittedheads = omittedheads
        self.omittedbookmarks = omittedbookmarks
        self.omittedremotebookmarks = omittedremotebookmarks
        self.maxage = maxage
        self.repo.ui.log(
            "commitcloud_sync",
            "synced to workspace %s version %s: %d heads (%d omitted), %d bookmarks (%d omitted), %d remote bookmarks (%d omitted)\n",
            self.workspacename,
            version,
            len(heads),
            len(omittedheads),
            len(bookmarks),
            len(omittedbookmarks),
            len(remotebookmarks),
            len(omittedremotebookmarks),
        )
