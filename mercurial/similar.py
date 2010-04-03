# similar.py - mechanisms for finding similar files
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import util
import mdiff
import bdiff

def findrenames(repo, added, removed, threshold):
    '''find renamed files -- yields (before, after, score) tuples'''
    copies = {}
    ctx = repo['.']
    for i, r in enumerate(removed):
        repo.ui.progress(_('searching'), i, total=len(removed))
        if r not in ctx:
            continue
        fctx = ctx.filectx(r)

        # lazily load text
        @util.cachefunc
        def data():
            orig = fctx.data()
            return orig, mdiff.splitnewlines(orig)

        def score(text):
            if not len(text):
                return 0.0
            if not fctx.cmp(text):
                return 1.0
            if threshold == 1.0:
                return 0.0
            orig, lines = data()
            # bdiff.blocks() returns blocks of matching lines
            # count the number of bytes in each
            equal = 0
            matches = bdiff.blocks(text, orig)
            for x1, x2, y1, y2 in matches:
                for line in lines[y1:y2]:
                    equal += len(line)

            lengths = len(text) + len(orig)
            return equal * 2.0 / lengths

        for a in added:
            bestscore = copies.get(a, (None, threshold))[1]
            myscore = score(repo.wread(a))
            if myscore >= bestscore:
                copies[a] = (r, myscore)
    repo.ui.progress(_('searching'), None)

    for dest, v in copies.iteritems():
        source, score = v
        yield source, dest, score


