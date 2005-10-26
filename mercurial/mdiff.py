# mdiff.py - diff and patch routines for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import difflib, struct, bdiff, util, mpatch

def unidiff(a, ad, b, bd, fn, r=None, text=False):

    if not a and not b: return ""
    epoch = util.datestr((0, 0))

    if not text and (util.binary(a) or util.binary(b)):
        l = ['Binary file %s has changed\n' % fn]
    elif a == None:
        b = b.splitlines(1)
        l1 = "--- %s\t%s\n" % ("/dev/null", epoch)
        l2 = "+++ %s\t%s\n" % ("b/" + fn, bd)
        l3 = "@@ -0,0 +1,%d @@\n" % len(b)
        l = [l1, l2, l3] + ["+" + e for e in b]
    elif b == None:
        a = a.splitlines(1)
        l1 = "--- %s\t%s\n" % ("a/" + fn, ad)
        l2 = "+++ %s\t%s\n" % ("/dev/null", epoch)
        l3 = "@@ -1,%d +0,0 @@\n" % len(a)
        l = [l1, l2, l3] + ["-" + e for e in a]
    else:
        a = a.splitlines(1)
        b = b.splitlines(1)
        l = list(difflib.unified_diff(a, b, "a/" + fn, "b/" + fn))
        if not l: return ""
        # difflib uses a space, rather than a tab
        l[0] = l[0][:-2] + "\t" + ad + "\n"
        l[1] = l[1][:-2] + "\t" + bd + "\n"

    for ln in xrange(len(l)):
        if l[ln][-1] != '\n':
            l[ln] += "\n\ No newline at end of file\n"

    if r:
        l.insert(0, "diff %s %s\n" %
                    (' '.join(["-r %s" % rev for rev in r]), fn))

    return "".join(l)

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
textdiff = bdiff.bdiff
