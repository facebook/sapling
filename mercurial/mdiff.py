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

    while 1:
        if la >= len(a) or lb >= len(b): break
        if b[lb] < a[la]:
            si = lb
            while lb < len(b) and b[lb] < a[la] : lb += 1
            yield "insert", la, la, si, lb
        elif a[la] < b[lb]:
            si = la
            while la < len(a) and a[la] < b[lb]: la += 1
            yield "delete", si, la, lb, lb
        else:
            la += 1
            lb += 1

    if lb < len(b):
        yield "insert", la, la, lb, len(b)

    if la < len(a):
        yield "delete", la, len(a), lb, lb

def diff(a, b, sorted=0):
    bin = []
    p = [0]
    for i in a: p.append(p[-1] + len(i))

    if sorted:
        try:
            d = sortdiff(a, b)
        except:
            print a, b
            raise
    else:
        d = difflib.SequenceMatcher(None, a, b).get_opcodes()

    for o, m, n, s, t in d:
        if o == 'equal': continue
        s = "".join(b[s:t])
        bin.append(struct.pack(">lll", p[m], p[n], len(s)) + s)

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
