# manifest.py - manifest revision class for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import sys, struct
from revlog import *
from i18n import gettext as _
from demandload import *
demandload(globals(), "bisect")

class manifest(revlog):
    def __init__(self, opener):
        self.mapcache = None
        self.listcache = None
        self.addlist = None
        revlog.__init__(self, opener, "00manifest.i", "00manifest.d")

    def read(self, node):
        if node == nullid: return {} # don't upset local cache
        if self.mapcache and self.mapcache[0] == node:
            return self.mapcache[1]
        text = self.revision(node)
        map = {}
        flag = {}
        self.listcache = (text, text.splitlines(1))
        for l in self.listcache[1]:
            (f, n) = l.split('\0')
            map[f] = bin(n[:40])
            flag[f] = (n[40:-1] == "x")
        self.mapcache = (node, map, flag)
        return map

    def readflags(self, node):
        if node == nullid: return {} # don't upset local cache
        if not self.mapcache or self.mapcache[0] != node:
            self.read(node)
        return self.mapcache[2]

    def add(self, map, flags, transaction, link, p1=None, p2=None,
            changed=None):
        # directly generate the mdiff delta from the data collected during
        # the bisect loop below
        def gendelta(delta):
            i = 0
            result = []
            while i < len(delta):
                start = delta[i][2]
                end = delta[i][3]
                l = delta[i][4]
                if l == None:
                    l = ""
                while i < len(delta) - 1 and start <= delta[i+1][2] \
                          and end >= delta[i+1][2]:
                    if delta[i+1][3] > end:
                        end = delta[i+1][3]
                    if delta[i+1][4]:
                        l += delta[i+1][4]
                    i += 1
                result.append(struct.pack(">lll", start, end, len(l)) +  l)
                i += 1
            return result

        # apply the changes collected during the bisect loop to our addlist
        def addlistdelta(addlist, delta):
            # apply the deltas to the addlist.  start from the bottom up
            # so changes to the offsets don't mess things up.
            i = len(delta)
            while i > 0:
                i -= 1
                start = delta[i][0]
                end = delta[i][1]
                if delta[i][4]:
                    addlist[start:end] = [delta[i][4]]
                else:
                    del addlist[start:end]
            return addlist

        # calculate the byte offset of the start of each line in the
        # manifest
        def calcoffsets(addlist):
            offsets = [0] * (len(addlist) + 1)
            offset = 0
            i = 0
            while i < len(addlist):
                offsets[i] = offset
                offset += len(addlist[i])
                i += 1
            offsets[i] = offset
            return offsets

        # if we're using the listcache, make sure it is valid and
        # parented by the same node we're diffing against
        if not changed or not self.listcache or not p1 or \
               self.mapcache[0] != p1:
            files = map.keys()
            files.sort()

            self.addlist = ["%s\000%s%s\n" %
                            (f, hex(map[f]), flags[f] and "x" or '')
                            for f in files]
            cachedelta = None
        else:
            addlist = self.listcache[1]

            # find the starting offset for each line in the add list
            offsets = calcoffsets(addlist)

            # combine the changed lists into one list for sorting
            work = [[x, 0] for x in changed[0]]
            work[len(work):] = [[x, 1] for x in changed[1]]
            work.sort()

            delta = []
            bs = 0

            for w in work:
                f = w[0]
                # bs will either be the index of the item or the insert point
                bs = bisect.bisect(addlist, f, bs)
                if bs < len(addlist):
                    fn = addlist[bs][:addlist[bs].index('\0')]
                else:
                    fn = None
                if w[1] == 0:
                    l = "%s\000%s%s\n" % (f, hex(map[f]),
                                          flags[f] and "x" or '')
                else:
                    l = None
                start = bs
                if fn != f:
                    # item not found, insert a new one
                    end = bs
                    if w[1] == 1:
                        raise AssertionError(
                            _("failed to remove %s from manifest\n") % f)
                else:
                    # item is found, replace/delete the existing line
                    end = bs + 1
                delta.append([start, end, offsets[start], offsets[end], l])

            self.addlist = addlistdelta(addlist, delta)
            if self.mapcache[0] == self.tip():
                cachedelta = "".join(gendelta(delta))
            else:
                cachedelta = None

        text = "".join(self.addlist)
        if cachedelta and mdiff.patch(self.listcache[0], cachedelta) != text:
            raise AssertionError(_("manifest delta failure\n"))
        n = self.addrevision(text, transaction, link, p1, p2, cachedelta)
        self.mapcache = (n, map, flags)
        self.listcache = (text, self.addlist)
        self.addlist = None

        return n
