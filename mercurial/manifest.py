# manifest.py - manifest revision class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2, incorporated herein by reference.

from i18n import _
import mdiff, parsers, error, revlog
import array, struct

class manifestdict(dict):
    def __init__(self, mapping=None, flags=None):
        if mapping is None: mapping = {}
        if flags is None: flags = {}
        dict.__init__(self, mapping)
        self._flags = flags
    def flags(self, f):
        return self._flags.get(f, "")
    def set(self, f, flags):
        self._flags[f] = flags
    def copy(self):
        return manifestdict(dict.copy(self), dict.copy(self._flags))

class manifest(revlog.revlog):
    def __init__(self, opener):
        self.mapcache = None
        self.listcache = None
        revlog.revlog.__init__(self, opener, "00manifest.i")

    def parse(self, lines):
        mfdict = manifestdict()
        parsers.parse_manifest(mfdict, mfdict._flags, lines)
        return mfdict

    def readdelta(self, node):
        r = self.rev(node)
        return self.parse(mdiff.patchtext(self.revdiff(r - 1, r)))

    def read(self, node):
        if node == revlog.nullid:
            return manifestdict() # don't upset local cache
        if self.mapcache and self.mapcache[0] == node:
            return self.mapcache[1]
        text = self.revision(node)
        self.listcache = array.array('c', text)
        mapping = self.parse(text)
        self.mapcache = (node, mapping)
        return mapping

    def _search(self, m, s, lo=0, hi=None):
        '''return a tuple (start, end) that says where to find s within m.

        If the string is found m[start:end] are the line containing
        that string.  If start == end the string was not found and
        they indicate the proper sorted insertion point.  This was
        taken from bisect_left, and modified to find line start/end as
        it goes along.

        m should be a buffer or a string
        s is a string'''
        def advance(i, c):
            while i < lenm and m[i] != c:
                i += 1
            return i
        if not s:
            return (lo, lo)
        lenm = len(m)
        if not hi:
            hi = lenm
        while lo < hi:
            mid = (lo + hi) // 2
            start = mid
            while start > 0 and m[start-1] != '\n':
                start -= 1
            end = advance(start, '\0')
            if m[start:end] < s:
                # we know that after the null there are 40 bytes of sha1
                # this translates to the bisect lo = mid + 1
                lo = advance(end + 40, '\n') + 1
            else:
                # this translates to the bisect hi = mid
                hi = start
        end = advance(lo, '\0')
        found = m[lo:end]
        if cmp(s, found) == 0:
            # we know that after the null there are 40 bytes of sha1
            end = advance(end + 40, '\n')
            return (lo, end+1)
        else:
            return (lo, lo)

    def find(self, node, f):
        '''look up entry for a single file efficiently.
        return (node, flags) pair if found, (None, None) if not.'''
        if self.mapcache and node == self.mapcache[0]:
            return self.mapcache[1].get(f), self.mapcache[1].flags(f)
        text = self.revision(node)
        start, end = self._search(text, f)
        if start == end:
            return None, None
        l = text[start:end]
        f, n = l.split('\0')
        return revlog.bin(n[:40]), n[40:-1]

    def add(self, map, transaction, link, p1=None, p2=None,
            changed=None):
        # apply the changes collected during the bisect loop to our addlist
        # return a delta suitable for addrevision
        def addlistdelta(addlist, x):
            # start from the bottom up
            # so changes to the offsets don't mess things up.
            i = len(x)
            while i > 0:
                i -= 1
                start = x[i][0]
                end = x[i][1]
                if x[i][2]:
                    addlist[start:end] = array.array('c', x[i][2])
                else:
                    del addlist[start:end]
            return "".join([struct.pack(">lll", d[0], d[1], len(d[2])) + d[2]
                            for d in x ])

        def checkforbidden(l):
            for f in l:
                if '\n' in f or '\r' in f:
                    raise error.RevlogError(
                        _("'\\n' and '\\r' disallowed in filenames: %r") % f)

        # if we're using the listcache, make sure it is valid and
        # parented by the same node we're diffing against
        if not (changed and self.listcache and p1 and self.mapcache[0] == p1):
            files = sorted(map)
            checkforbidden(files)

            # if this is changed to support newlines in filenames,
            # be sure to check the templates/ dir again (especially *-raw.tmpl)
            hex, flags = revlog.hex, map.flags
            text = ["%s\000%s%s\n" % (f, hex(map[f]), flags(f))
                    for f in files]
            self.listcache = array.array('c', "".join(text))
            cachedelta = None
        else:
            addlist = self.listcache

            checkforbidden(changed[0])
            # combine the changed lists into one list for sorting
            work = [[x, 0] for x in changed[0]]
            work[len(work):] = [[x, 1] for x in changed[1]]
            work.sort()

            delta = []
            dstart = None
            dend = None
            dline = [""]
            start = 0
            # zero copy representation of addlist as a buffer
            addbuf = buffer(addlist)

            # start with a readonly loop that finds the offset of
            # each line and creates the deltas
            for w in work:
                f = w[0]
                # bs will either be the index of the item or the insert point
                start, end = self._search(addbuf, f, start)
                if w[1] == 0:
                    l = "%s\000%s%s\n" % (f, revlog.hex(map[f]), map.flags(f))
                else:
                    l = ""
                if start == end and w[1] == 1:
                    # item we want to delete was not found, error out
                    raise AssertionError(
                            _("failed to remove %s from manifest") % f)
                if dstart != None and dstart <= start and dend >= start:
                    if dend < end:
                        dend = end
                    if l:
                        dline.append(l)
                else:
                    if dstart != None:
                        delta.append([dstart, dend, "".join(dline)])
                    dstart = start
                    dend = end
                    dline = [l]

            if dstart != None:
                delta.append([dstart, dend, "".join(dline)])
            # apply the delta to the addlist, and get a delta for addrevision
            cachedelta = addlistdelta(addlist, delta)

            # the delta is only valid if we've been processing the tip revision
            if self.mapcache[0] != self.tip():
                cachedelta = None
            self.listcache = addlist

        n = self.addrevision(buffer(self.listcache), transaction, link,
                             p1, p2, cachedelta)
        self.mapcache = (n, map)

        return n
