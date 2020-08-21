# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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

    # Data. Hide draft commits before "hide-before".
    # This is used to migrate away from the "recent days" behavior and
    # eventually show all visible commits.
    hide-before = 2019-2-22

    # Default parameter for master
    master = remote/master

    # Collapse obsoleted commits
    collapse-obsolete = True
"""

from __future__ import absolute_import

import contextlib
import datetime
import itertools
import re
import time

from edenscm.mercurial import (
    bookmarks,
    cmdutil,
    commands,
    dagop,
    error,
    extensions,
    graphmod,
    mutation,
    node as nodemod,
    obsutil,
    phases,
    pycompat,
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
from edenscm.mercurial.i18n import _
from edenscm.mercurial.pycompat import range


cmdtable = {}
command = registrar.command(cmdtable)
revsetpredicate = registrar.revsetpredicate()

testedwith = "ships-with-fb-hgext"
commit_info = False
hiddenchanges = 0

# Remove unsupported --limit option.
logopts = [opt for opt in commands.logopts if opt[1] != "limit"]

configtable = {}
configitem = registrar.configitem(configtable)

configitem("smartlog", "collapse-obsolete", default=True)


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
        for k, v in pycompat.iteritems(edgemap):
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
            labels = [
                "status.modified",
                "status.added",
                "status.removed",
                "status.deleted",
                "status.unknown",
                "status.ignored",
                "status.copied",
            ]
            for prefix, label, change in zip(prefixes, labels, changes):
                for fname in change:
                    self.ui.write(
                        self.ui.label(" {0} {1}\n".format(prefix, fname), label)
                    )
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
            for k in pycompat.iterkeys(edgemap):
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


templatekeyword = registrar.templatekeyword()
templatefunc = registrar.templatefunc()


@templatekeyword("shelveenabled")
def shelveenabled(repo, ctx, **args):
    """Bool. Return true if shelve extension is enabled"""
    return "shelve" in extensions.enabled().keys()


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

    knownrevs = set(revs)
    gpcache = {}
    results = []
    reserved = []

    # we store parents together with the parent type information
    # but sometimes we need just a list of parents
    # [(a,b), (c,d), (e,f)] => [b, d, f]
    def unzip(parents):
        if parents:
            return list(list(zip(*parents))[1])
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
        ui.warn(_("smartlog encountered an error\n"), notice=_("note"))
        ui.warn(_("(so the sorting might be wrong.\n\n)"))
        results.reverse()

    # indent the top non-public stack
    if ui.configbool("smartlog", "indentnonpublic", False):
        rev, ch, ctx, parents = results[0]
        if ctx.phase() != phases.public:
            # find a public parent and add a fake node, so the non-public nodes
            # will be shown in the non-first column
            prev = None
            for i in range(1, len(results)):
                pctx = results[i][2]
                if pctx.phase() == phases.public:
                    prev = results[i][0]
                    break
            if prev:
                reserved.append(prev)

    return results, reserved


def addfakerev(results, prev):
    # Fake ctx that we stick in the dag so we can special case it later
    class fakectx(object):
        def __init__(self, rev):
            self._rev = rev

        def node(self):
            return "..."

        def obsolete(self):
            return False

        def invisible(self):
            return False

        def phase(self):
            return None

        def rev(self):
            return self._rev

        def files(self):
            return []

        def extra(self):
            return {}

        def closesbranch(self):
            return False

    # append the fake node to occupy the first column
    rev = results[0][0]
    fakerev = rev + 1
    results.insert(0, (fakerev, "F", fakectx(fakerev), [("P", prev)]))


def _reposnames(ui):
    # '' is local repo. This also defines an order precedence for master.
    repos = ui.configlist("smartlog", "repos", ["", "remote/", "default/"])
    names = ui.configlist("smartlog", "names", ["@", "master", "stable"])

    for repo in repos:
        for name in names:
            yield repo + name


@revsetpredicate("smartlog([heads], [master])")
def smartlogrevset(repo, subset, x):
    """``smartlog([heads], [master])``
    Changesets relevent to you.

    'heads' overrides what feature branches to include.
    (default: 'interestingbookmarks() + heads(draft()) + .')

    'master' is the head of the public branch.
    (default: 'interestingmaster()')
    """
    args = revset.getargsdict(x, "smartlogrevset", "heads master")
    if "master" in args:
        masterset = revset.getset(repo, subset, args["master"])
    else:
        masterset = repo.revs("interestingmaster()")

    if "heads" in args:
        heads = set(revset.getset(repo, subset, args["heads"]))
    else:
        heads = set(repo.revs("interestingbookmarks() + heads(draft()) + ."))

    # Remove "null" commit. "::x" does not support it.
    masterset -= smartset.baseset([nodemod.nullrev])
    if nodemod.nullrev in heads:
        heads.remove(nodemod.nullrev)

    cl = repo.changelog
    if cl.algorithmbackend == "segments":
        heads = cl.tonodes(heads)
        master = cl.tonodes(masterset)
        nodes = smartlognodes(repo, heads, master)
        return subset & smartset.idset(cl.torevs(nodes))

    # Explicitly disable revnum deprecation warnings.
    with repo.ui.configoverride({("devel", "legacy.revnum:real"): ""}):
        # Select ancestors that are draft.
        drafts = repo.revs("draft() & ::%ld", heads)
        # Include parents of drafts, and public heads.
        revs = repo.revs(
            "parents(%ld) + %ld + %ld + %ld", drafts, drafts, heads, masterset
        )

    # Include the ancestor of above commits to make the graph connected.
    #
    # When calculating ancestors, filter commits using 'public()' to reduce the
    # number of commits to calculate. This is sound because the above logic
    # includes p1 of draft commits, and assume master is public. Practically,
    # this optimization can make a 3x difference.
    revs = smartset.baseset(repo.revs("ancestor(%ld & public()) + %ld", revs, revs))

    # Collapse long obsoleted stack - only keep their heads and roots.
    # This is incompatible with automation (namely, nuclide-core) yet.
    if repo.ui.configbool("smartlog", "collapse-obsolete") and not repo.ui.plain():
        obsrevs = smartset.baseset(repo.revs("%ld & obsolete()", revs))
        hiderevs = smartset.baseset(
            repo.revs("%ld - (heads(%ld) + roots(%ld))", obsrevs, obsrevs, obsrevs)
        )
        revs = repo.revs("%ld - %ld", revs, hiderevs)

    return subset & revs


def smartlognodes(repo, headnodes, masternodes):
    """Calculate nodes based on new DAG abstraction.
    This function does not use revs or revsets.
    """
    draftnodes = repo.dageval(lambda: ancestors(headnodes) & draft())
    nodes = repo.dageval(
        lambda: parents(draftnodes) | draftnodes | headnodes | masternodes
    )

    # Include the ancestor of above commits to make the graph connected.
    nodes = repo.dageval(lambda: gcaall(public() & nodes) | nodes)

    # Collapse long obsoleted stack - only keep their heads and roots.
    # This is incompatible with automation (namely, nuclide-core) yet.
    if repo.ui.configbool("smartlog", "collapse-obsolete") and not repo.ui.plain():
        obsnodes = repo.dageval(lambda: nodes & obsolete())
        hidenodes = repo.dageval(lambda: obsnodes - heads(obsnodes) - roots(obsnodes))
        nodes = nodes - hidenodes

    return nodes


@revsetpredicate("interestingbookmarks()")
def interestingheads(repo, subset, x):
    """Set of interesting bookmarks (local and remote)"""
    rev = repo.changelog.rev
    heads = set()
    books = bookmarks.bmstore(repo)
    ignore = re.compile(repo.ui.config("smartlog", "ignorebookmarks", "!"))
    for b in books:
        if not ignore.match(b):
            heads.add(rev(books[b]))

    # add 'interesting' remote bookmarks as well
    if util.safehasattr(repo, "names") and "remotebookmarks" in repo.names:
        ns = repo.names["remotebookmarks"]
        for name in _reposnames(repo.ui):
            nodes = ns.namemap(repo, name)
            if nodes:
                ns.accessed(repo, name)
                heads.add(rev(nodes[0]))

    return subset & smartset.baseset(heads)


@revsetpredicate("interestingmaster()")
def interestingmaster(repo, subset, x):
    """Interesting 'master' commit"""

    names = set(bookmarks.bmstore(repo).keys())
    if util.safehasattr(repo, "names") and "remotebookmarks" in repo.names:
        names.update(set(repo.names["remotebookmarks"].listnames(repo)))

    for name in _reposnames(repo.ui):
        if name in names:
            revs = repo.revs("%s", name)
            break
    else:
        revs = repo.revs("last(public())")

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
    "smartlog|sl|slog|sm|sma|smar|smart|smartl|smartlo",
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

- Your local commits
- The master bookmark for your repository
- Any commits with local bookmarks

Excludes:

- All commits under master that aren't related to your commits
- Your local commits that are older than a specified date"""
    return _smartlog(ui, repo, *pats, **opts)


def getrevs(ui, repo, masterstring, **opts):
    global hiddenchanges
    hiddenchanges = 0

    global commit_info
    commit_info = opts.get("commit_info")

    headrevs = opts.get("rev")
    if headrevs:
        headspec = revsetlang.formatspec("%lr", headrevs)
    else:
        if opts.get("all"):
            datefilter = "_all()"
        else:
            before = ui.config("smartlog", "hide-before")
            if before:
                datefilter = revsetlang.formatspec("date(%s)", ">%s" % before)
            else:
                # last 2 weeks
                datefilter = "date(-14)"
            # Calculate hiddenchanges
            allheads = repo.revs("heads(draft()) - . - interestingbookmarks()")
            visibleheads = repo.revs("%ld & %r", allheads, datefilter)
            hiddenchanges = len(allheads) - len(visibleheads)

        headspec = revsetlang.formatspec(
            "interestingbookmarks() + (heads(draft()) & %r) + .", datefilter
        )

    revstring = revsetlang.formatspec(
        "smartlog(heads=%r, master=%r)", headspec, masterstring
    )

    return set(repo.anyrevs([revstring], user=True))


def _smartlog(ui, repo, *pats, **opts):
    if opts.get("rev"):
        masterfallback = "null"
    else:
        masterfallback = "interestingmaster()"

    masterstring = (
        opts.get("master") or ui.config("smartlog", "master") or masterfallback
    )

    masterrev = repo.anyrevs([masterstring], user=True).first()
    revs = getrevs(ui, repo, masterstring, **opts)

    if -1 in revs:
        revs.remove(-1)

    if len(revs) == 0:
        return

    # Print it!
    revdag, reserved = getdag(ui, repo, sorted(revs, reverse=True), masterrev)
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
    ui.pager("smartlog")
    if ui.config("experimental", "graph.renderer") == "legacy":
        overrides = {}
        if ui.config("experimental", "graphstyle.grandparent", "2.") == "|":
            overrides[("experimental", "graphstyle.grandparent")] = "2."
        with ui.configoverride(overrides, "smartlog"):
            if reserved:
                for prev in reserved:
                    addfakerev(revdag, prev)
            cmdutil.displaygraph(
                ui, repo, revdag, displayer, graphmod.asciiedges, None, None
            )
    else:
        cmdutil.rustdisplaygraph(ui, repo, revdag, displayer, reserved=reserved)

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

    global hiddenchanges
    if hiddenchanges:
        ui.warn(
            _("hiding %s old heads without bookmarks\n") % hiddenchanges,
            notice=_("note"),
        )
        ui.warn(_("(use --all to see them)\n"))
