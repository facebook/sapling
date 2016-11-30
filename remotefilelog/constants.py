from mercurial.i18n import _

import struct

REQUIREMENT = "remotefilelog"

FILENAMESTRUCT = '!H'
FILENAMESIZE = struct.calcsize(FILENAMESTRUCT)

NODESIZE = 20
PACKREQUESTCOUNTSTRUCT = '!I'

FILEPACK_CATEGORY=""
TREEPACK_CATEGORY="manifests"

def getunits(category):
    if category == FILEPACK_CATEGORY:
        return _("files")
    if category == TREEPACK_CATEGORY:
        return _("trees")
