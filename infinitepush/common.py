# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import json
import struct

from mercurial import extensions

def isremotebooksenabled(ui):
    return ('remotenames' in extensions._extensions and
            ui.configbool('remotenames', 'bookmarks', True))

def encodebookmarks(bookmarks):
    encoded = {}
    for bookmark, node in bookmarks.iteritems():
        encoded[bookmark] = node
    dumped = json.dumps(encoded)
    result = struct.pack('>i', len(dumped)) + dumped
    return result
