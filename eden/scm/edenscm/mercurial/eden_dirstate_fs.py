# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Eden implementation for the dirstate filesystem class."""

from . import filesystem, perftrace, pycompat, util
from .EdenThriftClient import ScmFileStatus
from .pycompat import decodeutf8


class eden_filesystem(filesystem.physicalfilesystem):
    def pendingchanges(self, match=None, listignored=False):
        if match is None:
            match = util.always

        with perftrace.trace("Get EdenFS Status"):
            perftrace.traceflag("status")
            edenstatus = self.dirstate.eden_client.getStatus(
                self.dirstate.p1(), list_ignored=listignored
            )

        MODIFIED = ScmFileStatus.MODIFIED
        REMOVED = ScmFileStatus.REMOVED
        ADDED = ScmFileStatus.ADDED
        IGNORED = ScmFileStatus.IGNORED

        for path, code in pycompat.iteritems(edenstatus):
            path = decodeutf8(path)
            if not match(path):
                continue

            if code == MODIFIED or code == ADDED:
                yield (path, True)
            elif code == REMOVED:
                yield (path, False)
            elif code == IGNORED and listignored:
                yield (path, True)
            else:
                raise RuntimeError(
                    "unexpected status code '%s' for '%s'" % (code, path)
                )
