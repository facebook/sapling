# destutil.py - Mercurial utility function for command destination
#
#  Copyright Matt Mackall <mpm@selenic.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .i18n import _
from . import (
    bookmarks,
    error,
    obsolete,
)

def _destupdatevalidate(repo, rev, clean, check):
    """validate that the destination comply to various rules

    This exists as its own function to help wrapping from extensions."""
    wc = repo[None]
    p1 = wc.p1()
    if not clean:
        # Check that the update is linear.
        #
        # Mercurial do not allow update-merge for non linear pattern
        # (that would be technically possible but was considered too confusing
        # for user a long time ago)
        #
        # See mercurial.merge.update for details
        if p1.rev() not in repo.changelog.ancestors([rev], inclusive=True):
            dirty = wc.dirty(missing=True)
            foreground = obsolete.foreground(repo, [p1.node()])
            if not repo[rev].node() in foreground:
                if dirty:
                    msg = _("uncommitted changes")
                    hint = _("commit and merge, or update --clean to"
                             " discard changes")
                    raise error.UpdateAbort(msg, hint=hint)
                elif not check:  # destination is not a descendant.
                    msg = _("not a linear update")
                    hint = _("merge or update --check to force update")
                    raise error.UpdateAbort(msg, hint=hint)

def _destupdateobs(repo, clean, check):
    """decide of an update destination from obsolescence markers"""
    node = None
    wc = repo[None]
    p1 = wc.p1()
    movemark = None

    if p1.obsolete() and not p1.children():
        # allow updating to successors
        successors = obsolete.successorssets(repo, p1.node())

        # behavior of certain cases is as follows,
        #
        # divergent changesets: update to highest rev, similar to what
        #     is currently done when there are more than one head
        #     (i.e. 'tip')
        #
        # replaced changesets: same as divergent except we know there
        # is no conflict
        #
        # pruned changeset: no update is done; though, we could
        #     consider updating to the first non-obsolete parent,
        #     similar to what is current done for 'hg prune'

        if successors:
            # flatten the list here handles both divergent (len > 1)
            # and the usual case (len = 1)
            successors = [n for sub in successors for n in sub]

            # get the max revision for the given successors set,
            # i.e. the 'tip' of a set
            node = repo.revs('max(%ln)', successors).first()
            if bookmarks.isactivewdirparent(repo):
                movemark = repo['.'].node()
    return node, movemark, None

def _destupdatebook(repo, clean, check):
    """decide on an update destination from active bookmark"""
    # we also move the active bookmark, if any
    activemark = None
    node, movemark = bookmarks.calculateupdate(repo.ui, repo, None)
    if node is not None:
        activemark = node
    return node, movemark, activemark

def _destupdatebranch(repo, clean, check):
    """decide on an update destination from current branch"""
    wc = repo[None]
    movemark = node = None
    try:
        node = repo.branchtip(wc.branch())
        if bookmarks.isactivewdirparent(repo):
            movemark = repo['.'].node()
    except error.RepoLookupError:
        if wc.branch() == 'default': # no default branch!
            node = repo.lookup('tip') # update to tip
        else:
            raise error.Abort(_("branch %s not found") % wc.branch())
    return node, movemark, None

# order in which each step should be evalutated
# steps are run until one finds a destination
destupdatesteps = ['evolution', 'bookmark', 'branch']
# mapping to ease extension overriding steps.
destupdatestepmap = {'evolution': _destupdateobs,
                     'bookmark': _destupdatebook,
                     'branch': _destupdatebranch,
                     }

def destupdate(repo, clean=False, check=False):
    """destination for bare update operation

    return (rev, movemark, activemark)

    - rev: the revision to update to,
    - movemark: node to move the active bookmark from
                (cf bookmark.calculate update),
    - activemark: a bookmark to activate at the end of the update.
    """
    node = movemark = activemark = None

    for step in destupdatesteps:
        node, movemark, activemark = destupdatestepmap[step](repo, clean, check)
        if node is not None:
            break
    rev = repo[node].rev()

    _destupdatevalidate(repo, rev, clean, check)

    return rev, movemark, activemark

def _destmergebook(repo):
    """find merge destination in the active bookmark case"""
    node = None
    bmheads = repo.bookmarkheads(repo._activebookmark)
    curhead = repo[repo._activebookmark].node()
    if len(bmheads) == 2:
        if curhead == bmheads[0]:
            node = bmheads[1]
        else:
            node = bmheads[0]
    elif len(bmheads) > 2:
        raise error.Abort(_("multiple matching bookmarks to merge - "
            "please merge with an explicit rev or bookmark"),
            hint=_("run 'hg heads' to see all heads"))
    elif len(bmheads) <= 1:
        raise error.Abort(_("no matching bookmark to merge - "
            "please merge with an explicit rev or bookmark"),
            hint=_("run 'hg heads' to see all heads"))
    assert node is not None
    return node

def _destmergebranch(repo):
    """find merge destination based on branch heads"""
    node = None
    branch = repo[None].branch()
    bheads = repo.branchheads(branch)
    nbhs = [bh for bh in bheads if not repo[bh].bookmarks()]

    if len(nbhs) > 2:
        raise error.Abort(_("branch '%s' has %d heads - "
                           "please merge with an explicit rev")
                         % (branch, len(bheads)),
                         hint=_("run 'hg heads .' to see heads"))

    parent = repo.dirstate.p1()
    if len(nbhs) <= 1:
        if len(bheads) > 1:
            raise error.Abort(_("heads are bookmarked - "
                               "please merge with an explicit rev"),
                             hint=_("run 'hg heads' to see all heads"))
        if len(repo.heads()) > 1:
            raise error.Abort(_("branch '%s' has one head - "
                               "please merge with an explicit rev")
                             % branch,
                             hint=_("run 'hg heads' to see all heads"))
        msg, hint = _('nothing to merge'), None
        if parent != repo.lookup(branch):
            hint = _("use 'hg update' instead")
        raise error.Abort(msg, hint=hint)

    if parent not in bheads:
        raise error.Abort(_('working directory not at a head revision'),
                         hint=_("use 'hg update' or merge with an "
                                "explicit revision"))
    if parent == nbhs[0]:
        node = nbhs[-1]
    else:
        node = nbhs[0]
    assert node is not None
    return node

def destmerge(repo):
    if repo._activebookmark:
        node = _destmergebook(repo)
    else:
        node = _destmergebranch(repo)
    return repo[node].rev()

histeditdefaultrevset = 'reverse(only(.) and not public() and not ::merge())'

def desthistedit(ui, repo):
    """Default base revision to edit for `hg histedit`."""
    # Avoid cycle: scmutil -> revset -> destutil
    from . import scmutil

    default = ui.config('histedit', 'defaultrev', histeditdefaultrevset)
    if default:
        revs = scmutil.revrange(repo, [default])
        if revs:
            # The revset supplied by the user may not be in ascending order nor
            # take the first revision. So do this manually.
            revs.sort()
            return revs.first()

    return None
