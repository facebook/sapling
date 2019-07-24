# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import struct

from edenscm.mercurial.i18n import _


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

# Tombstone string returned as content for blacklisted files
BLACKLISTED_CONTENT = "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n"
# Message shown to the user when file is blacklisted
BLACKLISTED_MESSAGE = "This version of the file is blacklisted and you are not allowed to access it. Update or rebase to a newer commit.\n"


def getunits(category):
    if category == FILEPACK_CATEGORY:
        return _("files")
    if category == TREEPACK_CATEGORY:
        return _("trees")


# Repack options passed to ``markledger``.
OPTION_LOOSEONLY = "looseonly"
OPTION_PACKSONLY = "packsonly"
