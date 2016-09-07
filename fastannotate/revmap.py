# Copyright 2016-present Facebook. All Rights Reserved.
#
# revmap: trivial hg hash - linelog rev bidirectional map
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os
import struct

# the revmap file format is straightforward:
#
#    8 bytes: header
#   20 bytes: hg hash for linelog revision 1
#    1 byte : flag for linelog revision 1
#   20 bytes: hg hash for linelog revision 2
#    1 byte : flag for linelog revision 2
#   ....
#
# the implementation is kinda stupid: __init__ loads the whole revmap.
# no laziness. benchmark shows loading 10000 revisions is about 0.015
# seconds, which looks enough for our use-case. if this implementation
# becomes a bottleneck, we can change it to lazily read the file
# from the end.

class CorruptedFileError(Exception):
    pass

# whether the changeset is in the side branch. i.e. not in the linear main
# branch but only got referenced by lines in merge changesets.
sidebranchflag = 1

class revmap(object):
    """trivial hg bin hash - linelog rev bidirectional map

    also stores a flag (uint8) for each revision.
    """

    HEADER = b'REVMAP0\0'

    def __init__(self, path=None):
        """create or load the revmap, optionally associate to a file

        if path is None, the revmap is entirely in-memory. the caller is
        responsible for locking. concurrent writes to a same file is unsafe.
        the caller needs to make sure one file is associated to at most one
        revmap object at a time."""
        self.path = path
        self._rev2hsh = [None]
        self._rev2flag = [None]
        self._hsh2rev = {}
        if path:
            if os.path.exists(path):
                self._load()
            else:
                # write the header so "append" can do incremental updates
                self.flush()

    @property
    def maxrev(self):
        """return max linelog revision number"""
        return len(self._rev2hsh) - 1

    def append(self, hsh, flag=0, flush=False):
        """add a binary hg hash and return the mapped linelog revision.
        if flush is True, incrementally update the file.
        """
        assert hsh not in self._hsh2rev
        assert len(hsh) == 20
        idx = len(self._rev2hsh)
        self._rev2hsh.append(hsh)
        self._rev2flag.append(flag)
        self._hsh2rev[hsh] = idx
        if flush and self.path: # incremental update
            with open(self.path, 'a') as f:
                f.write(hsh)
                f.write(struct.pack('B', flag))
        return idx

    def rev2hsh(self, rev):
        """convert linelog revision to hg hash. return None if not found."""
        if rev > self.maxrev or rev < 0:
            return None
        return self._rev2hsh[rev]

    def rev2flag(self, rev):
        """get the flag (uint8) for a given linelog revision.
        return None if revision does not exist.
        """
        if rev > self.maxrev or rev < 0:
            return None
        return self._rev2flag[rev]

    def hsh2rev(self, hsh):
        """convert hg hash to linelog revision. return None if not found."""
        return self._hsh2rev.get(hsh)

    def clear(self, flush=False):
        """make the map empty. if flush is True, write to disk"""
        # rev 0 is reserved, real rev starts from 1
        self._rev2hsh = [None]
        self._rev2flag = [None]
        self._hsh2rev = {}
        if flush:
            self.flush()

    def flush(self):
        """write the state down to the file"""
        if not self.path:
            return
        with open(self.path, 'wb') as f:
            f.write(self.HEADER)
            for i, hsh in enumerate(self._rev2hsh):
                if i == 0:
                    continue
                f.write(hsh)
                f.write(struct.pack('B', self._rev2flag[i]))

    def _load(self):
        """load state from file"""
        if not self.path:
            return
        with open(self.path, 'rb') as f:
            if f.read(len(self.HEADER)) != self.HEADER:
                raise CorruptedFileError()
            self.clear(flush=False)
            while True:
                buf = f.read(21)
                if len(buf) == 0:
                    break
                elif len(buf) == 21: # 20-byte hash + 1-byte flag
                    hsh = buf[0:20]
                    flag = struct.unpack('B', buf[20])[0]
                    self._hsh2rev[hsh] = len(self._rev2hsh)
                    self._rev2hsh.append(hsh)
                    self._rev2flag.append(flag)
                else:
                    raise CorruptedFileError()

    def __contains__(self, f):
        """(fctx or node) -> bool.
        test if f is in the map and is not in a side branch.
        """
        if isinstance(f, str):
            hsh = f
        else:
            hsh = f.node()
        rev = self.hsh2rev(hsh)
        if rev is None:
            return False
        return (self.rev2flag(rev) & sidebranchflag) == 0
