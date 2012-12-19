# branchmap.py - logic to computes, maintain and stores branchmap for local repo
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import bin, hex, nullid, nullrev
import encoding

def read(repo):
    partial = {}
    try:
        f = repo.opener("cache/branchheads")
        lines = f.read().split('\n')
        f.close()
    except (IOError, OSError):
        return {}, nullid, nullrev

    try:
        last, lrev = lines.pop(0).split(" ", 1)
        last, lrev = bin(last), int(lrev)
        if lrev >= len(repo) or repo[lrev].node() != last:
            # invalidate the cache
            raise ValueError('invalidating branch cache (tip differs)')
        for l in lines:
            if not l:
                continue
            node, label = l.split(" ", 1)
            label = encoding.tolocal(label.strip())
            if not node in repo:
                raise ValueError('invalidating branch cache because node '+
                                 '%s does not exist' % node)
            partial.setdefault(label, []).append(bin(node))
    except KeyboardInterrupt:
        raise
    except Exception, inst:
        if repo.ui.debugflag:
            repo.ui.warn(str(inst), '\n')
        partial, last, lrev = {}, nullid, nullrev
    return partial, last, lrev

def write(repo, branches, tip, tiprev):
    try:
        f = repo.opener("cache/branchheads", "w", atomictemp=True)
        f.write("%s %s\n" % (hex(tip), tiprev))
        for label, nodes in branches.iteritems():
            for node in nodes:
                f.write("%s %s\n" % (hex(node), encoding.fromlocal(label)))
        f.close()
    except (IOError, OSError):
        pass

def update(repo, partial, ctxgen):
    """Given a branchhead cache, partial, that may have extra nodes or be
    missing heads, and a generator of nodes that are at least a superset of
    heads missing, this function updates partial to be correct.
    """
    # collect new branch entries
    newbranches = {}
    for c in ctxgen:
        newbranches.setdefault(c.branch(), []).append(c.node())
    # if older branchheads are reachable from new ones, they aren't
    # really branchheads. Note checking parents is insufficient:
    # 1 (branch a) -> 2 (branch b) -> 3 (branch a)
    for branch, newnodes in newbranches.iteritems():
        bheads = partial.setdefault(branch, [])
        # Remove candidate heads that no longer are in the repo (e.g., as
        # the result of a strip that just happened).  Avoid using 'node in
        # self' here because that dives down into branchcache code somewhat
        # recursively.
        bheadrevs = [repo.changelog.rev(node) for node in bheads
                     if repo.changelog.hasnode(node)]
        newheadrevs = [repo.changelog.rev(node) for node in newnodes
                       if repo.changelog.hasnode(node)]
        ctxisnew = bheadrevs and min(newheadrevs) > max(bheadrevs)
        # Remove duplicates - nodes that are in newheadrevs and are already
        # in bheadrevs.  This can happen if you strip a node whose parent
        # was already a head (because they're on different branches).
        bheadrevs = sorted(set(bheadrevs).union(newheadrevs))

        # Starting from tip means fewer passes over reachable.  If we know
        # the new candidates are not ancestors of existing heads, we don't
        # have to examine ancestors of existing heads
        if ctxisnew:
            iterrevs = sorted(newheadrevs)
        else:
            iterrevs = list(bheadrevs)

        # This loop prunes out two kinds of heads - heads that are
        # superseded by a head in newheadrevs, and newheadrevs that are not
        # heads because an existing head is their descendant.
        while iterrevs:
            latest = iterrevs.pop()
            if latest not in bheadrevs:
                continue
            ancestors = set(repo.changelog.ancestors([latest],
                                                     bheadrevs[0]))
            if ancestors:
                bheadrevs = [b for b in bheadrevs if b not in ancestors]
        partial[branch] = [repo.changelog.node(rev) for rev in bheadrevs]

    # There may be branches that cease to exist when the last commit in the
    # branch was stripped.  This code filters them out.  Note that the
    # branch that ceased to exist may not be in newbranches because
    # newbranches is the set of candidate heads, which when you strip the
    # last commit in a branch will be the parent branch.
    for branch in partial.keys():
        nodes = [head for head in partial[branch]
                 if repo.changelog.hasnode(head)]
        if not nodes:
            del partial[branch]

