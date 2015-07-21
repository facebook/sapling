# smartlog.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import extensions, util, cmdutil, graphmod, templatekw, scmutil
from mercurial import bookmarks, commands, error, revset
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
commit_info = False
hiddenchanges = 0

def uisetup(ui):
    # Hide output for fake nodes
    def show(orig, self, ctx, copies, matchfn, props):
        if ctx.node() == "...":
            self.ui.write('\n\n\n')
            return
        res = orig(self, ctx, copies, matchfn, props)

        if commit_info and ctx == self.repo['.']:
            changes = ctx.p1().status(ctx)
            prefix = ['M', 'A', 'R', '!', '?', 'I', 'C']
            for i in range (0, len(prefix)):
                for f in changes[i]:
                    self.ui.write(' ' + prefix[i] + ' ' + f + '\n')
            self.ui.write('\n')
        return res

    wrapfunction(cmdutil.changeset_printer, '_show', show)
    wrapfunction(cmdutil.changeset_templater, '_show', show)

    def ascii(orig, ui, state, type, char, text, coldata):
        # Show . for fake nodes
        if type == 'F':
            char = "."
        # Color the current commits. @ is too subtle
        if enabled and getattr(ui, '_colormode', '') == 'ansi':
            color = None
            if char == "@":
                color = "\033[35m"
            elif char == "x":
                color = "\033[30m\033[1m"
            if color is not None:
                text = [color + line + "\033[0m" for line in text]
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

    revset.symbols['smartlog'] = smartlogrevset
    revset.safesymbols.add('smartlog')

# copied from graphmod or cmdutil or somewhere...
def grandparent(cl, lowestrev, roots, head):
    """Return all ancestors of head in roots whose revision is
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

def getdag(ui, repo, revs, master):
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
        def closesbranch(self):
            return False

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

        fake_nodes = []
        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                gp = gpcache[mpar] = grandparent(cl, lowestrev, revs, mpar)
            if not gp:
                parents.append(mpar)
            else:
                gp = [g for g in gp if g not in parents]
                for g in gp:
                    # Insert fake nodes in between children and grandparents.
                    # Reuse them across multiple chidlren when the grandparent
                    # is the same.
                    if not g in fakes:
                        fakes[g] = (mpar, 'F', fakectx(mpar), [g])
                        results.append(fakes[g])
                    parents.append(fakes[g][0])

        results.append((ctx.rev(), 'C', ctx, parents))

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
    try:
        return sorted(results, key=lambda x: order.index(x[0]) , reverse=True)
    except ValueError: # Happend when 'order' is empty
        ui.warn('note: Smartlog encountered an error, so the sorting might be wrong.\n\n')
        return sorted(results, reverse=True)

def _masterrevset(ui, repo, masterstring):
    """
    Try to find the name of ``master`` -- usually a bookmark.

    Defaults to 'tip' if no suitable local or remote bookmark is found.
    """

    if not masterstring:
        masterstring = ui.config('smartlog', 'master')

    if masterstring:
        return masterstring

    names = set(bookmarks.bmstore(repo).keys())
    if util.safehasattr(repo, 'names') and 'remotebookmarks' in repo.names:
        names.update(set(repo.names['remotebookmarks'].listnames(repo)))

    for name in _reposnames():
        if name in names:
            return name

    return 'tip'

def _reposnames():
    # '' is local repo. This also defines an order precedence for master.
    repos = ['', 'remote/', 'default/']
    names = ['@', 'master', 'trunk', 'stable']

    for repo in repos:
        for name in names:
            yield repo + name

def _masterrev(repo, masterrevset):
    try:
        master = scmutil.revsingle(repo, masterrevset)
    except error.RepoLookupError:
        master = scmutil.revsingle(repo, _masterrevset(repo.ui, repo, ''))

    if master:
        return master.rev()
    return None

def smartlogrevset(repo, subset, x):
    """``smartlog([scope, [master]])``
    Revisions included by default in the smartlog extension
    """

    args = revset.getargs(x, 0, 2, _('smartlog takes up to 2 arguments'))
    if len(args) > 0:
        scope = revset.getstring(args[0],
                                 _('scope must be either "all" or "recent"'))
        if scope not in ('all', 'recent'):
            raise util.Abort(_('scope must be either "all" or "recent"'))
    else:
        scope = 'recent'
    if len(args) > 1:
        masterstring = revset.getstring(args[1], _('master must be a string'))
    else:
        masterstring = ''

    revs = set()
    heads = set()

    rev = repo.changelog.rev
    branchinfo = repo.changelog.branchinfo
    ancestor = repo.changelog.ancestor
    node = repo.changelog.node
    parentrevs = repo.changelog.parentrevs

    books = bookmarks.bmstore(repo)
    ignore = re.compile(repo.ui.config('smartlog',
                                       'ignorebookmarks',
                                       '!'))
    for b in books:
        if not ignore.match(b):
            heads.add(rev(books[b]))

    # add 'interesting' remote bookmarks as well
    remotebooks = set()
    if util.safehasattr(repo, 'names') and 'remotebookmarks' in repo.names:
        ns = repo.names['remotebookmarks']
        remotebooks = set(ns.listnames(repo))
        for name in _reposnames():
            if name in remotebooks:
                heads.add(rev(ns.namemap(repo, name)[0]))

    heads.update(repo.revs('.'))

    global hiddenchanges
    headquery = 'head() & branch(.)'
    if remotebooks:
        # When we have remote bookmarks, only show draft heads, since public
        # heads should have a remote bookmark indicating them. This allows us to
        # force push server bookmarks to new locations, and not have the commits
        # clutter the user's smartlog.
        headquery += ' & draft()'

    allheads = set(repo.revs(headquery))
    if scope == 'all':
        heads.update(allheads)
    else:
        recent = set(repo.revs(headquery + ' & date(-14)'))
        hiddenchanges += len(allheads - heads) - len(recent - heads)
        heads.update(recent)

    branches = set()
    for head in heads:
        branches.add(branchinfo(head)[0])

    masterrevset = _masterrevset(repo.ui, repo, masterstring)
    masterrev = _masterrev(repo, masterrevset)

    masterbranch = branchinfo(masterrev)[0]

    for branch in branches:
        if branch != masterbranch:
            try:
                rs = 'first(reverse(branch("%s")) & public())' % branch
                branchmaster = repo.revs(rs).first()
                if branchmaster is None:
                    # local-only (draft) branch
                    rs = 'branch("%s")' % branch
                    branchmaster = repo.revs(rs).first()
            except:
                branchmaster = repo.revs('tip').first()
        else:
            branchmaster = masterrev

        # Find ancestors of heads that are not in master
        # Don't use revsets, they are too slow
        for head in heads:
            if branchinfo(head)[0] != branch:
                continue
            anc = rev(ancestor(node(head), node(branchmaster)))
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
        revs.add(branchmaster)

        # get common branch ancestor
        if branch != masterbranch:
            anc = None
            for r in revs:
                if branchinfo(r)[0] != branch:
                    continue
                if anc is None:
                    anc = r
                else:
                    anc = rev(ancestor(node(anc), node(r)))
            if anc:
                revs.add(anc)

    return subset & revs


@command('^smartlog|slog', [
    ('', 'template', '', _('display with template'), _('TEMPLATE')),
    ('', 'master', '', _('master bookmark'), ''),
    ('r', 'rev', [], _('show the specified revisions or range'), _('REV')),
    ('', 'all', False, _('don\'t hide old local commits'), ''),
    ('', 'commit-info', False, _('show changes in current commit'), ''),
    ] + commands.logopts, _('hg smartlog|slog'))
def smartlog(ui, repo, *pats, **opts):
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
    masterstring = opts.get('master')
    masterrevset = _masterrevset(ui, repo, masterstring)

    revs = set()

    global hiddenchanges
    hiddenchanges = 0

    global commit_info
    commit_info = opts.get('commit_info')

    if not opts.get('rev'):
        if opts.get('all'):
            scope = 'all'
        else:
            scope = 'recent'
        revstring = revset.formatspec('smartlog(%s, %s)', scope,
                                      masterrevset)
        revs.update(scmutil.revrange(repo, [revstring]))
        masterrev = _masterrev(repo, masterrevset)
    else:
        revs.update(scmutil.revrange(repo, opts.get('rev')))
        try:
            masterrev = repo.revs('.').first()
        except error.RepoLookupError:
            masterrev = revs[0]

    if -1 in revs:
        revs.remove(-1)

    # It's important that these function caches come after the revsets above,
    # because the revsets may cause extra nodes to become visible, which in turn
    # invalidates the changelog instance.
    rev = repo.changelog.rev
    branchinfo = repo.changelog.branchinfo
    ancestor = repo.changelog.ancestor
    node = repo.changelog.node
    parentrevs = repo.changelog.parentrevs

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

    if len(revs) == 0:
        return

    # Print it!
    global enabled
    try:
        enabled = True
        revdag = getdag(ui, repo, revs, masterrev)
        displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
        showparents = [ctx.node() for ctx in repo[None].parents()]
        cmdutil.displaygraph(ui, revdag, displayer, showparents,
                     graphmod.asciiedges, None, None)
    finally:
        enabled = False

    if hiddenchanges:
        ui.warn("note: hiding %s old heads without bookmarks " % (hiddenchanges) +
            "(use --all to see them)\n")
