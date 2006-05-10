# mdiff.py - diff and patch routines for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from demandload import demandload
import struct, bdiff, util, mpatch
demandload(globals(), "re")


def splitnewlines(text, keepends=False):
    '''like str.splitlines, but only split on newlines.'''
    i = 0
    lines = []
    while True:
        n = text.find('\n', i)
        if n == -1:
            last = text[i:]
            if last:
                lines.append(last)
            return lines
        lines.append(text[i:keepends and n+1 or n])
        i = n + 1

def unidiff(a, ad, b, bd, fn, r=None, text=False,
            showfunc=False, ignorews=False):

    if not a and not b: return ""
    epoch = util.datestr((0, 0))

    if not text and (util.binary(a) or util.binary(b)):
        l = ['Binary file %s has changed\n' % fn]
    elif not a:
        b = splitnewlines(b, keepends=True)
        if a is None:
            l1 = "--- %s\t%s\n" % ("/dev/null", epoch)
        else:
            l1 = "--- %s\t%s\n" % ("a/" + fn, ad)
        l2 = "+++ %s\t%s\n" % ("b/" + fn, bd)
        l3 = "@@ -0,0 +1,%d @@\n" % len(b)
        l = [l1, l2, l3] + ["+" + e for e in b]
    elif not b:
        a = splitnewlines(a, keepends=True)
        l1 = "--- %s\t%s\n" % ("a/" + fn, ad)
        if b is None:
            l2 = "+++ %s\t%s\n" % ("/dev/null", epoch)
        else:
            l2 = "+++ %s\t%s\n" % ("b/" + fn, bd)
        l3 = "@@ -1,%d +0,0 @@\n" % len(a)
        l = [l1, l2, l3] + ["-" + e for e in a]
    else:
        al = splitnewlines(a, keepends=True)
        bl = splitnewlines(b, keepends=True)
        l = list(bunidiff(a, b, al, bl, "a/" + fn, "b/" + fn,
                          showfunc=showfunc, ignorews=ignorews))
        if not l: return ""
        # difflib uses a space, rather than a tab
        l[0] = "%s\t%s\n" % (l[0][:-2], ad)
        l[1] = "%s\t%s\n" % (l[1][:-2], bd)

    for ln in xrange(len(l)):
        if l[ln][-1] != '\n':
            l[ln] += "\n\ No newline at end of file\n"

    if r:
        l.insert(0, "diff %s %s\n" %
                    (' '.join(["-r %s" % rev for rev in r]), fn))

    return "".join(l)

# somewhat self contained replacement for difflib.unified_diff
# t1 and t2 are the text to be diffed
# l1 and l2 are the text broken up into lines
# header1 and header2 are the filenames for the diff output
# context is the number of context lines
# showfunc enables diff -p output
# ignorews ignores all whitespace changes in the diff
def bunidiff(t1, t2, l1, l2, header1, header2, context=3, showfunc=False,
             ignorews=False):
    def contextend(l, len):
        ret = l + context
        if ret > len:
            ret = len
        return ret

    def contextstart(l):
        ret = l - context
        if ret < 0:
            return 0
        return ret

    def yieldhunk(hunk, header):
        if header:
            for x in header:
                yield x
        (astart, a2, bstart, b2, delta) = hunk
        aend = contextend(a2, len(l1))
        alen = aend - astart
        blen = b2 - bstart + aend - a2

        func = ""
        if showfunc:
            # walk backwards from the start of the context
            # to find a line starting with an alphanumeric char.
            for x in xrange(astart, -1, -1):
                t = l1[x].rstrip()
                if funcre.match(t):
                    func = ' ' + t[:40]
                    break

        yield "@@ -%d,%d +%d,%d @@%s\n" % (astart + 1, alen,
                                           bstart + 1, blen, func)
        for x in delta:
            yield x
        for x in xrange(a2, aend):
            yield ' ' + l1[x]

    header = [ "--- %s\t\n" % header1, "+++ %s\t\n" % header2 ]

    if showfunc:
        funcre = re.compile('\w')
    if ignorews:
        wsre = re.compile('[ \t]')

    # bdiff.blocks gives us the matching sequences in the files.  The loop
    # below finds the spaces between those matching sequences and translates
    # them into diff output.
    #
    diff = bdiff.blocks(t1, t2)
    hunk = None
    for i in xrange(len(diff)):
        # The first match is special.
        # we've either found a match starting at line 0 or a match later
        # in the file.  If it starts later, old and new below will both be
        # empty and we'll continue to the next match.
        if i > 0:
            s = diff[i-1]
        else:
            s = [0, 0, 0, 0]
        delta = []
        s1 = diff[i]
        a1 = s[1]
        a2 = s1[0]
        b1 = s[3]
        b2 = s1[2]

        old = l1[a1:a2]
        new = l2[b1:b2]

        # bdiff sometimes gives huge matches past eof, this check eats them,
        # and deals with the special first match case described above
        if not old and not new:
            continue

        if ignorews:
            wsold = wsre.sub('', "".join(old))
            wsnew = wsre.sub('', "".join(new))
            if wsold == wsnew:
                continue

        astart = contextstart(a1)
        bstart = contextstart(b1)
        prev = None
        if hunk:
            # join with the previous hunk if it falls inside the context
            if astart < hunk[1] + context + 1:
                prev = hunk
                astart = hunk[1]
                bstart = hunk[3]
            else:
                for x in yieldhunk(hunk, header):
                    yield x
                # we only want to yield the header if the files differ, and
                # we only want to yield it once.
                header = None
        if prev:
            # we've joined the previous hunk, record the new ending points.
            hunk[1] = a2
            hunk[3] = b2
            delta = hunk[4]
        else:
            # create a new hunk
            hunk = [ astart, a2, bstart, b2, delta ]

        delta[len(delta):] = [ ' ' + x for x in l1[astart:a1] ]
        delta[len(delta):] = [ '-' + x for x in old ]
        delta[len(delta):] = [ '+' + x for x in new ]

    if hunk:
        for x in yieldhunk(hunk, header):
            yield x

def patchtext(bin):
    pos = 0
    t = []
    while pos < len(bin):
        p1, p2, l = struct.unpack(">lll", bin[pos:pos + 12])
        pos += 12
        t.append(bin[pos:pos + l])
        pos += l
    return "".join(t)

def patch(a, bin):
    return mpatch.patches(a, [bin])

patches = mpatch.patches
patchedsize = mpatch.patchedsize
textdiff = bdiff.bdiff
