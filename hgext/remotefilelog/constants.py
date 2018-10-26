# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import struct

from mercurial.i18n import _


REQUIREMENT = "remotefilelog"

FILENAMESTRUCT = "!H"
FILENAMESIZE = struct.calcsize(FILENAMESTRUCT)

NODESIZE = 20
PACKREQUESTCOUNTSTRUCT = "!I"

NODECOUNTSTRUCT = "!I"
NODECOUNTSIZE = struct.calcsize(NODECOUNTSTRUCT)

PATHCOUNTSTRUCT = "!I"
PATHCOUNTSIZE = struct.calcsize(PATHCOUNTSTRUCT)

FILEPACK_CATEGORY = ""
TREEPACK_CATEGORY = "manifests"

ALL_CATEGORIES = [FILEPACK_CATEGORY, TREEPACK_CATEGORY]

# revision metadata keys. must be a single character.
METAKEYFLAG = "f"  # revlog flag
METAKEYSIZE = "s"  # full rawtext size


def getunits(category):
    if category == FILEPACK_CATEGORY:
        return _("files")
    if category == TREEPACK_CATEGORY:
        return _("trees")


# Repack options passed to ``markledger``.
OPTION_LOOSEONLY = "looseonly"
OPTION_PACKSONLY = "packsonly"
