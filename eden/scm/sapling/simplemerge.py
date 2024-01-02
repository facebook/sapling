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

import functools
import hashlib
from contextlib import contextmanager
from typing import List, Optional, Tuple

from bindings import clientinfo

from . import error, mdiff, pycompat, util
from .i18n import _
from .pycompat import range

_DEFAULT_CACHE_SIZE = 10000
_automerge_cache = util.lrucachedict(_DEFAULT_CACHE_SIZE)
_automerge_prompt_msg = _(
    "%(conflict)s\n"
    "Above conflict can be resolved automatically "
    "(see '@prog@ help automerge' for details):\n"
    "<<<<<<< automerge algorithm yields:\n"
    " %(merged_lines)s"
    ">>>>>>>\n"
    "Accept this resolution?\n"
    "(a)ccept it, (r)eject it, or review it in (f)ile:"
    "$$ &Accept $$ &Reject $$ &File"
)


class AutomergeSummary:
    def __init__(self):
        self.spans = []

    def add(self, start, length):
        self.spans.append((start, start + length))

    def summary(self) -> Optional[str]:
        def span_str(span):
            s, e = span
            return str(e) if s + 1 == e else f"{s+1}-{e}"

        if not self.spans:
            return None

        lines = ",".join(span_str(s) for s in self.spans)
        if "-" in lines:
            msg = _(" lines %s have been resolved by automerge algorithms\n") % (lines)
        else:
            msg = _(" line %s has been resolved by automerge algorithms\n") % (lines)
        return msg


class AutomergeMetrics:
    def __init__(self):
        # config
        self.mode = None
        self.merge_algos = None
        self.disable_for_noninteractive = None
        self.interactive = None
        self.repo = None

        # derived metrics from config
        self.enabled = 0

        # conflicts can be automerged
        self.total = 0
        self.accepted = 0
        self.rejected = 0
        self.review_in_file = 0

        # conflicts that can't be resolved by traditional merge algorithm
        self.conflicts = 0

        # rebase metrics
        self.duration = 0
        self.has_exception = 0
        self.local_commit = None
        self.base_commit = None
        self.other_commit = None
        self.base_filepath = None

        # command metrics
        self.command = None
        self.client_correlator = None

    @classmethod
    def init_from_ui(cls, ui, repo_name):
        obj = cls()
        obj.mode = ui.config("automerge", "mode")
        obj.merge_algos = ui.config("automerge", "merge-algos")
        obj.disable_for_noninteractive = ui.config(
            "automerge", "disable-for-noninteractive"
        )
        obj.interactive = ui.interactive()
        obj.repo = repo_name

        obj.command = ui.cmdname
        obj.client_correlator = clientinfo.get_client_request_info()["correlator"]
        return obj

    def to_dict(self):
        metrics = {}
        for key, value in self.__dict__.items():
            if value is not None:
                key = f"automerge_{key}"
                metrics[key] = value
        return metrics

    def set_commits(self, localctx, basectx, otherctx):
        def get_hex(fctx):
            ctx = fctx.changectx()
            return ctx.hex() if ctx.node() else ctx.p1().hex()

        self.local_commit = get_hex(localctx)
        self.base_commit = get_hex(basectx)
        self.other_commit = get_hex(otherctx)
        self.base_filepath = basectx.path()


_automerge_metrics = AutomergeMetrics()


@contextmanager
def managed_merge_resource(ui, repo_name):
    global _automerge_cache
    global _automerge_metrics

    _automerge_metrics = AutomergeMetrics.init_from_ui(ui, repo_name)
    start_time_ms = int(util.timer() * 1000)
    try:
        yield
    except Exception:
        _automerge_metrics.has_exception = 1
        raise
    finally:
        # clear cache when exiting
        size = ui.configint("automerge", "cache-size", _DEFAULT_CACHE_SIZE)
        _automerge_cache = util.lrucachedict(size)

        # log metrics
        end_time_ms = int(util.timer() * 1000)
        _automerge_metrics.duration = end_time_ms - start_time_ms

        metrics = _automerge_metrics.to_dict()
        ui.log("merge_conflicts", **metrics)


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


### automerge algorithms


class CantShowWordConflicts(Exception):
    pass


def automerge_wordmerge(base_lines, a_lines, b_lines) -> Optional[List[bytes]]:
    """Try resolve conflicts using wordmerge.
    Return resolved lines, or None if merge failed.
    """
    basetext = b"".join(base_lines)
    atext = b"".join(a_lines)
    btext = b"".join(b_lines)
    try:
        m3 = Merge3Text(basetext, atext, btext, in_wordmerge=True)
        text = b"".join(render_minimized(m3)[0])
        return text.splitlines(True)
    except CantShowWordConflicts:
        return None


def automerge_adjacent_changes(base_lines, a_lines, b_lines) -> Optional[List[bytes]]:
    # require something to be changed
    if not base_lines:
        return None

    ablocks = unmatching_blocks(base_lines, a_lines)
    bblocks = unmatching_blocks(base_lines, b_lines)

    k = 0
    indexes = [0, 0]
    merged_lines = []
    A, B, BASE = range(3)
    while indexes[A] < len(ablocks) and indexes[B] < len(bblocks):
        ablock, bblock = ablocks[indexes[A]], bblocks[indexes[B]]
        if is_overlap(ablock[0], ablock[1], bblock[0], bblock[1]):
            return None
        elif is_non_unique_separator_for_insertion(
            base_lines, a_lines, b_lines, ablock, bblock
        ):
            return None

        i, block, lines = (
            (A, ablock, a_lines) if ablock[0] < bblock[0] else (B, bblock, b_lines)
        )
        # add base lines before the block
        while k < block[0]:
            merged_lines.append(base_lines[k])
            k += 1
        # skip base lines being deleted
        k += block[1] - block[0]
        # add new lines from the block
        merged_lines.extend(lines[block[2] : block[3]])

        indexes[i] += 1

    if indexes[A] < len(ablocks):
        blocks, index, lines = ablocks, indexes[A], a_lines
    else:
        blocks, index, lines = bblocks, indexes[B], b_lines

    while index < len(blocks):
        block = blocks[index]
        index += 1

        while k < block[0]:
            merged_lines.append(base_lines[k])
            k += 1
        k += block[1] - block[0]
        merged_lines.extend(lines[block[2] : block[3]])

    # add base lines at the end of block
    merged_lines.extend(base_lines[k:])

    return merged_lines


def automerge_subset_changes(base_lines, a_lines, b_lines) -> Optional[List[bytes]]:
    if base_lines:
        return None
    if len(a_lines) > len(b_lines):
        return automerge_subset_changes(base_lines, b_lines, a_lines)
    if is_sub_list(a_lines, b_lines):
        return b_lines


def is_non_unique_separator_for_insertion(
    base_lines, a_lines, b_lines, ablock, bblock
) -> bool:
    # no insertion on both sides
    if not (ablock[0] == bblock[0] or ablock[1] == bblock[1]):
        return False

    if ablock[1] <= bblock[0]:
        base_start, base_end = ablock[1], bblock[0]
    else:
        base_start, base_end = bblock[1], ablock[0]

    if base_start <= base_end:
        # empty is a subset of any list
        return True

    base_list = base_lines[base_start:base_end]
    a_list = a_lines[ablock[3] : ablock[4]]
    b_list = b_lines[bblock[3] : bblock[4]]
    return is_sub_list(base_list, a_list) or is_sub_list(base_list, b_list)


def is_sub_list(list1, list2):
    "check if list1 is a sublist of list2"
    # PERF: might be able to use rolling hash to optimize the time complexity
    len1, len2 = len(list1), len(list2)
    if len1 > len2:
        return False
    for i in range(len2 - len1 + 1):
        if list1 == list2[i : i + len1]:
            return True
    return False


def unmatching_blocks(lines1, lines2):
    text1 = b"".join(lines1)
    text2 = b"".join(lines2)
    blocks = mdiff.allblocks(text1, text2, lines1=lines1, lines2=lines2)
    return [b[0] for b in blocks if b[1] == "!"]


def is_overlap(s1, e1, s2, e2):
    return not (s1 >= e2 or s2 >= e1)


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


AUTOMERGE_ALGORITHMS = {
    "word-merge": automerge_wordmerge,
    "adjacent-changes": automerge_adjacent_changes,
    "subset-changes": automerge_subset_changes,
}


class Merge3Text:
    """3-way merge of texts.

    Given strings BASE, OTHER, THIS, tries to produce a combined text
    incorporating the changes from both BASE->OTHER and BASE->THIS."""

    def __init__(
        self, basetext, atext, btext, ui=None, in_wordmerge=False, premerge=False
    ):
        self.in_wordmerge = in_wordmerge

        # ui is used for (1) getting automerge configs; (2) prompt choices
        self.ui = ui
        self.automerge_fns = {}
        self.automerge_mode = ""
        self.init_automerge_fields(ui)
        self.premerge = premerge

        if in_wordmerge and self.automerge_fns:
            raise error.Abort(
                _("word-level merge does not support automerge algorithms")
            )

        self.basetext = basetext
        self.atext = atext
        self.btext = btext

        split = (
            splitwordswithoutemptylines if self.in_wordmerge else mdiff.splitnewlines
        )
        self.base = split(basetext)
        self.a = split(atext)
        self.b = split(btext)

    def init_automerge_fields(self, ui):
        if not ui:
            return
        self.automerge_mode = ui.config("automerge", "mode") or "reject"
        automerge_fns = self.automerge_fns
        automerge_algos = ui.configlist("automerge", "merge-algos")
        for name in automerge_algos:
            try:
                automerge_fns[name] = AUTOMERGE_ALGORITHMS[name]
            except KeyError:
                raise error.Abort(
                    _("unknown automerge algorithm '%s', availabe algorithms are %s")
                    % (name, list(AUTOMERGE_ALGORITHMS.keys()))
                )

    def merge_groups(self):
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
                if self.in_wordmerge:
                    raise CantShowWordConflicts()

                base_lines = self.base[t[1] : t[2]]
                a_lines = self.a[t[3] : t[4]]
                b_lines = self.b[t[5] : t[6]]
                yield (what, (base_lines, a_lines, b_lines))
            else:
                raise ValueError(what)

    def run_automerge(self, base_lines, a_lines, b_lines):
        for name, fn in self.automerge_fns.items():
            merged_lines = fn(base_lines, a_lines, b_lines)
            if merged_lines is not None:
                return name, merged_lines
        return None

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
        if self.in_wordmerge:

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


def try_automerge_conflict(
    m3, group_lines, name_base, name_a, name_b, render_conflict_fn, newline=b"\n"
):
    def automerge_cache_key(conflict_group_lines):
        m = hashlib.sha256()
        for lines in conflict_group_lines:
            for line in lines:
                m.update(line)
        return m.digest()

    def render_automerged_lines(merge_algorithm, merged_lines, newline):
        lines = []
        lines.append(
            (b"<<<<<<< '%s' automerge algorithm yields:" % merge_algorithm.encode())
            + newline
        )
        lines.extend(merged_lines)
        lines.append(b">>>>>>>" + newline)
        return lines

    def automerge_enabled(ui, automerge_mode):
        if not ui or automerge_mode == "reject":
            return False

        if ui.configbool("automerge", "disable-for-noninteractive", True):
            return ui.interactive()

        return True

    automerge_mode = m3.automerge_mode
    ui = m3.ui

    base_lines, a_lines, b_lines = group_lines
    extra_lines = []

    merged_res = m3.run_automerge(base_lines, a_lines, b_lines)
    is_enabled = automerge_enabled(ui, automerge_mode)

    _automerge_metrics.conflicts += 1
    _automerge_metrics.enabled = int(is_enabled)
    _automerge_metrics.total += bool(merged_res)

    if is_enabled and merged_res:
        merge_algorithm, merged_lines = merged_res
        if automerge_mode == "accept":
            _automerge_metrics.accepted += 1
            return merge_algorithm, merged_lines
        elif automerge_mode == "prompt":
            cache_key = automerge_cache_key(group_lines)
            if cache_key not in _automerge_cache:
                prompt = {
                    "conflict": b"".join(
                        _render_diff_conflict(
                            base_lines,
                            a_lines,
                            b_lines,
                            name_base,
                            name_a,
                            name_b,
                            newline=newline,
                            one_side=False,
                        )
                    ).decode(),
                    "merged_lines": b" ".join(merged_lines).decode(),
                    "merge_algorithm": merge_algorithm,
                }
                index = ui.promptchoice(_automerge_prompt_msg % prompt, 1)
                _automerge_cache[cache_key] = index
            index = _automerge_cache[cache_key]
            if index == 0:  # accept
                _automerge_metrics.accepted += 1
                return merge_algorithm, merged_lines
            elif index == 2:  # review-in-file
                _automerge_metrics.review_in_file += 1
                extra_lines.extend(
                    render_automerged_lines(merge_algorithm, merged_lines, newline)
                )
            else:
                _automerge_metrics.rejected += 1
        elif automerge_mode == "review-in-file":
            _automerge_metrics.review_in_file += 1
            extra_lines.extend(
                render_automerged_lines(merge_algorithm, merged_lines, newline)
            )
        else:
            _automerge_metrics.rejected += 1

    lines = render_conflict_fn(base_lines, a_lines, b_lines)
    lines.extend(extra_lines)
    return None, lines


def render_minimized(
    m3,
    name_a=None,
    name_b=None,
    name_base=None,
    start_marker=b"<<<<<<<",
    mid_marker=b"=======",
    end_marker=b">>>>>>>",
) -> Tuple[List[bytes], int]:
    """Return merge in cvs-like form."""

    def render_minimized_conflict(base_lines, a_lines, b_lines):
        lines = []
        minimized = _minimize(a_lines, b_lines)
        lines_before, a_lines, b_lines, lines_after = minimized
        lines.extend(lines_before)
        lines.append(start_marker + newline)
        lines.extend(a_lines)
        lines.append(mid_marker + newline)
        lines.extend(b_lines)
        lines.append(end_marker + newline)
        lines.extend(lines_after)
        return lines

    newline = _detect_newline(m3)
    if name_a:
        start_marker = start_marker + b" " + name_a
    if name_b:
        end_marker = end_marker + b" " + name_b

    return _apply_conflict_render(
        m3, name_a, name_b, name_base, render_minimized_conflict, newline
    )


def render_merge3(m3, name_a, name_b, name_base) -> Tuple[List[bytes], int]:
    """Return merge in cvs-like form."""

    def render_merge3_conflict(base_lines, a_lines, b_lines):
        lines = []
        lines.append(b"<<<<<<< " + name_a + newline)
        lines.extend(a_lines)
        lines.append(b"||||||| " + name_base + newline)
        lines.extend(base_lines)
        lines.append(b"=======" + newline)
        lines.extend(b_lines)
        lines.append(b">>>>>>> " + name_b + newline)
        return lines

    newline = _detect_newline(m3)
    return _apply_conflict_render(
        m3, name_a, name_b, name_base, render_merge3_conflict, newline
    )


def _apply_conflict_render(m3, name_a, name_b, name_base, render_fn, newline):
    conflictscount = 0
    lines = []
    automerge_summary = AutomergeSummary()

    for what, group_lines in m3.merge_groups():
        if what == "conflict":
            automerge_algo, merged_lines = try_automerge_conflict(
                m3,
                group_lines,
                name_base,
                name_a,
                name_b,
                render_fn,
                newline,
            )
            if automerge_algo:
                automerge_summary.add(len(lines), len(merged_lines))
            else:
                conflictscount += 1
            lines.extend(merged_lines)
        else:
            lines.extend(group_lines)

    # to avoid printing duplicate messages in both `premerge` and `merge``, we skip
    # the logic when it's in `premerge`` and there are conflicts, as it will
    # call `merge` later
    if not (m3.premerge and conflictscount) and (
        automerge_msg := automerge_summary.summary()
    ):
        m3.ui.status(automerge_msg)

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
    return _render_mergediff_ext(m3, name_a, name_b, name_base, one_side=True)


def render_mergediff2(m3, name_a, name_b, name_base):
    return _render_mergediff_ext(m3, name_a, name_b, name_base, one_side=False)


def _render_mergediff_ext(m3, name_a, name_b, name_base, one_side):
    newline = _detect_newline(m3)
    render_conflict_fn = functools.partial(
        _render_diff_conflict,
        name_base=name_base,
        name_a=name_a,
        name_b=name_b,
        newline=newline,
        one_side=one_side,
    )
    return _apply_conflict_render(
        m3, name_a, name_b, name_base, render_conflict_fn, newline
    )


def _render_diff_conflict(
    base_lines,
    a_lines,
    b_lines,
    name_base=b"",
    name_a=b"",
    name_b=b"",
    newline=b"\n",
    one_side=True,  # diff on one side of the conflict, other diff on both sides
):
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

    lines = []
    if one_side:
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
    else:
        lines.append(b"<<<<<<< " + name_a + newline)
        lines.extend(difflines(ablocks, base_lines, a_lines))
        lines.append(b"======= " + name_base + newline)
        lines.extend(difflines(bblocks, base_lines, b_lines))
        lines.append(b">>>>>>> " + name_b + newline)
    return lines


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

    _automerge_metrics.set_commits(localctx, basectx, otherctx)

    premerge = opts.get("premerge", False)
    m3 = Merge3Text(basetext, localtext, othertext, ui=ui, premerge=premerge)

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
        lines, conflictscount = render_minimized(m3, name_a, name_b, name_base)

    mergedtext = b"".join(lines)
    if opts.get("print"):
        ui.fout.write(mergedtext)
    else:
        # HACK(phillco): We need to call ``workingflags()`` if ``localctx`` is
        # a workingfilectx (see workingfilectx.workingflags).
        flags = getattr(localctx, "workingflags", localctx.flags)()
        localctx.write(mergedtext, flags)

    return conflictscount
