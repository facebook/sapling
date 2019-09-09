# -*- coding: utf-8 -*-

# snapshotlist.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from edenscm.mercurial import error, node


# Supported file format version.
# Version 1 is:
#  * A single line containing "v1"
#  * A list of node hashes for each snapshot, one per line.
FORMAT_VERSION = "v1"


class snapshotlist(object):
    """list of local snapshots
    """

    def __init__(self, repo):
        self.vfs = repo.svfs
        try:
            lines = self.vfs("snapshotlist").readlines()
            if not lines or lines[0].strip() != FORMAT_VERSION:
                raise error.Abort("invalid snapshots file format")
            self.snapshots = {node.bin(snapshot.strip()) for snapshot in lines[1:]}
        except IOError as err:
            if err.errno != errno.ENOENT:
                raise
            self.snapshots = set()

    def _write(self, fp):
        fp.write("%s\n" % FORMAT_VERSION)
        for s in sorted(self.snapshots):
            fp.write("%s\n" % (node.hex(s),))

    def add(self, newnodes, tr):
        newnodes = self.snapshots.union(newnodes)
        if self.snapshots != newnodes:
            self.snapshots = newnodes
            tr.addfilegenerator("snapshots", ("snapshotlist",), self._write)
