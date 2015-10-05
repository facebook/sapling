# destutil.py - Mercurial utility function for command destination
#
#  Copyright Matt Mackall <mpm@selenic.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from .i18n import _
from . import (
    bookmarks,
    error,
    obsolete,
)

def destupdate(repo, clean=False, check=False):
    """destination for bare update operation

    return (rev, movemark, activemark)

    - rev: the revision to update to,
    - movemark: node to move the active bookmark from
                (cf bookmark.calculate update),
    - activemark: a bookmark to activate at the end of the update.
    """
    node = None
    wc = repo[None]
    p1 = wc.p1()
    activemark = None

    # we also move the active bookmark, if any
    node, movemark = bookmarks.calculateupdate(repo.ui, repo, None)
    if node is not None:
        activemark = node

    if node is None:
        try:
            node = repo.branchtip(wc.branch())
        except error.RepoLookupError:
            if wc.branch() == 'default': # no default branch!
                node = repo.lookup('tip') # update to tip
            else:
                raise error.Abort(_("branch %s not found") % wc.branch())

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
    rev = repo[node].rev()

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

    return rev, movemark, activemark
