# manifest.py - manifest revision class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import mdiff, parsers, error, revlog, util
import array, struct

class manifestdict(dict):
    def __init__(self):
        self._flags = {}
    def __setitem__(self, k, v):
        assert v is not None
        dict.__setitem__(self, k, v)
    def flags(self, f):
        return self._flags.get(f, "")
    def setflag(self, f, flags):
        """Set the flags (symlink, executable) for path f."""
        self._flags[f] = flags
    def copy(self):
        copy = manifestdict()
        dict.__init__(copy, self)
        copy._flags = dict.copy(self._flags)
        return copy
    def intersectfiles(self, files):
        '''make a new manifestdict with the intersection of self with files

        The algorithm assumes that files is much smaller than self.'''
        ret = manifestdict()
        for fn in files:
            if fn in self:
                ret[fn] = self[fn]
                flags = self._flags.get(fn, None)
                if flags:
                    ret._flags[fn] = flags
        return ret

    def matches(self, match):
        '''generate a new manifest filtered by the match argument'''
        if match.always():
            return self.copy()

        files = match.files()
        if (match.matchfn == match.exact or
            (not match.anypats() and util.all(fn in self for fn in files))):
            return self.intersectfiles(files)

        mf = self.copy()
        for fn in mf.keys():
            if not match(fn):
                del mf[fn]
        return mf

    def diff(self, m2, clean=False):
        '''Finds changes between the current manifest and m2.

        Args:
          m2: the manifest to which this manifest should be compared.
          clean: if true, include files unchanged between these manifests
                 with a None value in the returned dictionary.

        The result is returned as a dict with filename as key and
        values of the form ((n1,fl1),(n2,fl2)), where n1/n2 is the
        nodeid in the current/other manifest and fl1/fl2 is the flag
        in the current/other manifest. Where the file does not exist,
        the nodeid will be None and the flags will be the empty
        string.
        '''
        diff = {}

        for fn, n1 in self.iteritems():
            fl1 = self._flags.get(fn, '')
            n2 = m2.get(fn, None)
            fl2 = m2._flags.get(fn, '')
            if n2 is None:
                fl2 = ''
            if n1 != n2 or fl1 != fl2:
                diff[fn] = ((n1, fl1), (n2, fl2))
            elif clean:
                diff[fn] = None

        for fn, n2 in m2.iteritems():
            if fn not in self:
                fl2 = m2._flags.get(fn, '')
                diff[fn] = ((None, ''), (n2, fl2))

        return diff

    def text(self):
        """Get the full data of this manifest as a bytestring."""
        fl = sorted(self)
        _checkforbidden(fl)

        hex, flags = revlog.hex, self.flags
        # if this is changed to support newlines in filenames,
        # be sure to check the templates/ dir again (especially *-raw.tmpl)
        return ''.join("%s\0%s%s\n" % (f, hex(self[f]), flags(f)) for f in fl)

    def fastdelta(self, base, changes):
        """Given a base manifest text as an array.array and a list of changes
        relative to that text, compute a delta that can be used by revlog.
        """
        delta = []
        dstart = None
        dend = None
        dline = [""]
        start = 0
        # zero copy representation of base as a buffer
        addbuf = util.buffer(base)

        # start with a readonly loop that finds the offset of
        # each line and creates the deltas
        for f, todelete in changes:
            # bs will either be the index of the item or the insert point
            start, end = _msearch(addbuf, f, start)
            if not todelete:
                l = "%s\0%s%s\n" % (f, revlog.hex(self[f]), self.flags(f))
            else:
                if start == end:
                    # item we want to delete was not found, error out
                    raise AssertionError(
                            _("failed to remove %s from manifest") % f)
                l = ""
            if dstart is not None and dstart <= start and dend >= start:
                if dend < end:
                    dend = end
                if l:
                    dline.append(l)
            else:
                if dstart is not None:
                    delta.append([dstart, dend, "".join(dline)])
                dstart = start
                dend = end
                dline = [l]

        if dstart is not None:
            delta.append([dstart, dend, "".join(dline)])
        # apply the delta to the base, and get a delta for addrevision
        deltatext, arraytext = _addlistdelta(base, delta)
        return arraytext, deltatext

def _msearch(m, s, lo=0, hi=None):
    '''return a tuple (start, end) that says where to find s within m.

    If the string is found m[start:end] are the line containing
    that string.  If start == end the string was not found and
    they indicate the proper sorted insertion point.

    m should be a buffer or a string
    s is a string'''
    def advance(i, c):
        while i < lenm and m[i] != c:
            i += 1
        return i
    if not s:
        return (lo, lo)
    lenm = len(m)
    if not hi:
        hi = lenm
    while lo < hi:
        mid = (lo + hi) // 2
        start = mid
        while start > 0 and m[start - 1] != '\n':
            start -= 1
        end = advance(start, '\0')
        if m[start:end] < s:
            # we know that after the null there are 40 bytes of sha1
            # this translates to the bisect lo = mid + 1
            lo = advance(end + 40, '\n') + 1
        else:
            # this translates to the bisect hi = mid
            hi = start
    end = advance(lo, '\0')
    found = m[lo:end]
    if s == found:
        # we know that after the null there are 40 bytes of sha1
        end = advance(end + 40, '\n')
        return (lo, end + 1)
    else:
        return (lo, lo)

def _checkforbidden(l):
    """Check filenames for illegal characters."""
    for f in l:
        if '\n' in f or '\r' in f:
            raise error.RevlogError(
                _("'\\n' and '\\r' disallowed in filenames: %r") % f)


# apply the changes collected during the bisect loop to our addlist
# return a delta suitable for addrevision
def _addlistdelta(addlist, x):
    # for large addlist arrays, building a new array is cheaper
    # than repeatedly modifying the existing one
    currentposition = 0
    newaddlist = array.array('c')

    for start, end, content in x:
        newaddlist += addlist[currentposition:start]
        if content:
            newaddlist += array.array('c', content)

        currentposition = end

    newaddlist += addlist[currentposition:]

    deltatext = "".join(struct.pack(">lll", start, end, len(content))
                   + content for start, end, content in x)
    return deltatext, newaddlist

def _parse(lines):
    mfdict = manifestdict()
    parsers.parse_manifest(mfdict, mfdict._flags, lines)
    return mfdict

class manifest(revlog.revlog):
    def __init__(self, opener):
        # During normal operations, we expect to deal with not more than four
        # revs at a time (such as during commit --amend). When rebasing large
        # stacks of commits, the number can go up, hence the config knob below.
        cachesize = 4
        opts = getattr(opener, 'options', None)
        if opts is not None:
            cachesize = opts.get('manifestcachesize', cachesize)
        self._mancache = util.lrucachedict(cachesize)
        revlog.revlog.__init__(self, opener, "00manifest.i")

    def readdelta(self, node):
        r = self.rev(node)
        return _parse(mdiff.patchtext(self.revdiff(self.deltaparent(r), r)))

    def readfast(self, node):
        '''use the faster of readdelta or read'''
        r = self.rev(node)
        deltaparent = self.deltaparent(r)
        if deltaparent != revlog.nullrev and deltaparent in self.parentrevs(r):
            return self.readdelta(node)
        return self.read(node)

    def read(self, node):
        if node == revlog.nullid:
            return manifestdict() # don't upset local cache
        if node in self._mancache:
            return self._mancache[node][0]
        text = self.revision(node)
        arraytext = array.array('c', text)
        mapping = _parse(text)
        self._mancache[node] = (mapping, arraytext)
        return mapping

    def find(self, node, f):
        '''look up entry for a single file efficiently.
        return (node, flags) pair if found, (None, None) if not.'''
        if node in self._mancache:
            mapping = self._mancache[node][0]
            return mapping.get(f), mapping.flags(f)
        text = self.revision(node)
        start, end = _msearch(text, f)
        if start == end:
            return None, None
        l = text[start:end]
        f, n = l.split('\0')
        return revlog.bin(n[:40]), n[40:-1]

    def add(self, map, transaction, link, p1, p2, added, removed):
        if p1 in self._mancache:
            # If our first parent is in the manifest cache, we can
            # compute a delta here using properties we know about the
            # manifest up-front, which may save time later for the
            # revlog layer.

            _checkforbidden(added)
            # combine the changed lists into one list for sorting
            work = [(x, False) for x in added]
            work.extend((x, True) for x in removed)
            # this could use heapq.merge() (from Python 2.6+) or equivalent
            # since the lists are already sorted
            work.sort()

            arraytext, deltatext = map.fastdelta(self._mancache[p1][1], work)
            cachedelta = self.rev(p1), deltatext
            text = util.buffer(arraytext)
        else:
            # The first parent manifest isn't already loaded, so we'll
            # just encode a fulltext of the manifest and pass that
            # through to the revlog layer, and let it handle the delta
            # process.
            text = map.text()
            arraytext = array.array('c', text)
            cachedelta = None

        n = self.addrevision(text, transaction, link, p1, p2, cachedelta)
        self._mancache[n] = (map, arraytext)

        return n
