# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# mdiff.py - diff and patch routines for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import re
import struct
import zlib
from hashlib import sha1

# pyre-fixme[21]: Could not find `edenscmnative`.
from edenscmnative import xdiff

from . import error, policy, pycompat, util
from .i18n import _
from .pycompat import range


_missing_newline_marker = "\\ No newline at end of file\n"

bdiff = policy.importmod(r"bdiff")
mpatch = policy.importmod(r"mpatch")

blocks = bdiff.blocks
fixws = bdiff.fixws
patches = mpatch.patches
patchedsize = mpatch.patchedsize
textdiff = bdiff.bdiff


# called by dispatch.py
def init(ui):
    if ui.configbool("experimental", "xdiff"):
        global blocks
        blocks = xdiff.blocks


def splitnewlines(text):
    """like str.splitlines, but only split on newlines."""
    lines = [l + "\n" for l in text.split("\n")]
    if lines:
        if lines[-1] == "\n":
            lines.pop()
        else:
            lines[-1] = lines[-1][:-1]
    return lines


class diffopts(object):
    """context is the number of context lines
    text treats all files as text
    showfunc enables diff -p output
    git enables the git extended patch format
    nodates removes dates from diff headers
    nobinary ignores binary files
    noprefix disables the 'a/' and 'b/' prefixes (ignored in plain mode)
    ignorews ignores all whitespace changes in the diff
    ignorewsamount ignores changes in the amount of whitespace
    ignoreblanklines ignores changes whose lines are all blank
    upgrade generates git diffs to avoid data loss
    filtercopysource removes copy/rename if source does not match matcher
    """

    defaults = {
        "context": 3,
        "text": False,
        "showfunc": False,
        "git": False,
        "nodates": False,
        "nobinary": False,
        "noprefix": False,
        "index": 0,
        "ignorews": False,
        "ignorewsamount": False,
        "ignorewseol": False,
        "ignoreblanklines": False,
        "upgrade": False,
        "showsimilarity": False,
        "worddiff": False,
        "hashbinary": False,
        "filtercopysource": False,
    }

    def __init__(self, **opts):
        opts = pycompat.byteskwargs(opts)
        for k in self.defaults.keys():
            v = opts.get(k)
            if v is None:
                v = self.defaults[k]
            setattr(self, k, v)

        try:
            self.context = int(self.context)
        except ValueError:
            raise error.Abort(
                _("diff context lines count must be " "an integer, not %r")
                % self.context
            )

    def copy(self, **kwargs):
        opts = dict((k, getattr(self, k)) for k in self.defaults)
        opts = pycompat.strkwargs(opts)
        opts.update(kwargs)
        return diffopts(**opts)


defaultopts = diffopts()


def wsclean(opts, text, blank=True):
    if opts.ignorews:
        text = bdiff.fixws(text, 1)
    elif opts.ignorewsamount:
        text = bdiff.fixws(text, 0)
    if blank and opts.ignoreblanklines:
        text = re.sub("\n+", "\n", text).strip("\n")
    if opts.ignorewseol:
        text = re.sub(r"[ \t\r\f]+\n", r"\n", text)
    return text


def splitblock(base1, lines1, base2, lines2, opts):
    # The input lines matches except for interwoven blank lines. We
    # transform it into a sequence of matching blocks and blank blocks.
    lines1 = [(wsclean(opts, l) and 1 or 0) for l in lines1]
    lines2 = [(wsclean(opts, l) and 1 or 0) for l in lines2]
    s1, e1 = 0, len(lines1)
    s2, e2 = 0, len(lines2)
    while s1 < e1 or s2 < e2:
        i1, i2, btype = s1, s2, "="
        if i1 >= e1 or lines1[i1] == 0 or i2 >= e2 or lines2[i2] == 0:
            # Consume the block of blank lines
            btype = "~"
            while i1 < e1 and lines1[i1] == 0:
                i1 += 1
            while i2 < e2 and lines2[i2] == 0:
                i2 += 1
        else:
            # Consume the matching lines
            while i1 < e1 and lines1[i1] == 1 and lines2[i2] == 1:
                i1 += 1
                i2 += 1
        yield [base1 + s1, base1 + i1, base2 + s2, base2 + i2], btype
        s1 = i1
        s2 = i2


def hunkinrange(hunk, linerange):
    """Return True if `hunk` defined as (start, length) is in `linerange`
    defined as (lowerbound, upperbound).

    >>> hunkinrange((5, 10), (2, 7))
    True
    >>> hunkinrange((5, 10), (6, 12))
    True
    >>> hunkinrange((5, 10), (13, 17))
    True
    >>> hunkinrange((5, 10), (3, 17))
    True
    >>> hunkinrange((5, 10), (1, 3))
    False
    >>> hunkinrange((5, 10), (18, 20))
    False
    >>> hunkinrange((5, 10), (1, 5))
    False
    >>> hunkinrange((5, 10), (15, 27))
    False
    """
    start, length = hunk
    lowerbound, upperbound = linerange
    return lowerbound < start + length and start < upperbound


def blocksinrange(blocks, rangeb):
    """filter `blocks` like (a1, a2, b1, b2) from items outside line range
    `rangeb` from ``(b1, b2)`` point of view.

    Return `filteredblocks, rangea` where:

    * `filteredblocks` is list of ``block = (a1, a2, b1, b2), stype`` items of
      `blocks` that are inside `rangeb` from ``(b1, b2)`` point of view; a
      block ``(b1, b2)`` being inside `rangeb` if
      ``rangeb[0] < b2 and b1 < rangeb[1]``;
    * `rangea` is the line range w.r.t. to ``(a1, a2)`` parts of `blocks`.
    """
    lbb, ubb = rangeb
    lba, uba = None, None
    filteredblocks = []
    for block in blocks:
        (a1, a2, b1, b2), stype = block
        if lbb >= b1 and ubb <= b2 and stype == "=":
            # rangeb is within a single "=" hunk, restrict back linerange1
            # by offsetting rangeb
            lba = lbb - b1 + a1
            uba = ubb - b1 + a1
        else:
            if b1 <= lbb < b2:
                if stype == "=":
                    lba = a2 - (b2 - lbb)
                else:
                    lba = a1
            if b1 < ubb <= b2:
                if stype == "=":
                    uba = a1 + (ubb - b1)
                else:
                    uba = a2
        if hunkinrange((b1, (b2 - b1)), rangeb):
            filteredblocks.append(block)
    if lba is None or uba is None or uba < lba:
        raise error.Abort(_("line range exceeds file size"))
    return filteredblocks, (lba, uba)


def allblocks(text1, text2, opts=None, lines1=None, lines2=None):
    """Return (block, type) tuples, where block is an mdiff.blocks
    line entry. type is '=' for blocks matching exactly one another
    (bdiff blocks), '!' for non-matching blocks and '~' for blocks
    matching only after having filtered blank lines.
    line1 and line2 are text1 and text2 split with splitnewlines() if
    they are already available.
    """
    if opts is None:
        opts = defaultopts
    if opts.ignorews or opts.ignorewsamount or opts.ignorewseol:
        text1 = wsclean(opts, text1, False)
        text2 = wsclean(opts, text2, False)
    diff = blocks(text1, text2)
    for i, s1 in enumerate(diff):
        # The first match is special.
        # we've either found a match starting at line 0 or a match later
        # in the file.  If it starts later, old and new below will both be
        # empty and we'll continue to the next match.
        if i > 0:
            s = diff[i - 1]
        else:
            s = [0, 0, 0, 0]
        s = [s[1], s1[0], s[3], s1[2]]

        # bdiff sometimes gives huge matches past eof, this check eats them,
        # and deals with the special first match case described above
        if s[0] != s[1] or s[2] != s[3]:
            type = "!"
            if opts.ignoreblanklines:
                if lines1 is None:
                    lines1 = splitnewlines(text1)
                if lines2 is None:
                    lines2 = splitnewlines(text2)
                old = wsclean(opts, "".join(lines1[s[0] : s[1]]))
                new = wsclean(opts, "".join(lines2[s[2] : s[3]]))
                if old == new:
                    type = "~"
            yield s, type
        yield s1, "="


def unidiff(a, ad, b, bd, fn1, fn2, opts=defaultopts, check_binary=True):
    """Return a unified diff as a (headers, hunks) tuple.

    If the diff is not null, `headers` is a list with unified diff header
    lines "--- <original>" and "+++ <new>" and `hunks` is a generator yielding
    (hunkrange, hunklines) coming from _unidiff().
    Otherwise, `headers` and `hunks` are empty.

    Setting `check_binary` to false will skip the binary check, i.e. when
    it has been done in advance. Files are expected to be text in this case.
    """

    def datetag(date, fn=None):
        if not opts.git and not opts.nodates:
            return "\t%s" % date
        if fn and " " in fn:
            return "\t"
        return ""

    sentinel = [], ()
    if not a and not b:
        return sentinel

    if opts.noprefix:
        aprefix = bprefix = ""
    else:
        aprefix = "a/"
        bprefix = "b/"

    epoch = util.datestr((0, 0))

    fn1 = util.pconvert(fn1)
    fn2 = util.pconvert(fn2)

    if not opts.text and check_binary and (util.binary(a) or util.binary(b)):
        if a and b and len(a) == len(b) and a == b:
            return sentinel
        headerlines = []
        if opts.hashbinary:
            message = "Binary file %s has changed to %s\n" % (fn1, sha1(b).hexdigest())
        else:
            message = "Binary file %s has changed\n" % fn1
        hunks = ((None, [message]),)
    elif not a:
        without_newline = b[-1] != "\n"
        b = splitnewlines(b)
        if a is None:
            l1 = "--- /dev/null%s" % datetag(epoch)
        else:
            l1 = "--- %s%s%s" % (aprefix, fn1, datetag(ad, fn1))
        l2 = "+++ %s%s" % (bprefix + fn2, datetag(bd, fn2))
        headerlines = [l1, l2]
        size = len(b)
        hunkrange = (0, 0, 1, size)
        hunklines = ["@@ -0,0 +1,%d @@\n" % size] + ["+" + e for e in b]
        if without_newline:
            hunklines[-1] += "\n"
            hunklines.append(_missing_newline_marker)
        hunks = ((hunkrange, hunklines),)
    elif not b:
        without_newline = a[-1] != "\n"
        a = splitnewlines(a)
        l1 = "--- %s%s%s" % (aprefix, fn1, datetag(ad, fn1))
        if b is None:
            l2 = "+++ /dev/null%s" % datetag(epoch)
        else:
            l2 = "+++ %s%s%s" % (bprefix, fn2, datetag(bd, fn2))
        headerlines = [l1, l2]
        size = len(a)
        hunkrange = (1, size, 0, 0)
        hunklines = ["@@ -1,%d +0,0 @@\n" % size] + ["-" + e for e in a]
        if without_newline:
            hunklines[-1] += "\n"
            hunklines.append(_missing_newline_marker)
        hunks = ((hunkrange, hunklines),)
    else:
        hunks = _unidiff(a, b, opts=opts)
        if not next(hunks):
            return sentinel

        headerlines = [
            "--- %s%s%s" % (aprefix, fn1, datetag(ad, fn1)),
            "+++ %s%s%s" % (bprefix, fn2, datetag(bd, fn2)),
        ]

    return headerlines, hunks


def _unidiff(t1, t2, opts=defaultopts):
    """Yield hunks of a headerless unified diff from t1 and t2 texts.

    Each hunk consists of a (hunkrange, hunklines) tuple where `hunkrange` is a
    tuple (s1, l1, s2, l2) representing the range information of the hunk to
    form the '@@ -s1,l1 +s2,l2 @@' header and `hunklines` is a list of lines
    of the hunk combining said header followed by line additions and
    deletions.

    The hunks are prefixed with a bool.
    """
    l1 = splitnewlines(t1)
    l2 = splitnewlines(t2)

    def contextend(l, len):
        ret = l + opts.context
        if ret > len:
            ret = len
        return ret

    def contextstart(l):
        ret = l - opts.context
        if ret < 0:
            return 0
        return ret

    lastfunc = [0, ""]

    def yieldhunk(hunk):
        (astart, a2, bstart, b2, delta) = hunk
        aend = contextend(a2, len(l1))
        alen = aend - astart
        blen = b2 - bstart + aend - a2

        func = ""
        if opts.showfunc:
            lastpos, func = lastfunc
            # walk backwards from the start of the context up to the start of
            # the previous hunk context until we find a line starting with an
            # alphanumeric char.
            for i in range(astart - 1, lastpos - 1, -1):
                if l1[i][0].isalnum():
                    func = " " + l1[i].rstrip()[:40]
                    lastfunc[1] = func
                    break
            # by recording this hunk's starting point as the next place to
            # start looking for function lines, we avoid reading any line in
            # the file more than once.
            lastfunc[0] = astart

        # zero-length hunk ranges report their start line as one less
        if alen:
            astart += 1
        if blen:
            bstart += 1

        hunkrange = astart, alen, bstart, blen
        hunklines = (
            ["@@ -%d,%d +%d,%d @@%s\n" % (hunkrange + (func,))]
            + delta
            + [" " + l1[x] for x in range(a2, aend)]
        )
        # If either file ends without a newline and the last line of
        # that file is part of a hunk, a marker is printed. If the
        # last line of both files is identical and neither ends in
        # a newline, print only one marker. That's the only case in
        # which the hunk can end in a shared line without a newline.
        skip = False
        if t1[-1] != "\n" and astart + alen == len(l1) + 1:
            for i in range(len(hunklines) - 1, -1, -1):
                if hunklines[i][0] in ("-", " "):
                    if hunklines[i][0] == " ":
                        skip = True
                    hunklines[i] += "\n"
                    hunklines.insert(i + 1, _missing_newline_marker)
                    break
        if not skip and t2[-1] != "\n" and bstart + blen == len(l2) + 1:
            for i in range(len(hunklines) - 1, -1, -1):
                if hunklines[i][0] == "+":
                    hunklines[i] += "\n"
                    hunklines.insert(i + 1, _missing_newline_marker)
                    break
        yield hunkrange, hunklines

    # bdiff.blocks gives us the matching sequences in the files.  The loop
    # below finds the spaces between those matching sequences and translates
    # them into diff output.
    #
    hunk = None
    ignoredlines = 0
    has_hunks = False
    for s, stype in allblocks(t1, t2, opts, l1, l2):
        a1, a2, b1, b2 = s
        if stype != "!":
            if stype == "~":
                # The diff context lines are based on t1 content. When
                # blank lines are ignored, the new lines offsets must
                # be adjusted as if equivalent blocks ('~') had the
                # same sizes on both sides.
                ignoredlines += (b2 - b1) - (a2 - a1)
            continue
        delta = []
        old = l1[a1:a2]
        new = l2[b1:b2]

        b1 -= ignoredlines
        b2 -= ignoredlines
        astart = contextstart(a1)
        bstart = contextstart(b1)
        prev = None
        if hunk:
            # join with the previous hunk if it falls inside the context
            if astart < hunk[1] + opts.context + 1:
                prev = hunk
                astart = hunk[1]
                bstart = hunk[3]
            else:
                if not has_hunks:
                    has_hunks = True
                    yield True
                for x in yieldhunk(hunk):
                    yield x
        if prev:
            # we've joined the previous hunk, record the new ending points.
            hunk[1] = a2
            hunk[3] = b2
            delta = hunk[4]
        else:
            # create a new hunk
            hunk = [astart, a2, bstart, b2, delta]

        delta[len(delta) :] = [" " + x for x in l1[astart:a1]]
        delta[len(delta) :] = ["-" + x for x in old]
        delta[len(delta) :] = ["+" + x for x in new]

    if hunk:
        if not has_hunks:
            has_hunks = True
            yield True
        for x in yieldhunk(hunk):
            yield x
    elif not has_hunks:
        yield False


def b85diff(to, tn):
    """print base85-encoded binary diff"""

    def fmtline(line):
        l = len(line)
        if l <= 26:
            l = chr(ord("A") + l - 1)
        else:
            l = chr(l - 26 + ord("a") - 1)
        return "%c%s\n" % (l, util.b85encode(line, True))

    def chunk(text, csize=52):
        l = len(text)
        i = 0
        while i < l:
            yield text[i : i + csize]
            i += csize

    if to is None:
        to = ""
    if tn is None:
        tn = ""

    if to == tn:
        return ""

    # TODO: deltas
    ret = []
    ret.append("GIT binary patch\n")
    ret.append("literal %d\n" % len(tn))
    for l in chunk(zlib.compress(tn)):
        ret.append(fmtline(l))
    ret.append("\n")

    return "".join(ret)


def patchtext(bin):
    pos = 0
    t = []
    while pos < len(bin):
        p1, p2, l = struct.unpack(">lll", bin[pos : pos + 12])
        pos += 12
        t.append(bin[pos : pos + l])
        pos += l
    return "".join(t)


def patch(a, bin):
    if len(a) == 0:
        # skip over trivial delta header
        return util.buffer(bin, 12)
    return mpatch.patches(a, [bin])


# similar to difflib.SequenceMatcher.get_matching_blocks
def get_matching_blocks(a, b):
    return [(d[0], d[2], d[1] - d[0]) for d in blocks(a, b)]


def trivialdiffheader(length):
    return struct.pack(">lll", 0, 0, length) if length else ""


def replacediffheader(oldlen, newlen):
    return struct.pack(">lll", 0, oldlen, newlen)
