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

from mercurial import pathutil
from mercurial.i18n import _

_version = 4
_versionformat = ">I"

class state(object):
    def __init__(self, repo):
        self._opener = repo.opener
        self._ui = repo.ui
        self._rootdir = pathutil.normasprefix(repo.root)
        self._lastclock = None

        self.mode = self._ui.config('fsmonitor', 'mode', default='on')
        self.walk_on_invalidate = self._ui.configbool(
            'fsmonitor', 'walk_on_invalidate', False)
        self.timeout = float(self._ui.config(
            'fsmonitor', 'timeout', default='2'))

    def get(self):
        try:
            file = self._opener('fsmonitor.state', 'rb')
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            return None, None, None

        versionbytes = file.read(4)
        if len(versionbytes) < 4:
            self._ui.log(
                'fsmonitor', 'fsmonitor: state file only has %d bytes, '
                'nuking state\n' % len(versionbytes))
            self.invalidate()
            return None, None, None
        try:
            diskversion = struct.unpack(_versionformat, versionbytes)[0]
            if diskversion != _version:
                # different version, nuke state and start over
                self._ui.log(
                    'fsmonitor', 'fsmonitor: version switch from %d to '
                    '%d, nuking state\n' % (diskversion, _version))
                self.invalidate()
                return None, None, None

            state = file.read().split('\0')
            # state = hostname\0clock\0ignorehash\0 + list of files, each
            # followed by a \0
            diskhostname = state[0]
            hostname = socket.gethostname()
            if diskhostname != hostname:
                # file got moved to a different host
                self._ui.log('fsmonitor', 'fsmonitor: stored hostname "%s" '
                             'different from current "%s", nuking state\n' %
                             (diskhostname, hostname))
                self.invalidate()
                return None, None, None

            clock = state[1]
            ignorehash = state[2]
            # discard the value after the last \0
            notefiles = state[3:-1]

        finally:
            file.close()

        return clock, ignorehash, notefiles

    def set(self, clock, ignorehash, notefiles):
        if clock is None:
            self.invalidate()
            return

        try:
            file = self._opener('fsmonitor.state', 'wb')
        except (IOError, OSError):
            self._ui.warn(_("warning: unable to write out fsmonitor state\n"))
            return

        try:
            file.write(struct.pack(_versionformat, _version))
            file.write(socket.gethostname() + '\0')
            file.write(clock + '\0')
            file.write(ignorehash + '\0')
            if notefiles:
                file.write('\0'.join(notefiles))
                file.write('\0')
        finally:
            file.close()

    def invalidate(self):
        try:
            os.unlink(os.path.join(self._rootdir, '.hg', 'fsmonitor.state'))
        except OSError as inst:
            if inst.errno != errno.ENOENT:
                raise

    def setlastclock(self, clock):
        self._lastclock = clock

    def getlastclock(self):
        return self._lastclock
