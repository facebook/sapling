# discovery.py - protocol changeset discovery functions
#
# Copyright 2010 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import nullid, short
from i18n import _
import util, error

def findincoming(repo, remote, base=None, heads=None, force=False):
    """Return list of roots of the subsets of missing nodes from remote

    If base dict is specified, assume that these nodes and their parents
    exist on the remote side and that no child of a node of base exists
    in both remote and repo.
    Furthermore base will be updated to include the nodes that exists
    in repo and remote but no children exists in repo and remote.
    If a list of heads is specified, return only nodes which are heads
    or ancestors of these heads.

    All the ancestors of base are in repo and in remote.
    All the descendants of the list returned are missing in repo.
    (and so we know that the rest of the nodes are missing in remote, see
    outgoing)
    """
    return findcommonincoming(repo, remote, base, heads, force)[1]

def findcommonincoming(repo, remote, base=None, heads=None, force=False):
    """Return a tuple (common, missing roots, heads) used to identify
    missing nodes from remote.

    If base dict is specified, assume that these nodes and their parents
    exist on the remote side and that no child of a node of base exists
    in both remote and repo.
    Furthermore base will be updated to include the nodes that exists
    in repo and remote but no children exists in both repo and remote.
    In other words, base is the set of heads of the DAG resulting from
    the intersection of the nodes from repo and remote.
    If a list of heads is specified, return only nodes which are heads
    or ancestors of these heads.

    All the ancestors of base are in repo and in remote.
    """
    m = repo.changelog.nodemap
    search = []
    fetch = set()
    seen = set()
    seenbranch = set()
    if base is None:
        base = {}

    if not heads:
        heads = remote.heads()

    if repo.changelog.tip() == nullid:
        base[nullid] = 1
        if heads != [nullid]:
            return [nullid], [nullid], list(heads)
        return [nullid], [], []

    # assume we're closer to the tip than the root
    # and start by examining the heads
    repo.ui.status(_("searching for changes\n"))

    unknown = []
    for h in heads:
        if h not in m:
            unknown.append(h)
        else:
            base[h] = 1

    heads = unknown
    if not unknown:
        return base.keys(), [], []

    req = set(unknown)
    reqcnt = 0

    # search through remote branches
    # a 'branch' here is a linear segment of history, with four parts:
    # head, root, first parent, second parent
    # (a branch always has two parents (or none) by definition)
    unknown = remote.branches(unknown)
    while unknown:
        r = []
        while unknown:
            n = unknown.pop(0)
            if n[0] in seen:
                continue

            repo.ui.debug("examining %s:%s\n"
                          % (short(n[0]), short(n[1])))
            if n[0] == nullid: # found the end of the branch
                pass
            elif n in seenbranch:
                repo.ui.debug("branch already found\n")
                continue
            elif n[1] and n[1] in m: # do we know the base?
                repo.ui.debug("found incomplete branch %s:%s\n"
                              % (short(n[0]), short(n[1])))
                search.append(n[0:2]) # schedule branch range for scanning
                seenbranch.add(n)
            else:
                if n[1] not in seen and n[1] not in fetch:
                    if n[2] in m and n[3] in m:
                        repo.ui.debug("found new changeset %s\n" %
                                      short(n[1]))
                        fetch.add(n[1]) # earliest unknown
                    for p in n[2:4]:
                        if p in m:
                            base[p] = 1 # latest known

                for p in n[2:4]:
                    if p not in req and p not in m:
                        r.append(p)
                        req.add(p)
            seen.add(n[0])

        if r:
            reqcnt += 1
            repo.ui.progress(_('searching'), reqcnt, unit=_('queries'))
            repo.ui.debug("request %d: %s\n" %
                        (reqcnt, " ".join(map(short, r))))
            for p in xrange(0, len(r), 10):
                for b in remote.branches(r[p:p + 10]):
                    repo.ui.debug("received %s:%s\n" %
                                  (short(b[0]), short(b[1])))
                    unknown.append(b)

    # do binary search on the branches we found
    while search:
        newsearch = []
        reqcnt += 1
        repo.ui.progress(_('searching'), reqcnt, unit=_('queries'))
        for n, l in zip(search, remote.between(search)):
            l.append(n[1])
            p = n[0]
            f = 1
            for i in l:
                repo.ui.debug("narrowing %d:%d %s\n" % (f, len(l), short(i)))
                if i in m:
                    if f <= 2:
                        repo.ui.debug("found new branch changeset %s\n" %
                                          short(p))
                        fetch.add(p)
                        base[i] = 1
                    else:
                        repo.ui.debug("narrowed branch search to %s:%s\n"
                                      % (short(p), short(i)))
                        newsearch.append((p, i))
                    break
                p, f = i, f * 2
            search = newsearch

    # sanity check our fetch list
    for f in fetch:
        if f in m:
            raise error.RepoError(_("already have changeset ")
                                  + short(f[:4]))

    if base.keys() == [nullid]:
        if force:
            repo.ui.warn(_("warning: repository is unrelated\n"))
        else:
            raise util.Abort(_("repository is unrelated"))

    repo.ui.debug("found new changesets starting at " +
                 " ".join([short(f) for f in fetch]) + "\n")

    repo.ui.progress(_('searching'), None)
    repo.ui.debug("%d total queries\n" % reqcnt)

    return base.keys(), list(fetch), heads

def findoutgoing(repo, remote, base=None, heads=None, force=False):
    """Return list of nodes that are roots of subsets not in remote

    If base dict is specified, assume that these nodes and their parents
    exist on the remote side.
    If a list of heads is specified, return only nodes which are heads
    or ancestors of these heads, and return a second element which
    contains all remote heads which get new children.
    """
    if base is None:
        base = {}
        findincoming(repo, remote, base, heads, force=force)

    repo.ui.debug("common changesets up to "
                  + " ".join(map(short, base.keys())) + "\n")

    remain = set(repo.changelog.nodemap)

    # prune everything remote has from the tree
    remain.remove(nullid)
    remove = base.keys()
    while remove:
        n = remove.pop(0)
        if n in remain:
            remain.remove(n)
            for p in repo.changelog.parents(n):
                remove.append(p)

    # find every node whose parents have been pruned
    subset = []
    # find every remote head that will get new children
    updated_heads = set()
    for n in remain:
        p1, p2 = repo.changelog.parents(n)
        if p1 not in remain and p2 not in remain:
            subset.append(n)
        if heads:
            if p1 in heads:
                updated_heads.add(p1)
            if p2 in heads:
                updated_heads.add(p2)

    # this is the set of all roots we have to push
    if heads:
        return subset, list(updated_heads)
    else:
        return subset

def prepush(repo, remote, force, revs, newbranch):
    '''Analyze the local and remote repositories and determine which
    changesets need to be pushed to the remote. Return value depends
    on circumstances:

    If we are not going to push anything, return a tuple (None,
    outgoing) where outgoing is 0 if there are no outgoing
    changesets and 1 if there are, but we refuse to push them
    (e.g. would create new remote heads).

    Otherwise, return a tuple (changegroup, remoteheads), where
    changegroup is a readable file-like object whose read() returns
    successive changegroup chunks ready to be sent over the wire and
    remoteheads is the list of remote heads.'''
    common = {}
    remote_heads = remote.heads()
    inc = findincoming(repo, remote, common, remote_heads, force=force)

    cl = repo.changelog
    update, updated_heads = findoutgoing(repo, remote, common, remote_heads)
    outg, bases, heads = cl.nodesbetween(update, revs)

    if not bases:
        repo.ui.status(_("no changes found\n"))
        return None, 1

    if not force and remote_heads != [nullid]:

        def fail_multiple_heads(unsynced, branch=None):
            if branch:
                msg = _("abort: push creates new remote heads"
                        " on branch '%s'!\n") % branch
            else:
                msg = _("abort: push creates new remote heads!\n")
            repo.ui.warn(msg)
            if unsynced:
                repo.ui.status(_("(you should pull and merge or"
                                 " use push -f to force)\n"))
            else:
                repo.ui.status(_("(did you forget to merge?"
                                 " use push -f to force)\n"))
            return None, 0

        if remote.capable('branchmap'):
            # Check for each named branch if we're creating new remote heads.
            # To be a remote head after push, node must be either:
            # - unknown locally
            # - a local outgoing head descended from update
            # - a remote head that's known locally and not
            #   ancestral to an outgoing head
            #
            # New named branches cannot be created without --force.

            # 1. Create set of branches involved in the push.
            branches = set(repo[n].branch() for n in outg)

            # 2. Check for new branches on the remote.
            remotemap = remote.branchmap()
            newbranches = branches - set(remotemap)
            if newbranches and not newbranch: # new branch requires --new-branch
                branchnames = ', '.join(sorted(newbranches))
                repo.ui.warn(_("abort: push creates "
                               "new remote branches: %s!\n")
                             % branchnames)
                repo.ui.status(_("(use 'hg push --new-branch' to create new "
                                 "remote branches)\n"))
                return None, 0
            branches.difference_update(newbranches)

            # 3. Construct the initial oldmap and newmap dicts.
            # They contain information about the remote heads before and
            # after the push, respectively.
            # Heads not found locally are not included in either dict,
            # since they won't be affected by the push.
            # unsynced contains all branches with incoming changesets.
            oldmap = {}
            newmap = {}
            unsynced = set()
            for branch in branches:
                remoteheads = remotemap[branch]
                prunedheads = [h for h in remoteheads if h in cl.nodemap]
                oldmap[branch] = prunedheads
                newmap[branch] = list(prunedheads)
                if len(remoteheads) > len(prunedheads):
                    unsynced.add(branch)

            # 4. Update newmap with outgoing changes.
            # This will possibly add new heads and remove existing ones.
            ctxgen = (repo[n] for n in outg)
            repo._updatebranchcache(newmap, ctxgen)

            # 5. Check for new heads.
            # If there are more heads after the push than before, a suitable
            # warning, depending on unsynced status, is displayed.
            for branch in branches:
                if len(newmap[branch]) > len(oldmap[branch]):
                    return fail_multiple_heads(branch in unsynced, branch)

            # 6. Check for unsynced changes on involved branches.
            if unsynced:
                repo.ui.warn(_("note: unsynced remote changes!\n"))

        else:
            # Old servers: Check for new topological heads.
            # Code based on _updatebranchcache.
            newheads = set(h for h in remote_heads if h in cl.nodemap)
            oldheadcnt = len(newheads)
            newheads.update(outg)
            if len(newheads) > 1:
                for latest in reversed(outg):
                    if latest not in newheads:
                        continue
                    minhrev = min(cl.rev(h) for h in newheads)
                    reachable = cl.reachable(latest, cl.node(minhrev))
                    reachable.remove(latest)
                    newheads.difference_update(reachable)
            if len(newheads) > oldheadcnt:
                return fail_multiple_heads(inc)
            if inc:
                repo.ui.warn(_("note: unsynced remote changes!\n"))

    if revs is None:
        # use the fast path, no race possible on push
        nodes = repo.changelog.findmissing(common.keys())
        cg = repo._changegroup(nodes, 'push')
    else:
        cg = repo.changegroupsubset(update, revs, 'push')
    return cg, remote_heads
