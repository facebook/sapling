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
        Dict[str, Tuple[str, None]],
        Dict[str, Tuple[str, str]],
        Dict[str, Tuple[str, Optional[str]]],
    ],
] = {
    # too many matching divergent bookmark
    "toomanybookmarks": {
        "merge": (
            _(
                "multiple matching bookmarks to merge -"
                " please merge with an explicit rev or bookmark"
            ),
            _("run '@prog@ heads' to see all heads"),
        ),
        "rebase": (
            _(
                "multiple matching bookmarks to rebase -"
                " please rebase to an explicit rev or bookmark"
            ),
            _("run '@prog@ heads' to see all heads"),
        ),
    },
    # no other matching divergent bookmark
    "nootherbookmarks": {
        "merge": (
            _(
                "no matching bookmark to merge - "
                "please merge with an explicit rev or bookmark"
            ),
            _("run '@prog@ heads' to see all heads"),
        ),
        "rebase": (
            _(
                "no matching bookmark to rebase - "
                "please rebase to an explicit rev or bookmark"
            ),
            _("run '@prog@ heads' to see all heads"),
        ),
    },
    # branch have too many unbookmarked heads, no obvious destination
    "toomanyheads": {
        "merge": (
            _("branch '%s' has %d heads - please merge with an explicit rev"),
            _("run '@prog@ heads .' to see heads"),
        ),
        "rebase": (
            _("branch '%s' has %d heads - please rebase to an explicit rev"),
            _("run '@prog@ heads .' to see heads"),
        ),
    },
    # branch have no other unbookmarked heads
    "bookmarkedheads": {
        "merge": (
            _("heads are bookmarked - please merge with an explicit rev"),
            _("run '@prog@ heads' to see all heads"),
        ),
        "rebase": (
            _("heads are bookmarked - please rebase to an explicit rev"),
            _("run '@prog@ heads' to see all heads"),
        ),
    },
    # branch have just a single heads, but there is other branches
    "nootherbranchheads": {
        "merge": (
            _("branch '%s' has one head - please merge with an explicit rev"),
            _("run '@prog@ heads' to see all heads"),
        ),
        "rebase": (
            _("branch '%s' has one head - please rebase to an explicit rev"),
            _("run '@prog@ heads' to see all heads"),
        ),
    },
    # repository have a single head
    "nootherheads": {
        "merge": (_("nothing to merge"), None),
        "rebase": (_("nothing to rebase"), None),
    },
    # repository have a single head and we are not on it
    "nootherheadsbehind": {
        "merge": (_("nothing to merge"), _("use '@prog@ goto' instead")),
        "rebase": (_("nothing to rebase"), _("use '@prog@ goto' instead")),
    },
    # We are not on a head
    "notatheads": {
        "merge": (
            _("working directory not at a head revision"),
            _("use '@prog@ goto' or merge with an explicit revision"),
        ),
        "rebase": (
            _("working directory not at a head revision"),
            _("use '@prog@ goto' or rebase to an explicit revision"),
        ),
    },
    "emptysourceset": {
        "merge": (_("source set is empty"), None),
        "rebase": (_("source set is empty"), None),
    },
    "multiplebranchessourceset": {
        "merge": (_("source set is rooted in multiple branches"), None),
        "rebase": (
            _("rebaseset is rooted in multiple named branches"),
            _("specify an explicit destination with --dest"),
        ),
    },
}


def _destmergebook(repo, action: str = "merge", sourceset=None, destspace=None):
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
        msg, hint = msgdestmerge["toomanybookmarks"][action]
        raise error.ManyMergeDestAbort(msg, hint=hint)
    elif len(bmheads) <= 1:
        msg, hint = msgdestmerge["nootherbookmarks"][action]
        raise error.NoMergeDestAbort(msg, hint=hint)
    assert node is not None
    return node


def _destmergebranch(
    repo,
    action: str = "merge",
    sourceset=None,
    onheadcheck: bool = True,
    destspace=None,
):
    """find merge destination based on branch heads"""
    node = None

    if sourceset is None:
        sourceset = [repo[repo.dirstate.p1()].rev()]
        branch = repo.dirstate.branch()
    elif not sourceset:
        msg, hint = msgdestmerge["emptysourceset"][action]
        raise error.NoMergeDestAbort(msg, hint=hint)
    else:
        branch = None
        for ctx in repo.set("roots(%ld::%ld)", sourceset, sourceset):
            if branch is not None and ctx.branch() != branch:
                msg, hint = msgdestmerge["multiplebranchessourceset"][action]
                raise error.ManyMergeDestAbort(msg, hint=hint)
            branch = ctx.branch()

    bheads = repo.branchheads(branch)
    onhead = repo.revs("%ld and %ln", sourceset, bheads)
    if onheadcheck and not onhead:
        # Case A: working copy if not on a head. (merge only)
        #
        # This is probably a user mistake We bailout pointing at 'hg update'
        if len(repo.heads()) <= 1:
            msg, hint = msgdestmerge["nootherheadsbehind"][action]
        else:
            msg, hint = msgdestmerge["notatheads"][action]
        raise error.Abort(msg, hint=hint)
    # remove heads descendants of source from the set
    bheads = list(repo.revs("%ln - (%ld::)", bheads, sourceset))
    # filters out bookmarked heads
    nbhs = list(repo.revs("%ld - bookmark()", bheads))

    if destspace is not None:
        # restrict search space
        # used in the 'hg pull --rebase' case, see issue 5214.
        nbhs = list(repo.revs("%ld and %ld", destspace, nbhs))

    if len(nbhs) > 1:
        # Case B: There is more than 1 other anonymous heads
        #
        # This means that there will be more than 1 candidate. This is
        # ambiguous. We abort asking the user to pick as explicit destination
        # instead.
        msg, hint = msgdestmerge["toomanyheads"][action]
        msg %= (branch, len(bheads) + 1)
        raise error.ManyMergeDestAbort(msg, hint=hint)
    elif not nbhs:
        # Case B: There is no other anonymous heads
        #
        # This means that there is no natural candidate to merge with.
        # We abort, with various messages for various cases.
        if bheads:
            msg, hint = msgdestmerge["bookmarkedheads"][action]
        elif len(repo.heads()) > 1:
            msg, hint = msgdestmerge["nootherbranchheads"][action]
            msg %= branch
        elif not onhead:
            # if 'onheadcheck == False' (rebase case),
            # this was not caught in Case A.
            msg, hint = msgdestmerge["nootherheadsbehind"][action]
        else:
            msg, hint = msgdestmerge["nootherheads"][action]
        raise error.NoMergeDestAbort(msg, hint=hint)
    else:
        node = nbhs[0]
    assert node is not None
    return node


def destmerge(
    repo,
    action: str = "merge",
    sourceset=None,
    onheadcheck: bool = True,
    destspace=None,
):
    """return the default destination for a merge

    (or raise exception about why it can't pick one)

    :action: the action being performed, controls emitted error message
    """
    # destspace is here to work around issues with `hg pull --rebase` see
    # issue5214 for details
    if repo._activebookmark:
        node = _destmergebook(
            repo, action=action, sourceset=sourceset, destspace=destspace
        )
    else:
        node = _destmergebranch(
            repo,
            action=action,
            sourceset=sourceset,
            onheadcheck=onheadcheck,
            destspace=destspace,
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
