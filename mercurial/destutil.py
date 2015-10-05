# destutil.py - Mercurial utility function for command destination
#
#  Copyright Matt Mackall <mpm@selenic.com> and other
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from .i18n import _
from . import (
    error,
    obsolete,
)

def destupdate(repo, clean=False):
    """destination for bare update operation
    """
    # Here is where we should consider bookmarks, divergent bookmarks, and tip
    # of current branch; but currently we are only checking the branch tips.
    node = None
    wc = repo[None]
    p1 = wc.p1()
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
                    raise error.Abort(msg, hint=hint)
                else:  # destination is not a descendant.
                    msg = _("not a linear update")
                    hint = _("merge or update --check to force update")
                    raise error.Abort(msg, hint=hint)

    return rev
