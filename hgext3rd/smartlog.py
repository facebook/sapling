# smartlog.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""command to display a relevant subgraph

With this extension installed, Mercurial gains one new command: smartlog.
It displays a subgraph of changesets containing only the changesets relevant
to the user.

::

    [smartlog]
    # (remote) names to show
    repos = , remote/, default/
    names = @, master, stable
    # move the top non-public stack to the second column
    indentnonpublic = True
"""

from __future__ import absolute_import

from itertools import chain
import re

from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    error,
    extensions,
    graphmod,
    obsolete,
    phases,
    revset,
    scmutil,
    templatekw,
    util,
)
from mercurial import node as nodemod
from mercurial.i18n import _
from hgext import pager

pager.attended.append('smartlog')

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'
enabled = False
commit_info = False
hiddenchanges = 0

def _drawendinglines(orig, lines, extra, edgemap, seen):
    # if we are going to have only one single column, draw the missing '|'s
    # and restore everything to normal. see comment in 'ascii' below for an
    # example of what will be changed. note: we do not respect 'graphstyle'
    # but always draw '|' here, for simplicity.
    if len(seen) == 1 or any(l[0:2] != [' ', ' '] for l in lines):
        # draw '|' from bottom to top in the 1st column to connect to
        # something, like a '/' in the 2nd column, or a '+' in the 1st column.
        for line in reversed(lines):
            if line[0:2] != [' ', ' ']:
                break
            line[0] = '|'
        # undo the wrapfunction
        extensions.unwrapfunction(graphmod, '_drawendinglines',
                                  _drawendinglines)
        # restore the space to '|'
        for k, v in edgemap.iteritems():
            if v == ' ':
                edgemap[k] = '|'
    orig(lines, extra, edgemap, seen)

def uisetup(ui):
    # Hide output for fake nodes
    def show(orig, self, ctx, copies, matchfn, props):
        if ctx.node() == "...":
            self.ui.write('\n\n\n')
            return
        res = orig(self, ctx, copies, matchfn, props)

        if commit_info and ctx == self.repo['.']:
            changes = ctx.p1().status(ctx)
            prefixes = ['M', 'A', 'R', '!', '?', 'I', 'C']
            for prefix, change in zip(prefixes, changes):
                for fname in change:
                    self.ui.write(' {0} {1}\n'.format(prefix, fname))
            self.ui.write('\n')
        return res

    extensions.wrapfunction(cmdutil.changeset_printer, '_show', show)
    extensions.wrapfunction(cmdutil.changeset_templater, '_show', show)

    def ascii(orig, ui, state, type, char, text, coldata):
        if type == 'F':
            # the fake node is used to move draft changesets to the 2nd column.
            # there can be at most one fake node, which should also be at the
            # top of the graph.
            # we should not draw the fake node and its edges, so change its
            # edge style to a space, and return directly.
            # these are very hacky but it seems to work well and it seems there
            # is no other easy choice for now.
            edgemap = state['edges']
            for k in edgemap.iterkeys():
                edgemap[k] = ' '
            # also we need to hack _drawendinglines to draw the missing '|'s:
            #    (before)      (after)
            #     o draft       o draft
            #    /             /
            #                 |
            #   o             o
            extensions.wrapfunction(graphmod, '_drawendinglines',
                                    _drawendinglines)
            return
        orig(ui, state, type, char, text, coldata)

    extensions.wrapfunction(graphmod, 'ascii', ascii)

    revset.symbols['smartlog'] = smartlogrevset
    revset.safesymbols.add('smartlog')

    def singlepublicsuccessor(repo, ctx, templ, **args):
        """:singlepublicsuccessor: String. Get a single public successor for a
        given node.  If there's none or more than one, return empty string.
        This is intended to be used for "Landed as" marking
        in `hg sl` output."""
        successorssets = obsolete.successorssets(repo, ctx.node())
        unfiltered = repo.unfiltered()
        ctxs = (unfiltered[n] for n in chain.from_iterable(successorssets))
        public = (c.hex() for c in ctxs if not c.mutable() and c != ctx)
        first = next(public, '')
        second = next(public, '')

        return '' if first and second else first

    templatekw.keywords['singlepublicsuccessor'] = singlepublicsuccessor

    def rebasesuccessors(repo, ctx, **args):
        """Return all of the node's successors created as a result of rebase"""
        rsnodes = list(modifysuccessors(ctx, 'rebase'))
        return templatekw.showlist('rebasesuccessor', rsnodes, **args)
    templatekw.keywords['rebasesuccessors'] = rebasesuccessors

    def amendsuccessors(repo, ctx, **args):
        """Return all of the node's successors created as a result of amend"""
        asnodes = list(modifysuccessors(ctx, 'amend'))
        return templatekw.showlist('amendsuccessor', asnodes, **args)
    templatekw.keywords['amendsuccessors'] = amendsuccessors

    def splituccessors(repo, ctx, **args):
        """Return all of the node's successors created as a result of split"""
        asnodes = list(modifysuccessors(ctx, 'split'))
        return templatekw.showlist('splitsuccessor', asnodes, **args)
    templatekw.keywords['splitsuccessors'] = splituccessors

    def foldsuccessors(repo, ctx, **args):
        """Return all of the node's successors created as a result of fold"""
        asnodes = list(modifysuccessors(ctx, 'fold'))
        return templatekw.showlist('foldsuccessor', asnodes, **args)
    templatekw.keywords['foldsuccessors'] = foldsuccessors

    def histeditsuccessors(repo, ctx, **args):
        """Return all of the node's successors created as a result of
           histedit
        """
        asnodes = list(modifysuccessors(ctx, 'histedit'))
        return templatekw.showlist('histeditsuccessor', asnodes, **args)
    templatekw.keywords['histeditsuccessors'] = histeditsuccessors

    def showgraphnode(orig, repo, ctx, **args):
        """Show obsolete nodes as 'x', even when inhibited."""
        char = orig(repo, ctx, **args)
        if char != 'o' or ctx.node() == '...':
            return char
        return 'x' if repo.revs('allsuccessors(%d)', ctx.rev()) else char

    def wrapshowgraphnode(loaded):
        """Ensure that evolve is loaded before wrapping showgraph() because
           the wrapper functions uses the 'allsuccessors' revset symbol,
           which is provided by the evolve extension.
        """
        if loaded:
            # Some callers directly call showgraphnode(), so wrap the original
            # function in addition to updating templatekw.keywords.
            extensions.wrapfunction(templatekw, 'showgraphnode', showgraphnode)
            templatekw.keywords['graphnode'] = templatekw.showgraphnode
    extensions.afterloaded('evolve', wrapshowgraphnode)

def modifysuccessors(ctx, operation):
    """Return all of the node's successors which were created as a result
    of a given modification operation (amend/rebase)"""
    hex = nodemod.hex
    return (hex(m.succnodes()[0]) for m in obsolete.successormarkers(ctx)
            if len(m.succnodes()) == 1
            and m.metadata().get('operation') == operation)

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
        if not parents or (len(parents) == 1 and parents[0] == -1) and n != -1:
            roots.append(n)

    def childsortkey(x):
        # Process children in the master line last. This makes them always
        # appear on the left side of the dag, resulting in a nice straight
        # master line in the ascii output. Otherwise show the oldest first, so
        # the graph is approximately in chronological order.
        return (x in masters, x)

    # Process roots, adding children to the queue as they become roots
    results = []
    while roots:
        n = roots.pop(0)
        results.append(n)
        if n in childmap:
            children = list(childmap[n])
            # reverse=True here because we insert(0) below, resulting
            # in a reversed insertion of the children.
            children = sorted(children, reverse=True, key=childsortkey)
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

    # Fake ctx that we stick in the dag so we can special case it later
    class fakectx(object):
        def __init__(self, rev):
            self._rev = rev
        def node(self):
            return "..."
        def obsolete(self):
            return False
        def phase(self):
            return None
        def rev(self):
            return self._rev
        def files(self):
            return []
        def closesbranch(self):
            return False

    knownrevs = set(revs)
    gpcache = {}
    results = []

    # we store parents together with the parent type information
    # but sometimes we need just a list of parents
    # [(a,b), (c,d), (e,f)] => [b, d, f]
    def unzip(parents):
        if parents:
            return list(zip(*parents)[1])
        else:
            return list()

    # For each rev we need to show, compute it's parents in the dag.
    # If we have to reach for a grandparent, insert a fake node so we
    # can show '...' in the graph.
    # Use 'reversed' to start at the lowest commit so fake nodes are
    # placed at their lowest possible positions.
    for rev in reversed(revs):
        ctx = repo[rev]
        # Parents in the dag
        parents = sorted(set([(graphmod.PARENT, p.rev()) for p in ctx.parents()
                              if p.rev() in knownrevs]))
        # Parents not in the dag
        mpars = [p.rev() for p in ctx.parents() if
                 p.rev() != nodemod.nullrev and p.rev() not in unzip(parents)]

        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                gp = gpcache[mpar] = revset.reachableroots(
                    repo, revset.baseset(revs), [mpar])
            if not gp:
                parents.append((graphmod.MISSINGPARENT, mpar))
            else:
                gp = [g for g in gp if g not in unzip(parents)]
                for g in gp:
                    parents.append((graphmod.GRANDPARENT, g))

        results.append((ctx.rev(), 'C', ctx, parents))

    # Compute parent rev->parents mapping
    lookup = {}
    for r in results:
        lookup[r[0]] = unzip(r[3])

    def parentfunc(node):
        return lookup.get(node, [])

    # Compute the revs on the master line. We use this for sorting later.
    masters = set()
    queue = [master]
    while queue:
        m = queue.pop()
        if m not in masters:
            masters.add(m)
            queue.extend(lookup.get(m, []))

    # Topologically sort the noderev numbers. Note: unlike the vanilla
    # topological sorting, we move master to the top.
    order = sortnodes([r[0] for r in results], parentfunc, masters)
    order = dict((e[1], e[0]) for e in enumerate(order))

    # Sort the actual results based on their position in the 'order'
    try:
        results.sort(key=lambda x: order[x[0]], reverse=True)
    except ValueError:  # Happened when 'order' is empty
        msg = _('note: smartlog encountered an error\n')
        hint = _('(so the sorting might be wrong.\n\n)')
        ui.warn(msg)
        ui.warn(hint)
        results.reverse()

    # indent the top non-public stack
    if ui.configbool('smartlog', 'indentnonpublic', False):
        rev, ch, ctx, parents = results[0]
        if ctx.phase() != phases.public:
            # find a public parent and add a fake node, so the non-public nodes
            # will be shown in the non-first column
            prev = None
            for i in xrange(1, len(results)):
                pctx = results[i][2]
                if pctx.phase() == phases.public:
                    prev = results[i][0]
                    break
            # append the fake node to occupy the first column
            if prev:
                fakerev = rev + 1
                results.insert(0, (fakerev, 'F', fakectx(fakerev),
                                   [('P', prev)]))

    return results

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

    for name in _reposnames(ui):
        if name in names:
            return name

    return 'tip'

def _reposnames(ui):
    # '' is local repo. This also defines an order precedence for master.
    repos = ui.configlist('smartlog', 'repos', ['', 'remote/', 'default/'])
    names = ui.configlist('smartlog', 'names', ['@', 'master', 'stable'])

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
            raise error.Abort(_('scope must be either "all" or "recent"'))
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
        for name in _reposnames(repo.ui):
            if name in remotebooks:
                heads.add(rev(ns.namemap(repo, name)[0]))

    heads.update(repo.revs('.'))

    global hiddenchanges
    headquery = 'head() & branch(.)'
    if remotebooks:
        # When we have remote bookmarks, only show draft heads, since public
        # heads should have a remote bookmark indicating them. This allows us
        # to force push server bookmarks to new locations, and not have the
        # commits clutter the user's smartlog.
        headquery = 'draft() &' + headquery

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

    # Check for empty repo
    if len(repo) == 0:
        masterbranch = None
    else:
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
            except Exception:
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
                if current not in revs:
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
    ('', 'all', False, _('don\'t hide old local changesets'), ''),
    ('', 'commit-info', False, _('show changes in current changeset'), ''),
] + commands.logopts, _('hg smartlog|slog'))
def smartlog(ui, repo, *pats, **opts):
    '''displays the graph of changesets that are relevant to you

Includes:

- Your bookmarks
- The @ or master bookmark (or tip if no bookmarks present).
- Your local heads that don't have bookmarks.

Excludes:

- All changesets under @/master/tip that aren't related to your changesets.
- Your local heads that are older than 2 weeks.
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
    # because the revsets may cause extra nodes to become visible, which in
    # turn invalidates the changelog instance.
    rev = repo.changelog.rev
    ancestor = repo.changelog.ancestor
    node = repo.changelog.node

    # Find lowest common ancestors of revs. If we have multiple roots in the
    # repo the following will find one ancestor per group of revs with the
    # same root.
    ancestors = set()
    for r in revs:
        added = False
        for anc in list(ancestors):
            lca = rev(ancestor(node(anc), node(r)))
            if lca != -1:
                if anc != lca:
                    ancestors.discard(anc)
                    ancestors.add(lca)
                added = True

        if not added:
            ancestors.add(r)

    revs |= ancestors

    revs = sorted(list(revs), reverse=True)

    if len(revs) == 0:
        return

    # Print it!
    global enabled
    backupconfig = ui.backupconfig('experimental', 'graphstyle.grandparent')
    try:
        if ui.config('experimental', 'graphstyle.grandparent') == '|':
            ui.setconfig('experimental', 'graphstyle.grandparent', '2.')
        enabled = True
        if masterrevset == 'tip':
            # 'tip' is what _masterrevset always returns when it can't find
            # master or @
            ui.warn(_('warning: there is no master changeset locally, try '
                      'pulling from server\n'))

        revdag = getdag(ui, repo, revs, masterrev)
        displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
        cmdutil.displaygraph(
            ui, repo, revdag, displayer, graphmod.asciiedges, None, None)

        try:
            with open(repo.join('completionhints'), 'w+') as f:
                for rev in revdag:
                    commit_hash = rev[2].node()
                    # Skip fakectxt nodes
                    if commit_hash != '...':
                        f.write(nodemod.short(commit_hash) + '\n')
        except IOError:
            # No write access. No big deal.
            pass
    finally:
        ui.restoreconfig(backupconfig)
        enabled = False

    if hiddenchanges:
        msg = _(
            "note: hiding %s old heads without bookmarks\n") % hiddenchanges
        hint = _("(use --all to see them)\n")
        ui.warn(msg)
        ui.warn(hint)
