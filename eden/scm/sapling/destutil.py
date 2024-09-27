# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# destutil.py - Mercurial utility function for command destination
#
#  Copyright Olivia Mackall <olivia@selenic.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from typing import Dict, Optional, Tuple, Union

from . import bookmarks, error, scmutil
from .i18n import _


msgdestmerge: Dict[
    str,
    Union[
        Tuple[str, None],
        Tuple[str, str],
        Tuple[str, Optional[str]],
    ],
] = {
    # too many matching divergent bookmark
    "toomanybookmarks": (
        _(
            "multiple matching bookmarks to merge -"
            " please merge with an explicit rev or bookmark"
        ),
        _("run '@prog@ heads' to see all heads"),
    ),
    # no other matching divergent bookmark
    "nootherbookmarks": (
        _(
            "no matching bookmark to merge - "
            "please merge with an explicit rev or bookmark"
        ),
        _("run '@prog@ heads' to see all heads"),
    ),
    # repo has too many unbookmarked heads, no obvious destination
    "toomanyheads": (
        _("repo has %d heads - please merge with an explicit rev"),
        _("run '@prog@ heads .' to see heads"),
    ),
    # repo has no other unbookmarked heads
    "bookmarkedheads": (
        _("heads are bookmarked - please merge with an explicit rev"),
        _("run '@prog@ heads' to see all heads"),
    ),
    # repo has just a single head, but there is other branches
    "nootherbranchheads": (
        _("repo has one head - please merge with an explicit rev"),
        _("run '@prog@ heads' to see all heads"),
    ),
    # repository have a single head
    "nootherheads": (_("nothing to merge"), None),
    # repository have a single head and we are not on it
    "nootherheadsbehind": (_("nothing to merge"), _("use '@prog@ goto' instead")),
    # We are not on a head
    "notatheads": (
        _("working directory not at a head revision"),
        _("use '@prog@ goto' or merge with an explicit revision"),
    ),
}


def _destmergebook(repo):
    """find merge destination in the active bookmark case"""
    node = None
    bmheads = bookmarks.headsforactive(repo)
    curhead = repo[repo._activebookmark].node()
    if len(bmheads) == 2:
        if curhead == bmheads[0]:
            node = bmheads[1]
        else:
            node = bmheads[0]
    elif len(bmheads) > 2:
        msg, hint = msgdestmerge["toomanybookmarks"]
        raise error.ManyMergeDestAbort(msg, hint=hint)
    elif len(bmheads) <= 1:
        msg, hint = msgdestmerge["nootherbookmarks"]
        raise error.NoMergeDestAbort(msg, hint=hint)
    assert node is not None
    return node


def _destmergeheads(repo):
    """find merge destination based on repo heads"""
    node = None

    sourceset = [repo[repo.dirstate.p1()].rev()]

    bheads = repo.headrevs()
    onhead = repo.revs("%ld and %ld", sourceset, bheads)
    if not onhead:
        # Case A: working copy if not on a head. (merge only)
        #
        # This is probably a user mistake We bailout pointing at 'hg update'
        if len(repo.heads()) <= 1:
            msg, hint = msgdestmerge["nootherheadsbehind"]
        else:
            msg, hint = msgdestmerge["notatheads"]
        raise error.Abort(msg, hint=hint)
    # remove heads descendants of source from the set
    bheads = list(repo.revs("%ld - (%ld::)", bheads, sourceset))
    # filters out bookmarked heads
    nbhs = list(repo.revs("%ld - bookmark()", bheads))

    if len(nbhs) > 1:
        # Case B: There is more than 1 other anonymous heads
        #
        # This means that there will be more than 1 candidate. This is
        # ambiguous. We abort asking the user to pick as explicit destination
        # instead.
        msg, hint = msgdestmerge["toomanyheads"]
        msg %= len(bheads) + 1
        raise error.ManyMergeDestAbort(msg, hint=hint)
    elif not nbhs:
        # Case B: There is no other anonymous heads
        #
        # This means that there is no natural candidate to merge with.
        # We abort, with various messages for various cases.
        if bheads:
            msg, hint = msgdestmerge["bookmarkedheads"]
        elif len(repo.heads()) > 1:
            msg, hint = msgdestmerge["nootherbranchheads"]
        elif not onhead:
            # if 'onheadcheck == False' (rebase case),
            # this was not caught in Case A.
            msg, hint = msgdestmerge["nootherheadsbehind"]
        else:
            msg, hint = msgdestmerge["nootherheads"]
        raise error.NoMergeDestAbort(msg, hint=hint)
    else:
        node = nbhs[0]
    assert node is not None
    return node


def destmerge(repo):
    """return the default destination for a merge

    (or raise exception about why it can't pick one)
    """
    # destspace is here to work around issues with `hg pull --rebase` see
    # issue5214 for details
    if repo._activebookmark:
        node = _destmergebook(repo)
    else:
        node = _destmergeheads(
            repo,
        )
    return repo[node].rev()


histeditdefaultrevset = "reverse(only(.) - public() - ::merge() - null)"


def desthistedit(ui, repo):
    """Default base revision to edit for `@prog@ histedit`."""
    default = ui.config("histedit", "defaultrev", histeditdefaultrevset)
    if default:
        revs = scmutil.revrange(repo, [default])
        if revs:
            # The revset supplied by the user may not be in ascending order nor
            # take the first revision. So do this manually.
            revs.sort()
            return revs.first()

    return None
