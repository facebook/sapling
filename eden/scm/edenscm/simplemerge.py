# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright (C) 2004, 2005 Canonical Ltd
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, see <http://www.gnu.org/licenses/>.

# mbp: "you know that thing where cvs gives you conflict markers?"
# s: "i hate that."

from __future__ import absolute_import

from typing import List, Tuple

from . import error, mdiff, pycompat, util
from .i18n import _
from .pycompat import range


def intersect(ra, rb):
    """Given two ranges return the range where they intersect or None.

    >>> intersect((0, 10), (0, 6))
    (0, 6)
    >>> intersect((0, 10), (5, 15))
    (5, 10)
    >>> intersect((0, 10), (10, 15))
    >>> intersect((0, 9), (10, 15))
    >>> intersect((0, 9), (7, 15))
    (7, 9)
    """
    assert ra[0] <= ra[1]
    assert rb[0] <= rb[1]

    sa = max(ra[0], rb[0])
    sb = min(ra[1], rb[1])
    if sa < sb:
        return sa, sb
    else:
        return None


def compare_range(a, astart, aend, b, bstart, bend):
    """Compare a[astart:aend] == b[bstart:bend], without slicing."""
    if (aend - astart) != (bend - bstart):
        return False
    for ia, ib in zip(range(astart, aend), range(bstart, bend)):
        if a[ia] != b[ib]:
            return False
    return True


class CantShowWordConflicts(Exception):
    pass


class wordmergemode:
    enforced = "enforced"  # Enforced word merge. Cannot draw conflict regions.
    ondemand = "ondemand"  # Try line merge first. Only use word merge if it can solve conflicts.
    disabled = "disabled"  # Do not ever attempt to do word merge.

    @classmethod
    def fromui(cls, ui):
        if ui.configbool("merge", "word-merge"):
            return cls.ondemand
        else:
            return cls.disabled


def trywordmerge(base_lines, a_lines, b_lines):
    """Try resolve conflicts using wordmerge.
    Return resolved lines, or None if merge failed.
    """
    basetext = b"".join(base_lines)
    atext = b"".join(a_lines)
    btext = b"".join(b_lines)
    try:
        m3 = Merge3Text(basetext, atext, btext, wordmerge=wordmergemode.enforced)
        text = b"".join(render_minimized(m3)[0])
        return text.splitlines(True)
    except CantShowWordConflicts:
        return None


def splitwordswithoutemptylines(text):
    """Run mdiff.splitwords. Then fold "\n" into the previous word.

    This makes "surrounding lines/words" more meaningful and avoids some
    aggressive merges where conflicts are more desirable.
    """
    words = mdiff.splitwords(text)
    buf = []
    result = []
    append = result.append
    bufappend = buf.append
    for word in words:
        if word != b"\n" and buf:
            append(b"".join(buf))
            buf.clear()
        bufappend(word)
    if buf:
        append(b"".join(buf))
    return result


class Merge3Text:
    """3-way merge of texts.

    Given strings BASE, OTHER, THIS, tries to produce a combined text
    incorporating the changes from both BASE->OTHER and BASE->THIS."""

    def __init__(self, basetext, atext, btext, wordmerge=wordmergemode.disabled):
        self.basetext = basetext
        self.atext = atext
        self.btext = btext
        if wordmerge is wordmergemode.enforced:
            split = splitwordswithoutemptylines
        else:
            split = mdiff.splitnewlines
        self.base = split(basetext)
        self.a = split(atext)
        self.b = split(btext)
        self.wordmerge = wordmerge
        self.automerge_fns = (
            [trywordmerge] if wordmerge == wordmergemode.ondemand else []
        )

    def merge_groups(self, automerge=False):
        """Yield sequence of line groups.

        Each one is a tuple:

        'unchanged', lines
             Lines unchanged from base

        'a', lines
             Lines taken from a

        'same', lines
             Lines taken from a (and equal to b)

        'b', lines
             Lines taken from b

        'conflict', (base_lines, a_lines, b_lines)
             Lines from base were changed to either a or b and conflict.

        'automerge', lines
             Lines produced by automerge algorithms to resolve conflict.
        """
        for t in self.merge_regions():
            what = t[0]
            if what == "unchanged":
                yield what, self.base[t[1] : t[2]]
            elif what == "a" or what == "same":
                yield what, self.a[t[1] : t[2]]
            elif what == "b":
                yield what, self.b[t[1] : t[2]]
            elif what == "conflict":
                if self.wordmerge is wordmergemode.enforced:
                    raise CantShowWordConflicts()

                base_lines = self.base[t[1] : t[2]]
                a_lines = self.a[t[3] : t[4]]
                b_lines = self.b[t[5] : t[6]]
                merged_lines = None
                if automerge:
                    for fn in self.automerge_fns:
                        merged_lines = fn(base_lines, a_lines, b_lines)
                        if merged_lines is not None:
                            yield "automerge", merged_lines
                            break
                if merged_lines is None:
                    yield (what, (base_lines, a_lines, b_lines))
            else:
                raise ValueError(what)

    def merge_regions(self):
        """Return sequences of matching and conflicting regions.

        This returns tuples, where the first value says what kind we
        have:

        'unchanged', start, end
             Take a region of base[start:end]

        'same', astart, aend
             b and a are different from base but give the same result

        'a', start, end
             Non-clashing insertion from a[start:end]

        'conflict', zstart, zend, astart, aend, bstart, bend
            Conflict between a and b, with z as common ancestor

        Method is as follows:

        The two sequences align only on regions which match the base
        and both descendants.  These are found by doing a two-way diff
        of each one against the base, and then finding the
        intersections between those regions.  These "sync regions"
        are by definition unchanged in both and easily dealt with.

        The regions in between can be in any of three cases:
        conflicted, or changed on only one side.
        """

        # section a[0:ia] has been disposed of, etc
        iz = ia = ib = 0

        for region in self.find_sync_regions():
            zmatch, zend, amatch, aend, bmatch, bend = region
            # print 'match base [%d:%d]' % (zmatch, zend)

            matchlen = zend - zmatch
            assert matchlen >= 0
            assert matchlen == (aend - amatch)
            assert matchlen == (bend - bmatch)

            len_a = amatch - ia
            len_b = bmatch - ib
            len_base = zmatch - iz
            assert len_a >= 0
            assert len_b >= 0
            assert len_base >= 0

            # print 'unmatched a=%d, b=%d' % (len_a, len_b)

            if len_a or len_b:
                # try to avoid actually slicing the lists
                equal_a = compare_range(self.a, ia, amatch, self.base, iz, zmatch)
                equal_b = compare_range(self.b, ib, bmatch, self.base, iz, zmatch)
                same = compare_range(self.a, ia, amatch, self.b, ib, bmatch)

                if same:
                    yield "same", ia, amatch
                elif equal_a and not equal_b:
                    yield "b", ib, bmatch
                elif equal_b and not equal_a:
                    yield "a", ia, amatch
                elif not equal_a and not equal_b:
                    yield "conflict", iz, zmatch, ia, amatch, ib, bmatch
                else:
                    raise AssertionError("can't handle a=b=base but unmatched")

                ia = amatch
                ib = bmatch
            iz = zmatch

            # if the same part of the base was deleted on both sides
            # that's OK, we can just skip it.

            if matchlen > 0:
                assert ia == amatch
                assert ib == bmatch
                assert iz == zmatch

                yield "unchanged", zmatch, zend
                iz = zend
                ia = aend
                ib = bend

    def find_sync_regions(self):
        """Return a list of sync regions, where both descendants match the base.

        Generates a list of (base1, base2, a1, a2, b1, b2).  There is
        always a zero-length sync region at the end of all the files.
        """

        ia = ib = 0
        if self.wordmerge is wordmergemode.enforced:

            def escape(word):
                # escape "word" so it looks like a line ending with "\n"
                return word.replace(b"\\", b"\\\\").replace(b"\n", b"\\n") + b"\n"

            def concat(words):
                # escape and concat words
                return b"".join(map(escape, words))

            basetext = concat(self.base)
            atext = concat(self.a)
            btext = concat(self.b)
        else:
            basetext = self.basetext
            atext = self.atext
            btext = self.btext

        amatches = mdiff.get_matching_blocks(basetext, atext)
        bmatches = mdiff.get_matching_blocks(basetext, btext)
        len_a = len(amatches)
        len_b = len(bmatches)

        sl = []

        while ia < len_a and ib < len_b:
            abase, amatch, alen = amatches[ia]
            bbase, bmatch, blen = bmatches[ib]

            # there is an unconflicted block at i; how long does it
            # extend?  until whichever one ends earlier.
            i = intersect((abase, abase + alen), (bbase, bbase + blen))
            if i:
                intbase = i[0]
                intend = i[1]
                intlen = intend - intbase

                # found a match of base[i[0], i[1]]; this may be less than
                # the region that matches in either one
                assert intlen <= alen
                assert intlen <= blen
                assert abase <= intbase
                assert bbase <= intbase

                asub = amatch + (intbase - abase)
                bsub = bmatch + (intbase - bbase)
                aend = asub + intlen
                bend = bsub + intlen

                assert self.base[intbase:intend] == self.a[asub:aend], (
                    self.base[intbase:intend],
                    self.a[asub:aend],
                )

                assert self.base[intbase:intend] == self.b[bsub:bend]

                sl.append((intbase, intend, asub, aend, bsub, bend))

            # advance whichever one ends first in the base text
            if (abase + alen) < (bbase + blen):
                ia += 1
            else:
                ib += 1

        intbase = len(self.base)
        abase = len(self.a)
        bbase = len(self.b)
        sl.append((intbase, intbase, abase, abase, bbase, bbase))

        return sl


def _minimize(a_lines, b_lines):
    """Trim conflict regions of lines where A and B sides match.

    Lines where both A and B have made the same changes at the beginning
    or the end of each merge region are eliminated from the conflict
    region and are instead considered the same.
    """
    alen = len(a_lines)
    blen = len(b_lines)

    # find matches at the front
    ii = 0
    while ii < alen and ii < blen and a_lines[ii] == b_lines[ii]:
        ii += 1
    startmatches = ii

    # find matches at the end
    ii = 0
    while ii < alen and ii < blen and a_lines[-ii - 1] == b_lines[-ii - 1]:
        ii += 1
    endmatches = ii

    lines_before = a_lines[:startmatches]
    new_a_lines = a_lines[startmatches : alen - endmatches]
    new_b_lines = b_lines[startmatches : blen - endmatches]
    lines_after = a_lines[alen - endmatches :]
    return lines_before, new_a_lines, new_b_lines, lines_after


def render_minimized(
    m3,
    name_a=None,
    name_b=None,
    start_marker=b"<<<<<<<",
    mid_marker=b"=======",
    end_marker=b">>>>>>>",
) -> Tuple[List[bytes], int]:
    """Return merge in cvs-like form."""
    conflictscount = 0
    newline = _detect_newline(m3)
    if name_a:
        start_marker = start_marker + b" " + name_a
    if name_b:
        end_marker = end_marker + b" " + name_b

    merge_groups = m3.merge_groups(automerge=True)
    lines = []
    for what, group_lines in merge_groups:
        if what == "conflict":
            conflictscount += 1
            base_lines, a_lines, b_lines = group_lines
            minimized = _minimize(a_lines, b_lines)
            lines_before, a_lines, b_lines, lines_after = minimized
            lines.extend(lines_before)
            lines.append(start_marker + newline)
            lines.extend(a_lines)
            lines.append(mid_marker + newline)
            lines.extend(b_lines)
            lines.append(end_marker + newline)
            lines.extend(lines_after)
        else:
            lines.extend(group_lines)

    return lines, conflictscount


def render_merge3(m3, name_a, name_b, name_base) -> Tuple[List[bytes], int]:
    """Return merge in cvs-like form."""
    conflictscount = 0
    newline = _detect_newline(m3)
    merge_groups = m3.merge_groups(automerge=True)
    lines = []

    for what, group_lines in merge_groups:
        if what == "conflict":
            conflictscount += 1
            base_lines, a_lines, b_lines = group_lines
            lines.append(b"<<<<<<< " + name_a + newline)
            lines.extend(a_lines)
            lines.append(b"||||||| " + name_base + newline)
            lines.extend(base_lines)
            lines.append(b"=======" + newline)
            lines.extend(b_lines)
            lines.append(b">>>>>>> " + name_b + newline)
        else:
            lines.extend(group_lines)

    return lines, conflictscount


def _detect_newline(m3):
    newline = b"\n"
    if len(m3.a) > 0:
        if m3.a[0].endswith(b"\r\n"):
            newline = b"\r\n"
        elif m3.a[0].endswith(b"\r"):
            newline = b"\r"
    return newline


def _verifytext(text, path, ui, opts):
    """verifies that text is non-binary (unless opts[text] is passed,
    then we just warn)"""
    if util.binary(text):
        msg = _("%s looks like a binary file.") % path
        if not opts.get("quiet"):
            ui.warn(_("warning: %s\n") % msg)
        if not opts.get("text"):
            raise error.Abort(msg)
    return text


def _picklabels(overrides):
    if len(overrides) > 3:
        raise error.Abort(_("can only specify three labels."))
    result = [None, None, None]
    for i, override in enumerate(overrides):
        result[i] = pycompat.encodeutf8(override)
    return result


def render_mergediff(m3, name_a, name_b, name_base):
    newline = _detect_newline(m3)
    lines = []
    conflictscount = 0
    for what, group_lines in m3.merge_groups(automerge=True):
        if what == "conflict":
            base_lines, a_lines, b_lines = group_lines
            basetext = b"".join(base_lines)
            bblocks = list(
                mdiff.allblocks(
                    basetext,
                    b"".join(b_lines),
                    lines1=base_lines,
                    lines2=b_lines,
                )
            )
            ablocks = list(
                mdiff.allblocks(
                    basetext,
                    b"".join(a_lines),
                    lines1=base_lines,
                    lines2=b_lines,
                )
            )

            def matchinglines(blocks):
                return sum(block[1] - block[0] for block, kind in blocks if kind == "=")

            def difflines(blocks, lines1, lines2):
                for block, kind in blocks:
                    if kind == "=":
                        for line in lines1[block[0] : block[1]]:
                            yield b" " + line
                    else:
                        for line in lines1[block[0] : block[1]]:
                            yield b"-" + line
                        for line in lines2[block[2] : block[3]]:
                            yield b"+" + line

            lines.append(b"<<<<<<<" + newline)
            if matchinglines(ablocks) < matchinglines(bblocks):
                lines.append(b"======= " + name_a + newline)
                lines.extend(a_lines)
                lines.append(b"------- " + name_base + newline)
                lines.append(b"+++++++ " + name_b + newline)
                lines.extend(difflines(bblocks, base_lines, b_lines))
            else:
                lines.append(b"------- " + name_base + newline)
                lines.append(b"+++++++ " + name_a + newline)
                lines.extend(difflines(ablocks, base_lines, a_lines))
                lines.append(b"======= " + name_b + newline)
                lines.extend(b_lines)
            lines.append(b">>>>>>>" + newline)
            conflictscount += 1
        else:
            lines.extend(group_lines)
    return lines, conflictscount


def _resolve(m3, sides):
    lines = []
    for what, group_lines in m3.merge_groups():
        if what == "conflict":
            for side in sides:
                lines.extend(group_lines[side])
        else:
            lines.extend(group_lines)
    return lines


def simplemerge(ui, localctx, basectx, otherctx, **opts):
    """Performs the simplemerge algorithm.

    The merged result is written into `localctx`.

    Returns the number of conflicts.
    """

    def readctx(ctx):
        # Merges were always run in the working copy before, which means
        # they used decoded data, if the user defined any repository
        # filters.
        #
        # Maintain that behavior today for BC, though perhaps in the future
        # it'd be worth considering whether merging encoded data (what the
        # repository usually sees) might be more useful.
        return _verifytext(ctx.data(), ctx.path(), ui, opts)

    mode = opts.get("mode", "merge")
    name_a, name_b, name_base = None, None, None
    if mode != "union":
        name_a, name_b, name_base = _picklabels(opts.get("label", []))

    try:
        localtext = readctx(localctx)
        basetext = readctx(basectx)
        othertext = readctx(otherctx)
    except error.Abort:
        return 1

    m3 = Merge3Text(basetext, localtext, othertext, wordmerge=wordmergemode.fromui(ui))

    conflictscount = 0
    if mode == "union":
        lines = _resolve(m3, (1, 2))
    elif mode == "local":
        lines = _resolve(m3, (1,))
    elif mode == "other":
        lines = _resolve(m3, (2,))
    elif mode == "mergediff":
        lines, conflictscount = render_mergediff(m3, name_a, name_b, name_base)
    elif mode == "merge3":
        lines, conflictscount = render_merge3(m3, name_a, name_b, name_base)
    else:
        lines, conflictscount = render_minimized(m3, name_a, name_b)

    mergedtext = b"".join(lines)
    if opts.get("print"):
        ui.fout.write(mergedtext)
    else:
        # HACK(phillco): We need to call ``workingflags()`` if ``localctx`` is
        # a workingfilectx (see workingfilectx.workingflags).
        flags = getattr(localctx, "workingflags", localctx.flags)()
        localctx.write(mergedtext, flags)

    return conflictscount
