# smartlog.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import extensions, util, cmdutil, graphmod, templatekw, scmutil
from mercurial import bookmarks, commands, error
from mercurial.extensions import wrapfunction
from hgext import pager
from mercurial.node import hex, nullrev
from mercurial.i18n import _
import errno, os, re

pager.attended.append('smartlog')

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'
enabled = False

def uisetup(ui):
    # Hide output for fake nodes
    def show(orig, self, ctx, copies, matchfn, props):
        if ctx.node() == "...":
            self.ui.write('\n\n\n')
            return
        return orig(self, ctx, copies, matchfn, props)

    wrapfunction(cmdutil.changeset_printer, '_show', show)
    wrapfunction(cmdutil.changeset_templater, '_show', show)

    def ascii(orig, ui, state, type, char, text, coldata):
        # Show . for fake nodes
        if type == 'F':
            char = "."
        # Color the current commits. @ is too subtle
        if enabled and char == "@":
            newtext = []
            for line in text:
                line = "\033[35m" + line + "\033[0m"
                newtext.append(line)
            text = newtext
        return orig(ui, state, type, char, text, coldata)
    wrapfunction(graphmod, 'ascii', ascii)

    def drawedges(orig, edges, nodeline, interline):
        orig(edges, nodeline, interline)
        if enabled:
            for (start, end) in edges:
                if start == end:
                    # terrible hack, but this makes the line below
                    # the commit marker (.) also be a .
                    if '.' in nodeline:
                        interline[2 * start] = "."
    wrapfunction(graphmod, '_drawedges', drawedges)

# copied from graphmod or cmdutil or somewhere...
def grandparent(cl, lowestrev, roots, head):
    """Return all ancestors of head in roots which revision is
    greater or equal to lowestrev.
    """
    pending = set([head])
    seen = set()
    kept = set()
    llowestrev = max(nullrev, lowestrev)
    while pending:
        r = pending.pop()
        if r >= llowestrev and r not in seen:
            if r in roots:
                kept.add(r)
            else:
                pending.update([p for p in cl.parentrevs(r)])
            seen.add(r)
    return sorted(kept)

def sortnodes(nodes, parentfunc, masters):
    """Topologically sorts the nodes, using the parentfunc to find
    the parents of nodes.  Given a topological tie between children,
    any node in masters is chosen last."""
    nodes = set(nodes)
    childmap = {}
    parentmap = {}
    roots = []

    # Build a child and parent map
    for n in nodes:
        parents = [p for p in parentfunc(n) if p in nodes]
        parentmap[n] = set(parents)
        for p in parents:
            childmap.setdefault(p, set()).add(n)
        if not parents:
            roots.append(n)

    def childsort(x, y):
        xm = x in masters
        ym = y in masters
        # Process children in the master line last.
        # This makes them always appear on the left side of the dag,
        # resulting in a nice straight master line in the ascii output.
        if xm and not ym:
            return 1
        elif not xm and ym:
            return -1
        else:
            # If both children are not in the master line, show the oldest first,
            # so the graph is approximately in chronological order.
            return x - y

    # Process roots, adding children to the queue as they become roots
    results = []
    while roots:
        n = roots.pop(0)
        results.append(n)
        if n in childmap:
            children = list(childmap[n])
            # reverse=True here because we insert(0) below, resulting
            # in a reversed insertion of the children.
            children = sorted(children, reverse=True, cmp=childsort)
            for c in children:
                childparents = parentmap[c]
                childparents.remove(n)
                if len(childparents) == 0:
                    # insert at the beginning, that way child nodes
                    # are likely to be output immediately after their
                    # parents.
                    roots.insert(0, c)

    return results

def getdag(repo, revs, master):
    cl = repo.changelog
    lowestrev = min(revs)

    # Fake ctx that we stick in the dag so we can special case it later
    class fakectx(object):
        def __init__(self, rev):
            self._rev = rev
        def node(self):
            return "..."
        def obsolete(self):
            return False
        def rev(self):
            return self._rev
        def files(self):
            return []

    fakes = {}
    knownrevs = set(revs)
    gpcache = {}
    results = []

    # For each rev we need to show, compute it's parents in the dag.
    # If we have to reach for a grandparent, insert a fake node so we
    # can show '...' in the graph.
    # Use 'reversed' to start at the lowest commit so fake nodes are
    # placed at their lowest possible positions.
    for rev in reversed(revs):
        ctx = repo[rev]
        # Parents in the dag
        parents = sorted(set([p.rev() for p in ctx.parents()
                              if p.rev() in knownrevs]))
        # Parents not in the dag
        mpars = [p.rev() for p in ctx.parents() if
                 p.rev() != nullrev and p.rev() not in parents]

        fake = None
        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                gp = gpcache[mpar] = grandparent(cl, lowestrev, revs, mpar)
            if not gp:
                parents.append(mpar)
            else:
                gp = [g for g in gp if g not in parents]
                for g in gp:
                    # Insert fake nods in between children and grandparents.
                    # Reuse them across multiple chidlren when the grandparent
                    # is the same.
                    if not g in fakes:
                        fakes[g] = (mpar, 'F', fakectx(mpar), [g])
                        fake = fakes[g]
                    parents.append(fakes[g][0])

        results.append((ctx.rev(), 'C', ctx, parents))
        if fake:
            results.append(fake)

    # Compute parent rev->parents mapping
    lookup = {}
    for r in results:
        lookup[r[0]] = r[3]
    def parentfunc(node):
        return lookup.get(node, [])

    # Compute the revs on the master line. We use this for sorting later.
    masters = set()
    queue = [master]
    while queue:
        m = queue.pop()
        if not m in masters:
            masters.add(m)
            queue.extend(lookup.get(m, []))

    # Topologically sort the noderev numbers
    order = sortnodes([r[0] for r in results], parentfunc, masters)

    # Sort the actual results based on their position in the 'order'
    return sorted(results, key=lambda x: order.index(x[0]) , reverse=True)

@command('^smartlog|slog', [
    ('', 'template', '', _('display with template'), _('TEMPLATE')),
    ('', 'master', '', _('master bookmark'), ''),
    ] + commands.logopts, _('hg smartlog|slog'))
def mylog(ui, repo, *pats, **opts):
    '''Displays the graph of commits that are relevant to you.
Also highlights your current commit in purple.

Includes:

- Your bookmarks
- The @ or master bookmark (or tip if no bookmarks present).
- Your local commit heads that don't have bookmarks.

Excludes:

- All commits under @/master/tip that aren't related to your commits.
- Your local commit heads that are older than 2 weeks.
    '''
    master = opts.get('master')
    revs = set()
    heads = set()

    rev = repo.changelog.rev
    ancestor = repo.changelog.ancestor
    node = repo.changelog.node
    parentrevs = repo.changelog.parentrevs

    # Find all bookmarks and recent heads
    books = bookmarks.bmstore(repo)
    for b in books:
        heads.add(rev(books[b]))
    heads.update(repo.revs('head() & date(-14) & branch(.)'))

    if not master:
        if '@' in books:
            master = '@'
        elif 'master' in books:
            master = 'master'
        elif 'trunk' in books:
            master = 'trunk'
        else:
            master = 'tip'

    try:
        master = repo.revs(master)[0]
    except error.RepoLookupError:
        master = repo.revs('tip')[0]

    # Find ancestors of heads that are not in master
    # Don't use revsets, they are too slow
    for head in heads:
        anc = rev(ancestor(node(head), node(master)))
        queue = [head]
        while queue:
            current = queue.pop(0)
            if not current in revs:
                revs.add(current)
                if current != anc:
                    parents = parentrevs(current)
                    for p in parents:
                        if p > anc:
                            queue.append(p)

    # add context: master, current commit, and the common ancestor
    revs.add(master)
    revs.update(repo.revs('.'))

    if -1 in revs:
        revs.remove(-1)

    # get common ancestor
    anc = None
    for r in revs:
        if anc is None:
            anc = r
        else:
            anc = rev(ancestor(node(anc), node(r)))
    if anc:
        revs.add(anc)

    revs = sorted(list(revs), reverse=True)

    # Print it!
    global enabled
    try:
        enabled = True
        revdag = getdag(repo, revs, master)
        displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
        showparents = [ctx.node() for ctx in repo[None].parents()]
        cmdutil.displaygraph(ui, revdag, displayer, showparents,
                     graphmod.asciiedges, None, None)
    finally:
        enabled = False
