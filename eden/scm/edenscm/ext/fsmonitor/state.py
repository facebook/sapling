# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# state.py - fsmonitor persistent state

from __future__ import absolute_import

import errno
import os
import socket
import struct

from edenscm import pathutil
from edenscm.i18n import _


_version = 4
_versionformat = ">I"


class state:
    def __init__(self, repo):
        self._vfs = repo.localvfs
        self._ui = repo.ui
        self._rootdir = pathutil.normasprefix(repo.root)
        self._lastclock = None
        self._lastisfresh = False
        # File count reported by watchman
        self._lastchangedfilecount = 0
        # Non-normal file count stored in dirstate
        self._lastnonnormalcount = 0

        self.mode = self._ui.config("fsmonitor", "mode")
        self.walk_on_invalidate = self._ui.configbool("fsmonitor", "walk_on_invalidate")
        self.timeout = float(self._ui.config("fsmonitor", "timeout"))
        self._repo = repo
        self._droplist = []
        self._ignorelist = []

    def get(self):
        """return clock, notefiles"""
        clock = self._repo.dirstate.getclock()
        # note files are already included in nonnormalset, so they will be
        # processed anyway, do not return a separate notefiles.
        notefiles = []
        return clock, notefiles

    def setdroplist(self, droplist):
        """set a list of files to be dropped from dirstate upon 'set'.

        This is used to clean up deleted untracked files from treestate, which
        tracks untracked files.
        """
        self._droplist = droplist

    def setignorelist(self, ignorelist):
        """set a list of files that are found ignored when processing notefiles"""
        if self._ui.configbool("fsmonitor", "track-ignore-files"):
            self._ignorelist = ignorelist

    def set(self, clock, notefiles):
        ds = self._repo.dirstate
        dmap = ds._map
        changed = bool(self._droplist) or bool(self._lastisfresh)
        if self._lastchangedfilecount >= self._ui.configint(
            "fsmonitor", "watchman-changed-file-threshold"
        ):
            changed = True
        for path in self._droplist:
            dmap.deletefile(path, None)
        self._droplist = []
        for path in notefiles:
            changed |= ds.needcheck(path)
        for path in self._ignorelist:
            changed |= ds.needcheck(path)
        self._ignorelist = []
        # Avoid updating dirstate frequently if nothing changed.
        # But do update dirstate if the clock is reset to None, or is
        # moving away from None.
        if not clock or changed or not ds.getclock():
            ds.setclock(clock)
        return

    def invalidate(self, reason=None):
        try:
            os.unlink(
                os.path.join(
                    self._rootdir, self._ui.identity.dotdir(), "fsmonitor.state"
                )
            )
        except OSError as inst:
            if inst.errno != errno.ENOENT:
                raise

    def setlastclock(self, clock):
        self._lastclock = clock

    def setlastisfresh(self, isfresh):
        self._lastisfresh = isfresh

    def setwatchmanchangedfilecount(self, filecount):
        self._lastchangedfilecount = filecount

    def setlastnonnormalfilecount(self, count):
        self._lastnonnormalcount = count

    def getlastclock(self):
        return self._lastclock
