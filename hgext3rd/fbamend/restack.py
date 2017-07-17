# restack.py - rebase to make a stack connected again
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from collections import deque

from mercurial import (
    cmdutil,
    commands,
    error,
)

from . import common

def restack(ui, repo, rebaseopts=None):
    """Repair a situation in which one or more changesets in a stack
       have been obsoleted (thereby leaving their descendants in the stack
       unstable) by finding any such changesets and rebasing their descendants
       onto the latest version of each respective changeset.
    """
    if rebaseopts is None:
        rebaseopts = {}

    with repo.wlock(), repo.lock():
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)

        # Find the latest version of the changeset at the botom of the
        # current stack. If the current changeset is public, simply start
        # restacking from the current changeset with the assumption
        # that there are non-public changesets higher up.
        base = repo.revs('::. & draft()').first()
        latest = (common.latest(repo, base) if base is not None
                                     else repo['.'].rev())
        targets = _findrestacktargets(repo, latest)

        with repo.transaction('restack') as tr:
            # Attempt to stabilize all changesets that are or will be (after
            # rebasing) descendants of base.
            for rev in targets:
                try:
                    common.restackonce(ui, repo, rev, rebaseopts)
                except error.InterventionRequired:
                    tr.close()
                    raise

            # Ensure that we always end up on the latest version of the
            # current changeset. Usually, this will be taken care of
            # by the rebase operation. However, in some cases (such as
            # if we are on the precursor of the base changeset) the
            # rebase will not update to the latest version, so we need
            # to do this manually.
            successor = repo.revs('allsuccessors(.)').last()
            if successor is not None:
                commands.update(ui, repo, rev=successor)

def _findrestacktargets(repo, base):
    """Starting from the given base revision, do a BFS forwards through
       history, looking for changesets with unstable descendants on their
       precursors. Returns a list of any such changesets, in a top-down
       ordering that will allow all of the descendants of their precursors
       to be correctly rebased.
    """
    childrenof = common.getchildrelationships(repo,
        repo.revs('%d + allprecursors(%d)', base, base))

    # Perform BFS starting from base.
    queue = deque([base])
    targets = []
    processed = set()
    while queue:
        rev = queue.popleft()

        # Merges may result in the same revision being added to the queue
        # multiple times. Filter those cases out.
        if rev in processed:
            continue

        processed.add(rev)

        # Children need to be added in sorted order so that newer
        # children (as determined by rev number) will have their
        # descendants of their precursors rebased before older children.
        # This ensures that unstable changesets will always be rebased
        # onto the latest visible successor of their parent changeset.
        queue.extend(sorted(childrenof[rev]))

        # Look for visible precursors (which are probably visible because
        # they have unstable descendants) and successors (for which the latest
        # non-obsolete version should be visible).
        precursors = repo.revs('allprecursors(%d)', rev)
        successors = repo.revs('allsuccessors(%d)', rev)

        # If this changeset has precursors but no successor, then
        # if its precursors have children those children need to be
        # rebased onto the changeset.
        if precursors and not successors:
            children = []
            for p in precursors:
                children.extend(childrenof[p])
            if children:
                queue.extend(children)
                targets.append(rev)

    # We need to perform the rebases in reverse-BFS order so that
    # obsolescence information at lower levels is not modified by rebases
    # at higher levels.
    return reversed(targets)
