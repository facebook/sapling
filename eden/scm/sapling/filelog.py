# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# filelog.py - file history class for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import re
import struct
from typing import Dict, Pattern

import bindings

from . import eagerepo, error, git, mdiff, revlog, util
from .node import bin


_mdre: Pattern[bytes] = re.compile(b"\1\n")


def parsemeta(text):
    """return (metadatadict, metadatasize)"""
    # text can be buffer, so we can't use .startswith or .index
    if bytes(text[:2]) != b"\1\n":
        return None, None
    s = _mdre.search(text, 2).start()
    mtext = text[2:s].decode()
    meta = {}
    for l in mtext.splitlines():
        k, v = l.split(": ", 1)
        meta[k] = v
    return meta, (s + 2)


def packmeta(meta: "Dict[str, str]", text: bytes) -> bytes:
    keys = sorted(meta)
    metatext = "".join("%s: %s\n" % (k, meta[k]) for k in keys).encode()
    return b"".join((b"\1\n", metatext, b"\1\n", text))


def _censoredtext(text):
    m, offs = parsemeta(text)
    return m and b"censored" in m


class filelog(revlog.revlog):
    def __init__(self, opener, path):
        super(filelog, self).__init__(opener, "/".join(("data", path + ".i")))

    def read(self, node):
        t = self.revision(node)
        if not t.startswith(b"\1\n"):
            return t
        s = t.index(b"\1\n", 2)
        return t[s + 2 :]

    def add(self, text, meta, transaction, link, p1=None, p2=None):
        if meta or text.startswith(b"\1\n"):
            text = packmeta(meta, text)
        return self.addrevision(text, transaction, link, p1, p2)

    def renamed(self, node):
        if self.parents(node)[0] != revlog.nullid:
            return False
        t = self.revision(node)
        m = parsemeta(t)[0]
        if m and "copy" in m:
            return (m["copy"], bin(m["copyrev"]))
        return False

    def size(self, rev: int) -> int:
        """return the size of a given revision"""

        # for revisions with renames, we have to go the slow way
        node = self.node(rev)
        if self.renamed(node):
            return len(self.read(node))
        if self.iscensored(rev):
            return 0

        # XXX if self.read(node).startswith("\1\n"), this returns (size+4)
        return super(filelog, self).size(rev)

    def cmp(self, node, text):
        """compare text with a given file revision

        returns True if text is different than what is stored.
        """

        t = text
        if text.startswith(b"\1\n"):
            t = b"\1\n\1\n" + text

        samehashes = not super(filelog, self).cmp(node, t)
        if samehashes:
            return False

        # censored files compare against the empty file
        if self.iscensored(self.rev(node)):
            return text != b""

        # renaming a file produces a different hash, even if the data
        # remains unchanged. Check if it's the case (slow):
        if self.renamed(node):
            t2 = self.read(node)
            return t2 != text

        return True

    def checkhash(self, text, node, p1=None, p2=None, rev=None):
        try:
            super(filelog, self).checkhash(text, node, p1=p1, p2=p2, rev=rev)
        except error.RevlogError:
            if _censoredtext(text):
                raise error.CensoredNodeError(self.indexfile, node, text)
            raise

    def iscensored(self, rev):
        """Check if a file revision is censored."""
        return self.flags(rev) & revlog.REVIDX_ISCENSORED

    def _peek_iscensored(self, baserev, delta, flush):
        """Quickly check if a delta produces a censored revision."""
        # Fragile heuristic: unless new file meta keys are added alphabetically
        # preceding "censored", all censored revisions are prefixed by
        # "\1\ncensored:". A delta producing such a censored revision must be a
        # full-replacement delta, so we inspect the first and only patch in the
        # delta for this prefix.
        hlen = struct.calcsize(">lll")
        if len(delta) <= hlen:
            return False

        oldlen = self.rawsize(baserev)
        newlen = len(delta) - hlen
        if delta[:hlen] != mdiff.replacediffheader(oldlen, newlen):
            return False

        add = "\1\ncensored:"
        addlen = len(add)
        return newlen >= addlen and delta[hlen : hlen + addlen] == add


class fileslog:
    """Top level object representing all the file storage.

    Eventually filelog content access should go through this, but for now it's
    just used to handle remotefilelog writes.
    """

    def __init__(self, repo):
        self.ui = repo.ui
        self.repo = repo
        if git.isgitstore(repo):
            self.filestore = git.openstore(repo)
        elif eagerepo.iseagerepo(repo) or repo.storage_format() == "revlog":
            self.filestore = repo._rsrepo.eagerstore()

    def commitpending(self):
        """Used in alternative filelog implementations to commit pending
        additions."""
        if eagerepo.iseagerepo(self.repo):
            self.filestore.flush()

    def abortpending(self):
        """Used in alternative filelog implementations to throw out pending
        additions."""

    def abstracted_file_store(self):
        """get the abstracted storemodel::FileStore"""
        return bindings.storemodel.FileStore.from_store(self.filestore)
