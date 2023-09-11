# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Eden implementation for the dirstate filesystem class."""

from typing import Callable, Iterable, Optional, Tuple

from . import filesystem, perftrace, pycompat, util
from .i18n import _
from .pycompat import decodeutf8


class eden_filesystem(filesystem.physicalfilesystem):
    def pendingchanges(
        self, match: "Optional[Callable[[str], bool]]" = None, listignored: bool = False
    ) -> "Iterable[Tuple[str, bool]]":
        if match is None:
            match = util.always

        with perftrace.trace("Get EdenFS Status"):
            perftrace.traceflag("status")
            edenstatus = self.dirstate.eden_client.getStatus(
                self.dirstate.p1(), list_ignored=listignored
            )

        MODIFIED = "M"
        REMOVED = "R"
        ADDED = "A"
        IGNORED = "I"

        for path, code in pycompat.iteritems(edenstatus):
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
