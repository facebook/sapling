# state.py - fsmonitor persistent state
#
# Copyright 2013-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import os
import socket
import struct

from mercurial import pathutil, util
from mercurial.i18n import _


_version = 4
_versionformat = ">I"


class state(object):
    def __init__(self, repo):
        self._vfs = repo.vfs
        self._ui = repo.ui
        self._rootdir = pathutil.normasprefix(repo.root)
        self._lastclock = None

        self.mode = self._ui.config("fsmonitor", "mode")
        self.walk_on_invalidate = self._ui.configbool("fsmonitor", "walk_on_invalidate")
        self.timeout = float(self._ui.config("fsmonitor", "timeout"))
        self._repo = repo
        self._usetreestate = "treestate" in repo.requirements
        self._droplist = []

    def get(self):
        """return clock, ignorehash, notefiles"""
        if self._usetreestate:
            clock = self._repo.dirstate.getclock()
            # XXX: ignorehash is already broken, so return None
            ignorehash = None
            # note files are already included in nonnormalset, so they will be
            # processed anyway, do not return a separate notefiles.
            notefiles = []
            return clock, ignorehash, notefiles
        try:
            file = self._vfs("fsmonitor.state", "rb")
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            return None, None, None

        versionbytes = file.read(4)
        if len(versionbytes) < 4:
            self._ui.log(
                "fsmonitor",
                "fsmonitor: state file only has %d bytes, "
                "nuking state\n" % len(versionbytes),
            )
            self.invalidate(reason="state_file_truncated")
            return None, None, None
        try:
            diskversion = struct.unpack(_versionformat, versionbytes)[0]
            if diskversion != _version:
                # different version, nuke state and start over
                self._ui.log(
                    "fsmonitor",
                    "fsmonitor: version switch from %d to "
                    "%d, nuking state\n" % (diskversion, _version),
                )
                self.invalidate(reason="state_file_wrong_version")
                return None, None, None

            state = file.read().split("\0")
            # state = hostname\0clock\0ignorehash\0 + list of files, each
            # followed by a \0
            if len(state) < 3:
                self._ui.log(
                    "fsmonitor",
                    "fsmonitor: state file truncated (expected "
                    "3 chunks, found %d), nuking state\n",
                    len(state),
                )
                self.invalidate(reason="state_file_truncated")
                return None, None, None
            diskhostname = state[0]
            hostname = socket.gethostname()
            if diskhostname != hostname:
                # file got moved to a different host
                self._ui.log(
                    "fsmonitor",
                    'fsmonitor: stored hostname "%s" '
                    'different from current "%s", nuking state\n'
                    % (diskhostname, hostname),
                )
                self.invalidate(reason="hostname_mismatch")
                return None, None, None

            clock = state[1]
            ignorehash = state[2]
            # discard the value after the last \0
            notefiles = state[3:-1]

        finally:
            file.close()

        if "fsmonitor_details" in getattr(self._ui, "track", ()):
            self._ui.log(
                "fsmonitor_details", "clock, notefiles = %r, %r" % (clock, notefiles)
            )

        return clock, ignorehash, notefiles

    def setdroplist(self, droplist):
        """set a list of files to be dropped from dirstate upon 'set'.

        This is used to clean up deleted untracked files from treestate, which
        tracks untracked files.
        """
        self._droplist = droplist

    def set(self, clock, ignorehash, notefiles):
        if "fsmonitor_details" in getattr(self._ui, "track", ()):
            self._ui.log(
                "fsmonitor_details",
                "set clock, notefiles = %r, %r" % (clock, notefiles),
            )

        if self._usetreestate:
            ds = self._repo.dirstate
            dmap = ds._map
            changed = bool(self._droplist)
            for path in self._droplist:
                dmap.dropfile(path, None, real=True)
            self._droplist = []
            for path in notefiles:
                changed |= ds.needcheck(path)
            # Avoid updating dirstate frequently if nothing changed.
            # But do update dirstate if the clock is reset to None, or is
            # moving away from None.
            if not clock or changed or not ds.getclock():
                ds.setclock(clock)
            return

        if clock is None:
            self.invalidate(reason="no_clock")
            return

        # The code runs with a wlock taken, and dirstate has passed its
        # identity check. So we can update both dirstate and fsmonitor state.
        # See _poststatusfixup in context.py

        try:
            file = self._vfs("fsmonitor.state", "wb", atomictemp=True, checkambig=True)
        except (IOError, OSError):
            self._ui.warn(_("warning: unable to write out fsmonitor state\n"))
            return

        with file:
            file.write(struct.pack(_versionformat, _version))
            file.write(socket.gethostname() + "\0")
            file.write(clock + "\0")
            file.write(ignorehash + "\0")
            if notefiles:
                file.write("\0".join(notefiles))
                file.write("\0")

    def invalidate(self, reason=None):
        if reason:
            self._ui.log("command_info", watchman_invalidate_reason=reason)
        try:
            os.unlink(os.path.join(self._rootdir, ".hg", "fsmonitor.state"))
        except OSError as inst:
            if inst.errno != errno.ENOENT:
                raise
        if "fsmonitor_details" in getattr(self._ui, "track", ()):
            self._ui.log("fsmonitor_details", "fsmonitor state invalidated")

    def setlastclock(self, clock):
        if "fsmonitor_details" in getattr(self._ui, "track", ()):
            self._ui.log("fsmonitor_details", "setlastclock: %r" % clock)
        self._lastclock = clock

    def getlastclock(self):
        if "fsmonitor_details" in getattr(self._ui, "track", ()):
            self._ui.log("fsmonitor_details", "getlastclock: %r" % self._lastclock)
        return self._lastclock
