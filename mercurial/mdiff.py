#!/usr/bin/python
import difflib, struct, mmap

devzero = file("/dev/zero")

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

# This attempts to apply a series of patches in time proportional to
# the total size of the patches, rather than patches * len(text). This
# means rather than shuffling strings around, we shuffle around
# pointers to fragments with fragment lists.
#
# When the fragment lists get too long, we collapse them. To do this
# efficiently, we do all our operations inside a buffer created by
# mmap and simply use memmove. This avoids creating a bunch of large
# temporary string buffers.

def patches(a, bins):
    if not bins: return a

    plens = [len(x) for x in bins]
    pl = sum(plens)
    bl = len(a) + pl
    tl = bl + bl + pl # enough for the patches and two working texts
    b1, b2 = 0, bl

    if not tl: return a

    m = mmap.mmap(devzero.fileno(), tl, mmap.MAP_PRIVATE)

    # load our original text
    m.write(a)
    frags = [(len(a), b1)]

    # copy all the patches into our segment so we can memmove from them
    pos = b2 + bl
    m.seek(pos)
    for p in bins: m.write(p)

    def pull(dst, src, l): # pull l bytes from src
        while l:
            f = src.pop(0)
            if f[0] > l: # do we need to split?
                src.insert(0, (f[0] - l, f[1] + l))
                dst.append((l, f[1]))
                return
            dst.append(f)
            l -= f[0]

    def collect(buf, list):
        start = buf
        for l, p in list:
            m.move(buf, p, l)
            buf += l
        return (buf - start, start)

    for plen in plens:
        # if our list gets too long, execute it
        if len(frags) > 128:
            b2, b1 = b1, b2
            frags = [collect(b1, frags)]

        new = []
        end = pos + plen
        last = 0
        while pos < end:
            p1, p2, l = struct.unpack(">lll", m[pos:pos + 12])
            pull(new, frags, p1 - last) # what didn't change
            pull([], frags, p2 - p1)    # what got deleted
            new.append((l, pos + 12))        # what got added
            pos += l + 12
            last = p2
        frags = new + frags                    # what was left at the end

    t = collect(b2, frags)

    return m[t[1]:t[1] + t[0]]

def patch(a, bin):
    return patches(a, [bin])

try:
    import mpatch
    patches = mpatch.patches
except:
    pass
