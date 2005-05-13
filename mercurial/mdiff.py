#!/usr/bin/python
import difflib, struct
from cStringIO import StringIO

def unidiff(a, ad, b, bd, fn):
    if not a and not b: return ""
    a = a.splitlines(1)
    b = b.splitlines(1)
    l = list(difflib.unified_diff(a, b, "a/" + fn, "b/" + fn, ad, bd))
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
        d = sortdiff(a, b)
    else:
        d = difflib.SequenceMatcher(None, a, b).get_opcodes()

    for o, m, n, s, t in d:
        if o == 'equal': continue
        s = "".join(b[s:t])
        bin.append(struct.pack(">lll", p[m], p[n], len(s)) + s)

    return "".join(bin)

def patch(a, bin):
    last = pos = 0
    r = []

    c = 0
    while pos < len(bin):
        p1, p2, l = struct.unpack(">lll", bin[pos:pos + 12])
        pos += 12
        r.append(a[last:p1])
        r.append(bin[pos:pos + l])
        pos += l
        last = p2
        c += 1
    r.append(a[last:])

    return "".join(r)





