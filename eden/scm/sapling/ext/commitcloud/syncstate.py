# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import json
import time

NOTSET = object()


class SyncState:
    """
    Stores the local record of what state was stored in the cloud at the
    last sync.
    """

    # Version 2: a single JSON file "cloudsyncstate" for all workspaces.
    # format: {workspacename: state}. More compatible with metalog which works
    # best with fixed filenames.
    v2filename = "cloudsyncstate"

    @classmethod
    def erasestate(cls, repo, workspacename):
        # update v2 states
        with repo.lock(), repo.transaction("cloudstate"):
            states = cls.loadv2states(repo.svfs)
            if workspacename in states:
                del states[workspacename]
                cls.savev2states(repo.svfs, states)

    @classmethod
    def movestate(cls, repo, workspacename, new_workspacename):
        # update v2 states
        with repo.lock(), repo.transaction("cloudstate"):
            states = cls.loadv2states(repo.svfs)
            if workspacename in states:
                states[new_workspacename] = states[workspacename]
                cls.savev2states(repo.svfs, states)

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
        self.repo = repo
        self.prevstate = None

        # Try v2 state first.
        states = self.loadv2states(repo.svfs)
        data = states.get(workspacename)
        if data is not None:
            self.version = data["version"]
            self.heads = data["heads"]
            self.bookmarks = data["bookmarks"]
            self.remotebookmarks = data.get("remotebookmarks", {})
            self.maxage = data.get("maxage", None)
            self.omittedheads = data.get("omittedheads", [])
            self.omittedbookmarks = data.get("omittedbookmarks", [])
            self.omittedremotebookmarks = data.get("omittedremotebookmarks", [])
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
        svfs = tr._vfsmap[""]
        states = self.loadv2states(svfs)
        states[self.workspacename] = data
        tr.addfilegenerator(
            self.v2filename,
            [self.v2filename],
            lambda f, states=states: f.write(json.dumps(states).encode()),
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
