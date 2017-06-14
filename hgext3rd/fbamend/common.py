# common.py - common utilities for building commands
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from collections import defaultdict

from hgext import rebase
from mercurial import (
    extensions,
)
from mercurial.node import nullrev

inhibitmod = None

def detectinhibit():
    global inhibitmod
    try:
        inhibitmod = extensions.find('inhibit')
    except KeyError:
        pass

def deinhibit(repo, contexts):
    """Remove any inhibit markers on the given change contexts."""
    if inhibitmod:
        inhibitmod._deinhibitmarkers(repo, (ctx.node() for ctx in contexts))

def getchildrelationships(repo, revs):
    """Build a defaultdict of child relationships between all descendants of
       revs. This information will prevent us from having to repeatedly
       perform children that reconstruct these relationships each time.
    """
    cl = repo.changelog
    children = defaultdict(set)
    for rev in repo.revs('(%ld)::', revs):
        for parent in cl.parentrevs(rev):
            if parent != nullrev:
                children[parent].add(rev)
    return children

def restackonce(ui, repo, rev, rebaseopts=None, childrenonly=False):
    """Rebase all descendants of precursors of rev onto rev, thereby
       stabilzing any non-obsolete descendants of those precursors.
       Takes in an optional dict of options for the rebase command.
       If childrenonly is True, only rebases direct children of precursors
       of rev rather than all descendants of those precursors.
    """
    # Get visible descendants of precusors of rev.
    allprecursors = repo.revs('allprecursors(%d)', rev)
    fmt = '%s(%%ld) - %%ld' % ('children' if childrenonly else 'descendants')
    descendants = repo.revs(fmt, allprecursors, allprecursors)

    # Nothing to do if there are no descendants.
    if not descendants:
        return

    # Overwrite source and destination, leave all other options.
    if rebaseopts is None:
        rebaseopts = {}
    rebaseopts['rev'] = descendants
    rebaseopts['dest'] = rev

    # We need to ensure that the 'operation' field in the obsmarker metadata
    # is always set to 'rebase', regardless of the current command so that
    # the restacked commits will appear as 'rebased' in smartlog.
    overrides = {}
    try:
        tweakdefaults = extensions.find('tweakdefaults')
    except KeyError:
        # No tweakdefaults extension -- skip this since there is no wrapper
        # to set the metadata.
        pass
    else:
        overrides[(tweakdefaults.globaldata,
                   tweakdefaults.createmarkersoperation)] = 'rebase'

    # Perform rebase.
    with repo.ui.configoverride(overrides, 'restack'):
        rebase.rebase(ui, repo, **rebaseopts)

    # Remove any preamend bookmarks on precursors.
    _clearpreamend(repo, allprecursors)

    # Deinhibit the precursors so that they will be correctly shown as
    # obsolete. Also deinhibit their ancestors to handle the situation
    # where restackonce() is being used across several transactions
    # (such as calls to `hg next --rebase`), because each transaction
    # close will result in the ancestors being re-inhibited if they have
    # unrebased (and therefore unstable) descendants. As such, the final
    # call to restackonce() at the top of the stack should deinhibit the
    # entire stack.
    ancestors = repo.set('%ld %% %d', allprecursors, rev)
    deinhibit(repo, ancestors)

def _clearpreamend(repo, revs):
    """Remove any preamend bookmarks on the given revisions."""
    # Use unfiltered repo in case the given revs are hidden. This should
    # ordinarily never happen due to the inhibit extension but it's better
    # to be resilient to this case.
    repo = repo.unfiltered()
    cl = repo.changelog
    for rev in revs:
        for bookmark in repo.nodebookmarks(cl.node(rev)):
            if bookmark.endswith('.preamend'):
                repo._bookmarks.pop(bookmark, None)

def latest(repo, rev):
    """Find the "latest version" of the given revision -- either the
       latest visible successor, or the revision itself if it has no
       visible successors.
    """
    latest = repo.revs('allsuccessors(%d)', rev).last()
    return latest if latest is not None else rev
