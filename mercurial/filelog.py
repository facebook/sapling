# filelog.py - file history class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import error, mdiff, revlog
import re, struct

_mdre = re.compile('\1\n')
def parsemeta(text):
    """return (metadatadict, keylist, metadatasize)"""
    # text can be buffer, so we can't use .startswith or .index
    if text[:2] != '\1\n':
        return None, None
    s = _mdre.search(text, 2).start()
    mtext = text[2:s]
    meta = {}
    for l in mtext.splitlines():
        k, v = l.split(": ", 1)
        meta[k] = v
    return meta, (s + 2)

def packmeta(meta, text):
    keys = sorted(meta.iterkeys())
    metatext = "".join("%s: %s\n" % (k, meta[k]) for k in keys)
    return "\1\n%s\1\n%s" % (metatext, text)

def _censoredtext(text):
    m, offs = parsemeta(text)
    return m and "censored" in m

class filelog(revlog.revlog):
    def __init__(self, opener, path):
        super(filelog, self).__init__(opener,
                        "/".join(("data", path + ".i")))

    def read(self, node):
        t = self.revision(node)
        if not t.startswith('\1\n'):
            return t
        s = t.index('\1\n', 2)
        return t[s + 2:]

    def add(self, text, meta, transaction, link, p1=None, p2=None):
        if meta or text.startswith('\1\n'):
            text = packmeta(meta, text)
        return self.addrevision(text, transaction, link, p1, p2)

    def renamed(self, node):
        if self.parents(node)[0] != revlog.nullid:
            return False
        t = self.revision(node)
        m = parsemeta(t)[0]
        if m and "copy" in m:
            return (m["copy"], revlog.bin(m["copyrev"]))
        return False

    def size(self, rev):
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
        if text.startswith('\1\n'):
            t = '\1\n\1\n' + text

        samehashes = not super(filelog, self).cmp(node, t)
        if samehashes:
            return False

        # censored files compare against the empty file
        if self.iscensored(self.rev(node)):
            return text != ''

        # renaming a file produces a different hash, even if the data
        # remains unchanged. Check if it's the case (slow):
        if self.renamed(node):
            t2 = self.read(node)
            return t2 != text

        return True

    def checkhash(self, text, p1, p2, node, rev=None):
        try:
            super(filelog, self).checkhash(text, p1, p2, node, rev=rev)
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
        return newlen >= addlen and delta[hlen:hlen + addlen] == add
