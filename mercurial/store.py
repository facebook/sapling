# store.py - repository store handling for Mercurial
#
# Copyright 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

def _buildencodefun():
    e = '_'
    win_reserved = [ord(x) for x in '\\:*?"<>|']
    cmap = dict([ (chr(x), chr(x)) for x in xrange(127) ])
    for x in (range(32) + range(126, 256) + win_reserved):
        cmap[chr(x)] = "~%02x" % x
    for x in range(ord("A"), ord("Z")+1) + [ord(e)]:
        cmap[chr(x)] = e + chr(x).lower()
    dmap = {}
    for k, v in cmap.iteritems():
        dmap[v] = k
    def decode(s):
        i = 0
        while i < len(s):
            for l in xrange(1, 4):
                try:
                    yield dmap[s[i:i+l]]
                    i += l
                    break
                except KeyError:
                    pass
            else:
                raise KeyError
    return (lambda s: "".join([cmap[c] for c in s]),
            lambda s: "".join(list(decode(s))))

encodefilename, decodefilename = _buildencodefun()

def encodedopener(openerfn, fn):
    def o(path, *args, **kw):
        return openerfn(fn(path), *args, **kw)
    return o
