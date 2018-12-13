# mutation.py - commit mutation tracking
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import error, node as nodemod, util


def record(repo, extra, prednodes, op=None):
    for key in "mutpred", "mutuser", "mutdate", "mutop":
        if key in extra:
            del extra[key]
    if recording(repo):
        extra["mutpred"] = ",".join(nodemod.hex(p) for p in prednodes)
        extra["mutuser"] = repo.ui.config("mutation", "user") or repo.ui.username()
        date = repo.ui.config("mutation", "date")
        if date is None:
            date = util.makedate()
        else:
            date = util.parsedate(date)
        extra["mutdate"] = "%d %d" % date
        if op is not None:
            extra["mutop"] = op


def recording(repo):
    return repo.ui.configbool("mutation", "record")


def enabled(repo):
    return repo.ui.configbool("mutation", "enabled")


def predecessorsset(repo, startnode, closest=False):
    """Return a list of the commits that were replaced by the startnode.

    If there are no such commits, returns a list containing the startnode.

    If ``closest`` is True, returns a list of the visible commits that are the
    closest previous version of the start node.

    If ``closest`` is False, returns a list of the earliest original versions of
    the start node.
    """
    unfi = repo.unfiltered()
    cl = unfi.changelog
    clrevision = cl.changelogrevision

    def get(node):
        if node in unfi:
            extra = clrevision(node).extra
            if "mutpred" in extra:
                return [nodemod.bin(x) for x in extra["mutpred"].split(",")]
        return [node]

    preds = [startnode]
    nextpreds = sum((get(p) for p in preds), [])
    expanded = nextpreds != preds
    while expanded:
        if all(p in repo for p in nextpreds):
            # We have found a set of predecessors that are all visible - this is
            # a valid set to return.
            preds = nextpreds
            if closest:
                break
            # Now look at the next predecessors of each commit.
            newnextpreds = sum((get(p) for p in nextpreds), [])
        else:
            # Expand out to the predecessors of the commits until we find visible
            # ones.
            newnextpreds = sum(([p] if p in repo else get(p) for p in nextpreds), [])
        expanded = newnextpreds != nextpreds
        nextpreds = newnextpreds
        if not expanded:
            # We've reached a stable state and some of the commits might not be
            # visible.  Remove the invisible commits, and continue with what's
            # left.
            newnextpreds = [p for p in nextpreds if p in repo]
            if newnextpreds:
                expanded = newnextpreds != nextpreds
                nextpreds = newnextpreds
    return util.removeduplicates(preds)


def toposortrevs(repo, revs, predmap):
    """topologically sort revs according to the given predecessor map"""
    dag = {}
    valid = set(revs)
    heads = set(revs)
    clparentrevs = repo.changelog.parentrevs
    for rev in revs:
        prev = [p for p in clparentrevs(rev) if p in valid]
        prev.extend(predmap[rev])
        heads.difference_update(prev)
        dag[rev] = prev
    if not heads:
        raise error.Abort("commit predecessors and ancestors contain a cycle")
    seen = set()
    sortedrevs = []
    revstack = list(reversed(sorted(heads)))
    while revstack:
        rev = revstack[-1]
        if rev not in seen:
            seen.add(rev)
            for next in reversed(dag[rev]):
                if next not in seen:
                    revstack.append(next)
        else:
            sortedrevs.append(rev)
            revstack.pop()
    return sortedrevs


def toposort(repo, items, nodefn=None):
    """topologically sort nodes according to the given predecessor map

    items can either be nodes, or something convertible to nodes by a provided
    node function.
    """
    if nodefn is None:
        nodefn = lambda item: item
    clrev = repo.changelog.rev
    revmap = {clrev(nodefn(x)): i for i, x in enumerate(items)}
    predmap = {}
    for item in items:
        node = nodefn(item)
        rev = clrev(node)
        predmap[rev] = [
            r
            for r in map(clrev, predecessorsset(repo, node, closest=True))
            if r != rev and r in revmap
        ]
    sortedrevs = toposortrevs(repo, revmap.keys(), predmap)
    return [items[revmap[r]] for r in sortedrevs]
