# mdiff.py - diff and patch routines for mercurial
#
# Copyright 2005 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import difflib, struct
from mercurial.mpatch import *

def unidiff(a, ad, b, bd, fn):
    if not a and not b: return ""

    if a == None:
        b = b.splitlines(1)
        l1 = "--- %s\t%s\n" % ("/dev/null", ad)
        l2 = "+++ %s\t%s\n" % ("b/" + fn, bd)
        l3 = "@@ -0,0 +1,%d @@\n" % len(b)
        l = [l1, l2, l3] + ["+" + e for e in b]
    elif b == None:
        a = a.splitlines(1)
        l1 = "--- %s\t%s\n" % ("a/" + fn, ad)
        l2 = "+++ %s\t%s\n" % ("/dev/null", bd)
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

    return "".join(l)

def textdiff(a, b):
    return diff(a.splitlines(1), b.splitlines(1))

def sortdiff(a, b):
    la = lb = 0
    lena = len(a)
    lenb = len(b)
    while 1:
        am, bm, = la, lb
        while lb < lenb and la < len and a[la] == b[lb] :
            la += 1
            lb += 1
        if la>am: yield (am, bm, la-am)
        while lb < lenb and b[lb] < a[la]: lb += 1
        if lb>=lenb: break
        while la < lena and b[lb] > a[la]: la += 1
        if la>=lena: break
    yield (lena, lenb, 0)

def diff(a, b, sorted=0):
    if not a:
        s = "".join(b)
        return s and (struct.pack(">lll", 0, 0, len(s)) + s)

    bin = []
    p = [0]
    for i in a: p.append(p[-1] + len(i))

    if sorted:
        d = sortdiff(a, b)
    else:
        d = difflib.SequenceMatcher(None, a, b).get_matching_blocks()
    la = 0
    lb = 0
    for am, bm, size in d:
        s = "".join(b[lb:bm])
        if am > la or s:
            bin.append(struct.pack(">lll", p[la], p[am], len(s)) + s)
        la = am + size
        lb = bm + size
    
    return "".join(bin)

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
    return patches(a, [bin])
