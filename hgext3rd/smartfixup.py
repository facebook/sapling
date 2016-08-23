# smartfixup.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""apply working directory changes to changesets

The smartfixup extension provides a command to use annotate information to
amend modified chunks into the corresponding non-public changesets.

::

    [smartfixup]
    # only check 50 recent non-public changesets at most
    maxstacksize = 50
    # whether to add noise to new commits to avoid obsolescence cycle
    addnoise = 1

    [color]
    smartfixup.node = blue bold
    smartfixup.path = bold
"""

from __future__ import absolute_import

from collections import defaultdict
import linelog

from mercurial import (
    cmdutil,
    commands,
    context,
    crecord,
    error,
    mdiff,
    node,
    obsolete,
    patch,
    phases,
    repair,
    scmutil,
    util,
)
from mercurial.i18n import _

testedwith = 'internal'

cmdtable = {}
command = cmdutil.command(cmdtable)

class nullui(object):
    """blank ui object doing nothing"""
    debugflag = False
    verbose = False
    quiet = True

    def __getitem__(name):
        def nullfunc(*args, **kwds):
            return
        return nullfunc

class emptyfilecontext(object):
    """minimal filecontext representing an empty file"""
    def data(self):
        return ''

    def node(self):
        return node.nullid

def uniq(lst):
    """list -> list. remove duplicated items without changing the order"""
    seen = set()
    result = []
    for x in lst:
        if x not in seen:
            seen.add(x)
            result.append(x)
    return result

def getdraftstack(headctx, limit=None):
    """(ctx, int?) -> [ctx]. get a linear stack of non-public changesets.

    changesets are sorted in topo order, oldest first.
    return at most limit items, if limit is a positive number.

    merges are considered as non-draft as well. i.e. every commit
    returned has and only has 1 parent.
    """
    ctx = headctx
    result = []
    while ctx.phase() != phases.public:
        if limit and len(result) >= limit:
            break
        parents = ctx.parents()
        if len(parents) != 1:
            break
        result.append(ctx)
        ctx = parents[0]
    result.reverse()
    return result

class overlaystore(object):
    """read-only, hybrid store based on a dict and ctx.
    memworkingcopy: {path: content}, overrides file contents.
    """
    def __init__(self, basectx, memworkingcopy):
        self.basectx = basectx
        self.memworkingcopy = memworkingcopy

    def getfile(self, path):
        """comply with mercurial.patch.filestore.getfile"""
        fctx = self.basectx[path]
        if path in self.memworkingcopy:
            content = self.memworkingcopy[path]
        else:
            content = fctx.data()
        mode = (fctx.islink(), fctx.isbinary())
        renamed = fctx.renamed() # False or (path, node)
        return content, mode, (renamed and renamed[0])

def overlaycontext(memworkingcopy, ctx, parents=None, extra=None):
    """({path: content}, ctx, (p1node, p2node)?, {}?) -> memctx
    memworkingcopy overrides file contents.
    """
    # parents must contain 2 items: (node1, node2)
    if parents is None:
        parents = ctx.repo().changelog.parents(ctx.node())
    if extra is None:
        extra = ctx.extra()
    date = ctx.date()
    desc = ctx.description()
    user = ctx.user()
    files = set(ctx.files()).union(memworkingcopy.iterkeys())
    store = overlaystore(ctx, memworkingcopy)
    return context.makememctx(ctx.repo(), parents, desc, user, date, None,
                              files, store, extra=extra)

class filefixupstate(object):
    """state needed to apply fixups to a single file

    internally, it keeps file contents of several revisions and a linelog.

    the linelog uses odd revision numbers for original contents (fctxs passed
    to __init__), and even revision numbers for fixups, like:

        linelog rev 1: self.fctxs[0] (from an immutable "public" changeset)
        linelog rev 2: fixups made to self.fctxs[0]
        linelog rev 3: self.fctxs[1] (a child of fctxs[0])
        linelog rev 4: fixups made to self.fctxs[1]
        ...

    a typical use is like:

        1. call diffwith, to calculate self.fixups
        2. (optionally), present self.fixups to the user, or change it
        3. call apply, to apply changes
        4. read results from "finalcontents", or call getfinalcontent
    """

    def __init__(self, fctxs, ui=None):
        """([fctx], ui or None) -> None

        fctxs should be linear, and sorted by topo order - oldest first.
        fctxs[0] will be considered as "immutable" and will not be changed.
        """
        self.fctxs = fctxs
        self.ui = ui or nullui()

        # following fields are built from fctxs. they exist for perf reason
        self.contents = [f.data() for f in fctxs]
        self.contentlines = map(mdiff.splitnewlines, self.contents)
        self.linelog = self._buildlinelog()
        if self.ui.debugflag:
            assert self._checkoutlinelog() == self.contents

        # following fields will be filled later
        self.chunkstats = [0, 0] # [adopted, total : int]
        self.targetlines = [] # [str]
        self.fixups = [] # [(linelog rev, a1, a2, b1, b2)]
        self.finalcontents = [] # [str]

    def diffwith(self, destfctx):
        """calculate fixups needed by examining the differences between
        self.fctxs[-1] and targetfctx, chunk by chunk.

        targetfctx is the target state we move towards. we may or may not be
        able to get there because not all modified chunks can be amended into
        a non-public fctx unambiguously.

        call this only once, before apply().

        update self.fixups, self.chunkstats, and self.targetlines.
        """
        a = self.contents[-1]
        alines = self.contentlines[-1]
        b = targetfctx.data()
        blines = mdiff.splitnewlines(b)
        self.targetlines = blines

        self.linelog.annotate(self.linelog.maxrev)
        annotated = self.linelog.annotateresult # [(linelog rev, linenum)]
        assert len(annotated) == len(alines)
        # add a dummy end line to make insertion at the end easier
        if annotated:
            dummyendline = (annotated[-1][0], annotated[-1][1] + 1)
            annotated.append(dummyendline)

        # analyse diff blocks
        for chunk in self._alldiffchunks(a, b, alines, blines):
            newfixups = self._analysediffchunk(chunk, annotated)
            self.chunkstats[0] += bool(newfixups) # 1 or 0
            self.chunkstats[1] += 1
            self.fixups += newfixups

    def apply(self):
        """apply self.fixups. update self.linelog, self.finalcontents.

        call this only once, before getfinalcontent(), after diffwith().
        """
        # the following is unnecessary, as it's done by "diffwith":
        #   self.linelog.annotate(self.linelog.maxrev)
        for rev, a1, a2, b1, b2 in reversed(self.fixups):
            blines = self.targetlines[b1:b2]
            if self.ui.debugflag:
                idx = (max(rev - 1, 0)) // 2
                self.ui.write(_('%s: chunk %d:%d -> %d lines\n')
                              % (node.short(self.fctxs[idx].node()),
                                 a1, a2, len(blines)))
            self.linelog.replacelines(rev, a1, a2, b1, b2)
        self.finalcontents = self._checkoutlinelog()

    def getfinalcontent(self, fctx):
        """(fctx) -> str. get modified file content for a given filecontext"""
        idx = self.fctxs.index(fctx)
        return self.finalcontents[idx]

    def _analysediffchunk(self, chunk, annotated):
        """analyse a different chunk and return new fixups found

        return [] if no lines from the chunk can be safely applied.

        the chunk (or lines) cannot be safely applied, if, for example:
          - the modified (deleted) lines belong to a public changeset
            (self.fctxs[0])
          - the chunk is a pure insertion and the adjacent lines (at most 2
            lines) belong to different non-public changesets, or do not belong
            to any non-public changesets.
          - the chunk is modifying lines from different changesets.
            in this case, if the number of lines deleted equals to the number
            of lines added, assume it's a simple 1:1 map (could be wrong).
            otherwise, give up.
          - the chunk is modifying lines from a single non-public changeset,
            but other revisions touch the area as well. i.e. the lines are
            not continuous as seen from the linelog.
        """
        a1, a2, b1, b2 = chunk
        # find involved indexes from annotate result
        involved = annotated[a1:a2]
        if not involved: # a1 == a2
            # pure insertion, check nearby lines. ignore lines belong
            # to the public (first) changeset (i.e. annotated[i][0] == 1)
            nearbylinenums = set([a2, max(0, a1 - 1)])
            involved = [annotated[i]
                        for i in nearbylinenums if annotated[i][0] != 1]
        involvedrevs = list(set(r for r, l in involved))
        newfixups = []
        if len(involvedrevs) == 1 and self._iscontinuous(a1, a2 - 1, True):
            # chunk belongs to a single revision
            rev = involvedrevs[0]
            if rev > 1:
                fixuprev = rev + 1
                newfixups.append((fixuprev, a1, a2, b1, b2))
        elif a2 - a1 == b2 - b1 or b1 == b2:
            # 1:1 line mapping, or chunk was deleted
            for i in xrange(a1, a2):
                rev, linenum = annotated[i]
                if rev > 1:
                    if b1 == b2: # deletion, simply remove that single line
                        nb1 = nb2 = 0
                    else: # 1:1 line mapping, change the corresponding rev
                        nb1 = b1 + i - a1
                        nb2 = nb1 + 1
                    fixuprev = rev + 1
                    newfixups.append((fixuprev, i, i + 1, nb1, nb2))
        return self._optimizefixups(newfixups)

    @staticmethod
    def _alldiffchunks(a, b, alines, blines):
        """like mdiff.allblocks, but only care about differences"""
        blocks = mdiff.allblocks(a, b, lines1=alines, lines2=blines)
        for chunk, btype in blocks:
            if btype != '!':
                continue
            yield chunk

    def _buildlinelog(self):
        """calculate the initial linelog based on self.content{,line}s.
        this is similar to running a partial "annotate".
        """
        llog = linelog.linelog()
        a, alines = '', []
        for i in xrange(len(self.contents)):
            b, blines = self.contents[i], self.contentlines[i]
            llrev = i * 2 + 1
            chunks = self._alldiffchunks(a, b, alines, blines)
            for a1, a2, b1, b2 in reversed(list(chunks)):
                llog.replacelines(llrev, a1, a2, b1, b2)
            a, alines = b, blines
        return llog

    def _checkoutlinelog(self):
        """() -> [str]. check out file contents from linelog"""
        contents = []
        for i in xrange(len(self.contents)):
            rev = (i + 1) * 2
            self.linelog.annotate(rev)
            content = ''.join(map(self._getline, self.linelog.annotateresult))
            contents.append(content)
        return contents

    def _getline(self, lineinfo):
        """((rev, linenum)) -> str. convert rev+line number to line content"""
        rev, linenum = lineinfo
        if rev & 1: # odd: original line taken from fctxs
            return self.contentlines[rev // 2][linenum]
        else: # even: fixup line from targetfctx
            return self.targetlines[linenum]

    def _iscontinuous(self, a1, a2, closedinterval=False):
        """(a1, a2 : int) -> bool

        check if these lines are continuous. i.e. no other insertions or
        deletions (from other revisions) among these lines.

        closedinterval decides whether a2 should be included or not. i.e. is
        it [a1, a2), or [a1, a2] ?
        """
        if a1 >= a2:
            return True
        llog = self.linelog
        offset1 = llog.getoffset(a1)
        offset2 = llog.getoffset(a2) + int(closedinterval)
        linesinbetween = llog.getalllines(offset1, offset2)
        return len(linesinbetween) == a2 - a1 + int(closedinterval)

    def _optimizefixups(self, fixups):
        """[(rev, a1, a2, b1, b2)] -> [(rev, a1, a2, b1, b2)].
        merge adjacent fixups to make them less fragmented.
        """
        result = []
        pcurrentchunk = [[-1, -1, -1, -1, -1]]

        def pushchunk():
            if pcurrentchunk[0][0] != -1:
                result.append(tuple(pcurrentchunk[0]))

        for i, chunk in enumerate(fixups):
            rev, a1, a2, b1, b2 = chunk
            lastrev = pcurrentchunk[0][0]
            lasta2 = pcurrentchunk[0][2]
            lastb2 = pcurrentchunk[0][4]
            if (a1 == lasta2 and b1 == lastb2 and rev == lastrev and
                    self._iscontinuous(max(a1 - 1, 0), a1)):
                # merge into currentchunk
                pcurrentchunk[0][2] = a2
                pcurrentchunk[0][4] = b2
            else:
                pushchunk()
                pcurrentchunk[0] = list(chunk)
        pushchunk()
        return result
