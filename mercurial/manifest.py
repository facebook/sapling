# manifest.py - manifest revision class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import mdiff, parsers, error, revlog, util
import array, struct


class _lazymanifest(dict):
    """This is the pure implementation of lazymanifest.

    It has not been optimized *at all* and is not lazy.
    """

    def __init__(self, data):
        # This init method does a little bit of excessive-looking
        # precondition checking. This is so that the behavior of this
        # class exactly matches its C counterpart to try and help
        # prevent surprise breakage for anyone that develops against
        # the pure version.
        if data and data[-1] != '\n':
            raise ValueError('Manifest did not end in a newline.')
        dict.__init__(self)
        prev = None
        for l in data.splitlines():
            if prev is not None and prev > l:
                raise ValueError('Manifest lines not in sorted order.')
            prev = l
            f, n = l.split('\0')
            if len(n) > 40:
                self[f] = revlog.bin(n[:40]), n[40:]
            else:
                self[f] = revlog.bin(n), ''

    def __setitem__(self, k, v):
        node, flag = v
        assert node is not None
        if len(node) > 21:
            node = node[:21] # match c implementation behavior
        dict.__setitem__(self, k, (node, flag))

    def __iter__(self):
        return ((f, e[0], e[1]) for f, e in sorted(self.iteritems()))

    def copy(self):
        c = _lazymanifest('')
        c.update(self)
        return c

    def diff(self, m2, clean=False):
        '''Finds changes between the current manifest and m2.'''
        diff = {}

        for fn, e1 in self.iteritems():
            if fn not in m2:
                diff[fn] = e1, (None, '')
            else:
                e2 = m2[fn]
                if e1 != e2:
                    diff[fn] = e1, e2
                elif clean:
                    diff[fn] = None

        for fn, e2 in m2.iteritems():
            if fn not in self:
                diff[fn] = (None, ''), e2

        return diff

    def filtercopy(self, filterfn):
        c = _lazymanifest('')
        for f, n, fl in self:
            if filterfn(f):
                c[f] = n, fl
        return c

    def text(self):
        """Get the full data of this manifest as a bytestring."""
        fl = sorted(self)

        _hex = revlog.hex
        # if this is changed to support newlines in filenames,
        # be sure to check the templates/ dir again (especially *-raw.tmpl)
        return ''.join("%s\0%s%s\n" % (
            f, _hex(n[:20]), flag) for f, n, flag in fl)

try:
    _lazymanifest = parsers.lazymanifest
except AttributeError:
    pass

class manifestdict(object):
    def __init__(self, data=''):
        self._lm = _lazymanifest(data)

    def __getitem__(self, key):
        return self._lm[key][0]

    def find(self, key):
        return self._lm[key]

    def __len__(self):
        return len(self._lm)

    def __setitem__(self, key, node):
        self._lm[key] = node, self.flags(key, '')

    def __contains__(self, key):
        return key in self._lm

    def __delitem__(self, key):
        del self._lm[key]

    def keys(self):
        return [x[0] for x in self._lm]

    def iterkeys(self):
        return iter(self.keys())

    def intersectfiles(self, files):
        '''make a new lazymanifest with the intersection of self with files

        The algorithm assumes that files is much smaller than self.'''
        ret = manifestdict()
        lm = self._lm
        for fn in files:
            if fn in lm:
                ret._lm[fn] = self._lm[fn]
        return ret

    def filesnotin(self, m2):
        '''Set of files in this manifest that are not in the other'''
        files = set(self.iterkeys())
        files.difference_update(m2.iterkeys())
        return files

    def matches(self, match):
        '''generate a new manifest filtered by the match argument'''
        if match.always():
            return self.copy()

        files = match.files()
        if (match.matchfn == match.exact or
            (not match.anypats() and util.all(fn in self for fn in files))):
            return self.intersectfiles(files)

        lm = manifestdict('')
        lm._lm = self._lm.filtercopy(match)
        return lm

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
        return self._lm.diff(m2._lm, clean)

    def setflag(self, key, flag):
        self._lm[key] = self[key], flag

    def get(self, key, default=None):
        try:
            return self._lm[key][0]
        except KeyError:
            return default

    def flags(self, key, default=''):
        try:
            return self._lm[key][1]
        except KeyError:
            return default

    def __iter__(self):
        return (x[0] for x in self._lm)

    def copy(self):
        c = manifestdict('')
        c._lm = self._lm.copy()
        return c

    def iteritems(self):
        return (x[:2] for x in self._lm)

    def text(self):
        return self._lm.text()

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
                h, fl = self._lm[f]
                l = "%s\0%s%s\n" % (f, revlog.hex(h), fl)
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
        d = mdiff.patchtext(self.revdiff(self.deltaparent(r), r))
        return manifestdict(d)

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
        m = manifestdict(text)
        self._mancache[node] = (m, arraytext)
        return m

    def find(self, node, f):
        '''look up entry for a single file efficiently.
        return (node, flags) pair if found, (None, None) if not.'''
        m = self.read(node)
        try:
            return m.find(f)
        except KeyError:
            return None, None

    def add(self, m, transaction, link, p1, p2, added, removed):
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

            arraytext, deltatext = m.fastdelta(self._mancache[p1][1], work)
            cachedelta = self.rev(p1), deltatext
            text = util.buffer(arraytext)
        else:
            # The first parent manifest isn't already loaded, so we'll
            # just encode a fulltext of the manifest and pass that
            # through to the revlog layer, and let it handle the delta
            # process.
            text = m.text()
            arraytext = array.array('c', text)
            cachedelta = None

        n = self.addrevision(text, transaction, link, p1, p2, cachedelta)
        self._mancache[n] = (m, arraytext)

        return n
