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
    # whether to use ancestor cache (speed up on huge repos)
    useancestorcache = False
"""

from __future__ import absolute_import

import contextlib
import datetime
import itertools
import re
import time

from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    dagop,
    error,
    extensions,
    graphmod,
    node as nodemod,
    obsutil,
    phases,
    registrar,
    revlog,
    revset,
    revsetlang,
    scmutil,
    smartset,
    templatekw,
    templater,
    util,
)
from mercurial.i18n import _


try:
    # gdbm is preferred for its performance
    import gdbm as dbm

    dbm.open
except ImportError:
    # fallback to anydbm
    import anydbm as dbm

cmdtable = {}
command = registrar.command(cmdtable)
testedwith = "ships-with-fb-hgext"
commit_info = False
hiddenchanges = 0

# Remove unsupported --limit option.
logopts = [opt for opt in commands.logopts if opt[1] != "limit"]

try:
    xrange(0)
except NameError:
    xrange = range


@contextlib.contextmanager
def ancestorcache(path):
    # simple cache to speed up revlog.ancestors
    try:
        db = dbm.open(path, "c")
    except dbm.error:
        # database locked, fail gracefully
        yield
    else:

        def revlogancestor(orig, self, a, b):
            key = a + b
            try:
                return db[key]
            except KeyError:
                result = orig(self, a, b)
                db[key] = result
                return result

        extensions.wrapfunction(revlog.revlog, "ancestor", revlogancestor)
        try:
            yield
        finally:
            extensions.unwrapfunction(revlog.revlog, "ancestor", revlogancestor)
            try:
                db.close()
            except Exception:
                # database corruption, we just nuke the database
                util.tryunlink(path)


def _drawendinglines(orig, lines, extra, edgemap, seen):
    # if we are going to have only one single column, draw the missing '|'s
    # and restore everything to normal. see comment in 'ascii' below for an
    # example of what will be changed. note: we do not respect 'graphstyle'
    # but always draw '|' here, for simplicity.
    if len(seen) == 1 or any(l[0:2] != [" ", " "] for l in lines):
        # draw '|' from bottom to top in the 1st column to connect to
        # something, like a '/' in the 2nd column, or a '+' in the 1st column.
        for line in reversed(lines):
            if line[0:2] != [" ", " "]:
                break
            line[0] = "|"
        # undo the wrapfunction
        extensions.unwrapfunction(graphmod, "_drawendinglines", _drawendinglines)
        # restore the space to '|'
        for k, v in edgemap.iteritems():
            if v == " ":
                edgemap[k] = "|"
    orig(lines, extra, edgemap, seen)


def uisetup(ui):
    # Hide output for fake nodes
    def show(orig, self, ctx, *args):
        if ctx.node() == "...":
            self.ui.write("\n\n\n")
            return
        res = orig(self, ctx, *args)

        if commit_info and ctx == self.repo["."]:
            changes = ctx.p1().status(ctx)
            prefixes = ["M", "A", "R", "!", "?", "I", "C"]
            for prefix, change in zip(prefixes, changes):
                for fname in change:
                    self.ui.write(" {0} {1}\n".format(prefix, fname))
            self.ui.write("\n")
        return res

    extensions.wrapfunction(cmdutil.changeset_printer, "_show", show)
    extensions.wrapfunction(cmdutil.changeset_templater, "_show", show)

    def ascii(orig, ui, state, type, char, text, coldata):
        if type == "F":
            # the fake node is used to move draft changesets to the 2nd column.
            # there can be at most one fake node, which should also be at the
            # top of the graph.
            # we should not draw the fake node and its edges, so change its
            # edge style to a space, and return directly.
            # these are very hacky but it seems to work well and it seems there
            # is no other easy choice for now.
            edgemap = state["edges"]
            for k in edgemap.iterkeys():
                edgemap[k] = " "
            # also we need to hack _drawendinglines to draw the missing '|'s:
            #    (before)      (after)
            #     o draft       o draft
            #    /             /
            #                 |
            #   o             o
            extensions.wrapfunction(graphmod, "_drawendinglines", _drawendinglines)
            return
        orig(ui, state, type, char, text, coldata)

    extensions.wrapfunction(graphmod, "ascii", ascii)

    revset.symbols["smartlog"] = smartlogrevset
    revset.safesymbols.add("smartlog")


templatekeyword = registrar.templatekeyword()
templatefunc = registrar.templatefunc()


@templatekeyword("singlepublicsuccessor")
def singlepublicsuccessor(repo, ctx, templ, **args):
    """String. Get a single public successor for a
    given node.  If there's none or more than one, return empty string.
    This is intended to be used for "Landed as" marking
    in `hg sl` output."""
    successorssets = obsutil.successorssets(repo, ctx.node())
    unfiltered = repo.unfiltered()
    ctxs = (unfiltered[n] for n in itertools.chain.from_iterable(successorssets))
    public = (c.hex() for c in ctxs if not c.mutable() and c != ctx)
    first = next(public, "")
    second = next(public, "")

    return "" if first and second else first


@templatekeyword("shelveenabled")
def shelveenabled(repo, ctx, **args):
    """Bool. Return true if shelve extension is enabled"""
    return "shelve" in extensions.enabled().keys()


@templatekeyword("rebasesuccessors")
def rebasesuccessors(repo, ctx, **args):
    """Return all of the node's successors created as a result of rebase"""
    rsnodes = list(modifysuccessors(ctx, "rebase"))
    return templatekw.showlist("rebasesuccessor", rsnodes, args)


@templatekeyword("amendsuccessors")
def amendsuccessors(repo, ctx, **args):
    """Return all of the node's successors created as a result of amend"""
    asnodes = list(modifysuccessors(ctx, "amend"))
    return templatekw.showlist("amendsuccessor", asnodes, args)


@templatekeyword("splitsuccessors")
def splitsuccessors(repo, ctx, **args):
    """Return all of the node's successors created as a result of split"""
    asnodes = list(modifysuccessors(ctx, "split"))
    return templatekw.showlist("splitsuccessor", asnodes, args)


@templatekeyword("foldsuccessors")
def foldsuccessors(repo, ctx, **args):
    """Return all of the node's successors created as a result of fold"""
    asnodes = list(modifysuccessors(ctx, "fold"))
    return templatekw.showlist("foldsuccessor", asnodes, args)


@templatekeyword("histeditsuccessors")
def histeditsuccessors(repo, ctx, **args):
    """Return all of the node's successors created as a result of
       histedit
    """
    asnodes = list(modifysuccessors(ctx, "histedit"))
    return templatekw.showlist("histeditsuccessor", asnodes, args)


@templatekeyword("undosuccessors")
def undosuccessors(repo, ctx, **args):
    """Return all of the node's successors created as a result of undo"""
    asnodes = list(modifysuccessors(ctx, "undo"))
    return templatekw.showlist("undosuccessor", asnodes, args)


def successormarkers(ctx):
    for data in ctx.repo().obsstore.successors.get(ctx.node(), ()):
        yield obsutil.marker(ctx.repo(), data)


def modifysuccessors(ctx, operation):
    """Return all of the node's successors which were created as a result
    of a given modification operation"""
    repo = ctx.repo().filtered("visible")
    for m in successormarkers(ctx):
        if m.metadata().get("operation") == operation:
            for node in m.succnodes():
                try:
                    repo[node]
                except Exception:
                    # filtered or unknown node
                    pass
                else:
                    yield nodemod.hex(node)


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
        parents = sorted(
            set(
                [
                    (graphmod.PARENT, p.rev())
                    for p in ctx.parents()
                    if p.rev() in knownrevs
                ]
            )
        )
        # Parents not in the dag
        mpars = [
            p.rev()
            for p in ctx.parents()
            if p.rev() != nodemod.nullrev and p.rev() not in unzip(parents)
        ]

        for mpar in mpars:
            gp = gpcache.get(mpar)
            if gp is None:
                gp = gpcache[mpar] = dagop.reachableroots(
                    repo, smartset.baseset(revs), [mpar]
                )
            if not gp:
                parents.append((graphmod.MISSINGPARENT, mpar))
            else:
                gp = [g for g in gp if g not in unzip(parents)]
                for g in gp:
                    parents.append((graphmod.GRANDPARENT, g))

        results.append((ctx.rev(), "C", ctx, parents))

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
        msg = _("note: smartlog encountered an error\n")
        hint = _("(so the sorting might be wrong.\n\n)")
        ui.warn(msg)
        ui.warn(hint)
        results.reverse()

    # indent the top non-public stack
    if ui.configbool("smartlog", "indentnonpublic", False):
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
                results.insert(0, (fakerev, "F", fakectx(fakerev), [("P", prev)]))

    return results


def _masterrevset(ui, repo, masterstring):
    """
    Try to find the name of ``master`` -- usually a bookmark.

    Defaults to the last public revision, if no suitable local or remote
    bookmark is found.
    """

    if not masterstring:
        masterstring = ui.config("smartlog", "master")

    if masterstring:
        return masterstring

    names = set(bookmarks.bmstore(repo).keys())
    if util.safehasattr(repo, "names") and "remotebookmarks" in repo.names:
        names.update(set(repo.names["remotebookmarks"].listnames(repo)))

    for name in _reposnames(ui):
        if name in names:
            return name

    return "last(public())"


def _reposnames(ui):
    # '' is local repo. This also defines an order precedence for master.
    repos = ui.configlist("smartlog", "repos", ["", "remote/", "default/"])
    names = ui.configlist("smartlog", "names", ["@", "master", "stable"])

    for repo in repos:
        for name in names:
            yield repo + name


def _masterrev(repo, masterrevset):
    try:
        master = scmutil.revsingle(repo, masterrevset)
    except error.RepoLookupError:
        master = scmutil.revsingle(repo, _masterrevset(repo.ui, repo, ""))
    except error.Abort:  # empty revision set
        return None

    if master:
        return master.rev()
    return None


def smartlogrevset(repo, subset, x):
    """``smartlog([master], [recentdays=N])``
    Changesets relevent to you.

    'master' is the head of the public branch.
    Unnamed heads will be hidden unless it's within 'recentdays'.
    """

    args = revset.getargsdict(x, "smartlogrevset", "master recentdays")
    if "master" in args:
        masterstring = revsetlang.getstring(
            args["master"], _("master must be a string")
        )
    else:
        masterstring = ""

    recentdays = revsetlang.getinteger(
        args.get("recentdays"), _("recentdays should be int"), -1
    )

    revs = set()
    heads = set()

    rev = repo.changelog.rev
    ancestor = repo.changelog.ancestor
    node = repo.changelog.node
    parentrevs = repo.changelog.parentrevs

    books = bookmarks.bmstore(repo)
    ignore = re.compile(repo.ui.config("smartlog", "ignorebookmarks", "!"))
    for b in books:
        if not ignore.match(b):
            heads.add(rev(books[b]))

    # add 'interesting' remote bookmarks as well
    remotebooks = set()
    if util.safehasattr(repo, "names") and "remotebookmarks" in repo.names:
        ns = repo.names["remotebookmarks"]
        remotebooks = set(ns.listnames(repo))
        for name in _reposnames(repo.ui):
            if name in remotebooks:
                heads.add(rev(ns.namemap(repo, name)[0]))

    heads.update(repo.revs("."))

    global hiddenchanges
    headquery = "head()"
    if remotebooks:
        # When we have remote bookmarks, only show draft heads, since public
        # heads should have a remote bookmark indicating them. This allows us
        # to force push server bookmarks to new locations, and not have the
        # commits clutter the user's smartlog.
        headquery = "heads(draft())"

    allheads = set(repo.revs(headquery))
    if recentdays >= 0:
        recentquery = revsetlang.formatspec("%r & date(-%d)", headquery, recentdays)
        recentrevs = set(repo.revs(recentquery))
        hiddenchanges += len(allheads - heads) - len(recentrevs - heads)
        heads.update(recentrevs)
    else:
        heads.update(allheads)

    masterrevset = _masterrevset(repo.ui, repo, masterstring)
    masterrev = _masterrev(repo, masterrevset)

    if masterrev is None:
        masterrev = repo["tip"].rev()

    masternode = node(masterrev)

    # Find all draft ancestors and latest public ancestor of heads
    # that are not in master.
    # We don't want to draw all public commits because there can be too
    # many of them.
    # Don't use revsets, they are too slow
    for head in heads:
        anc = rev(ancestor(node(head), masternode))
        queue = [head]
        while queue:
            current = queue.pop(0)
            if current not in revs:
                revs.add(current)
                # stop as soon as we find public commit
                ispublic = repo[current].phase() == phases.public
                if current != anc and not ispublic:
                    parents = parentrevs(current)
                    for p in parents:
                        if p >= anc:
                            queue.append(p)

    # add context: master, current commit, and the common ancestor
    revs.add(masterrev)

    return subset & revs


@templatefunc("simpledate(date[, tz])")
def simpledate(context, mapping, args):
    """Date.  Returns a human-readable date/time that is simplified for
    dates and times in the recent past.
    """
    ctx = mapping["ctx"]
    repo = ctx.repo()
    date = templater.evalfuncarg(context, mapping, args[0])
    tz = None
    if len(args) == 2:
        tzname = templater.evalstring(context, mapping, args[1])
        if tzname:
            try:
                import pytz

                tz = pytz.timezone(tzname)
            except ImportError:
                msg = "Couldn't import pytz, using default time zone\n"
                repo.ui.warn(msg)
            except pytz.UnknownTimeZoneError:
                msg = "Unknown time zone: %s\n" % tzname
                repo.ui.warn(msg)
    then = datetime.datetime.fromtimestamp(date[0], tz)
    now = datetime.datetime.now(tz)
    td = now.date() - then.date()
    if then > now:
        # Time is in the future, render it in full
        return then.strftime("%Y-%m-%d %H:%M")
    elif td.days == 0:
        # Today ("Today at HH:MM")
        return then.strftime("Today at %H:%M")
    elif td.days == 1:
        # Yesterday ("Yesterday at HH:MM")
        return then.strftime("Yesterday at %H:%M")
    elif td.days <= 6:
        # In the last week (e.g. "Monday at HH:MM")
        return then.strftime("%A at %H:%M")
    elif now.year == then.year or td.days <= 90:
        # This year or in the last 3 months (e.g. "Jan 05 at HH:MM")
        return then.strftime("%b %d at %H:%M")
    else:
        # Before, render it in full
        return then.strftime("%Y-%m-%d %H:%M")


@templatefunc("smartdate(date, threshold, recent, other)")
def smartdate(context, mapping, args):
    """Date.  Returns one of two values depending on whether the date provided
    is in the past and recent or not."""
    date = templater.evalfuncarg(context, mapping, args[0])
    threshold = templater.evalinteger(context, mapping, args[1])
    now = time.time()
    then = date[0]
    if now - threshold <= then <= now:
        return templater.evalstring(context, mapping, args[2])
    else:
        return templater.evalstring(context, mapping, args[3])


@command(
    "^smartlog|slog",
    [
        ("", "master", "", _("master bookmark"), _("BOOKMARK")),
        ("r", "rev", [], _("show the specified revisions or range"), _("REV")),
        ("", "all", False, _("don't hide old local changesets"), ""),
        ("", "commit-info", False, _("show changes in current changeset"), ""),
    ]
    + logopts,
    _("[OPTION]... [[-r] REV]"),
)
def smartlog(ui, repo, *pats, **opts):
    """show a graph of the commits that are relevant to you

Includes:

- Your bookmarks
- The @ or master bookmark (or tip if no bookmarks present).
- Your local heads that don't have bookmarks.

Excludes:

- All changesets under @/master/tip that aren't related to your changesets.
- Your local heads that are older than 2 weeks."""
    if ui.configbool("smartlog", "useancestorcache"):
        cachevfs = repo.cachevfs

        # The cache directory must exist before we pass the db path to
        # ancestorcache.
        if not cachevfs.exists(""):
            cachevfs.makedir()
        with ancestorcache(cachevfs.join("smartlog-ancestor.db")):
            return _smartlog(ui, repo, *pats, **opts)
    else:
        return _smartlog(ui, repo, *pats, **opts)


def _smartlog(ui, repo, *pats, **opts):
    masterstring = opts.get("master")
    masterrevset = _masterrevset(ui, repo, masterstring)

    revs = set()

    global hiddenchanges
    hiddenchanges = 0

    global commit_info
    commit_info = opts.get("commit_info")

    if not opts.get("rev"):
        if opts.get("all"):
            recentdays = -1
        else:
            recentdays = 14
        masterrev = _masterrev(repo, masterrevset)
        revstring = revsetlang.formatspec(
            "smartlog(%s, %s)", masterrev or "", recentdays
        )
        revs.update(scmutil.revrange(repo, [revstring]))
    else:
        revs.update(scmutil.revrange(repo, opts.get("rev")))
        masterrev = _masterrev(repo, masterrevset)
        if masterrev not in revs:
            try:
                masterrev = repo.revs(".").first()
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
    overrides = {}
    if ui.config("experimental", "graphstyle.grandparent", "2.") == "|":
        overrides[("experimental", "graphstyle.grandparent")] = "2."
    with ui.configoverride(overrides, "smartlog"):
        revdag = getdag(ui, repo, revs, masterrev)
        displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
        ui.pager("smartlog")
        cmdutil.displaygraph(
            ui, repo, revdag, displayer, graphmod.asciiedges, None, None
        )

    try:
        with open(repo.localvfs.join("completionhints"), "w+") as f:
            for rev in revdag:
                commit_hash = rev[2].node()
                # Skip fakectxt nodes
                if commit_hash != "...":
                    f.write(nodemod.short(commit_hash) + "\n")
    except IOError:
        # No write access. No big deal.
        pass

    if hiddenchanges:
        msg = _("note: hiding %s old heads without bookmarks\n") % hiddenchanges
        hint = _("(use --all to see them)\n")
        ui.warn(msg)
        ui.warn(hint)
