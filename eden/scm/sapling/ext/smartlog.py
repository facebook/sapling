# Copyright (c) Meta Platforms, Inc. and affiliates.
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

    # Default parameter for master
    master = remote/master

    # Collapse obsoleted commits
    collapse-obsolete = True
"""

from __future__ import absolute_import

import datetime
import re
import time

import bindings

from sapling import (
    bookmarks,
    cmdutil,
    commands,
    dagop,
    error,
    extensions,
    graphmod,
    node as nodemod,
    phases,
    pycompat,
    registrar,
    revset,
    revsetlang,
    smartset,
    templater,
    util,
)
from sapling.i18n import _
from sapling.pycompat import range

if not pycompat.iswindows:
    from . import interactiveui
else:
    interactiveui = None
from . import rebase

cmdtable = {}
command = registrar.command(cmdtable)
revsetpredicate = registrar.revsetpredicate()

testedwith = "ships-with-fb-ext"

# Remove unsupported --limit option.
logopts = [opt for opt in commands.logopts if opt[1] != "limit"]

configtable = {}
configitem = registrar.configitem(configtable)

configitem("smartlog", "collapse-obsolete", default=True)
configitem("smartlog", "max-commit-threshold", default=1000)


templatekeyword = registrar.templatekeyword()
templatefunc = registrar.templatefunc()


@templatekeyword("shelveenabled")
def shelveenabled(repo, ctx, **args):
    """Bool. Return true if shelve extension is enabled"""
    return "shelve" in extensions.enabled().keys()


def getdag(ui, repo, revs, masterstring, template):

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

    simplifygrandparents = ui.configbool("log", "simplify-grandparents")
    cl = repo.changelog
    if cl.algorithmbackend != "segments":
        simplifygrandparents = False
    if simplifygrandparents:
        rootnodes = cl.tonodes(revs)

    masterrev = repo.anyrevs([masterstring], user=True).first()
    firstbranch = []
    if masterrev is not None:
        firstbranch.append(masterrev)
    revs = repo.revs("sort(%ld,topo,topo.firstbranch=%ld)", revs, firstbranch)
    ctxstream = revs.prefetchbytemplate(repo, template).iterctx()

    # For each rev we need to show, compute it's parents in the dag.
    # If we have to reach for a grandparent, insert a fake node so we
    # can show '...' in the graph.
    # Use 'reversed' to start at the lowest commit so fake nodes are
    # placed at their lowest possible positions.
    for ctx in ctxstream:
        rev = ctx.rev()
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
                if simplifygrandparents:
                    gp = gpcache[mpar] = cl.torevs(
                        cl.dageval(
                            lambda: headsancestors(
                                ancestors(cl.tonodes([mpar])) & rootnodes
                            )
                        )
                    )
                else:
                    gp = gpcache[mpar] = dagop.reachableroots(repo, revs, [mpar])
            if not gp:
                parents.append((graphmod.MISSINGPARENT, mpar))
            else:
                gp = [g for g in gp if g not in unzip(parents)]
                for g in gp:
                    parents.append((graphmod.GRANDPARENT, g))

        results.append((ctx.rev(), "C", ctx, parents))

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
    Changesets relevant to you.

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
    masterset -= smartset.baseset([nodemod.nullrev], repo=repo)
    if nodemod.nullrev in heads:
        heads.remove(nodemod.nullrev)

    cl = repo.changelog
    if cl.algorithmbackend == "segments":
        heads = cl.tonodes(heads)
        master = cl.tonodes(masterset)
        nodes = smartlognodes(repo, heads, master)
        return subset & smartset.idset(cl.torevs(nodes), repo=repo)

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
    revs = smartset.baseset(
        repo.revs("ancestor(%ld & public()) + %ld", revs, revs), repo=repo
    )

    # Collapse long obsoleted stack - only keep their heads and roots.
    # This is incompatible with automation (namely, nuclide-core) yet.
    if repo.ui.configbool("smartlog", "collapse-obsolete") and not repo.ui.plain():
        obsrevs = smartset.baseset(repo.revs("%ld & obsolete()", revs), repo=repo)
        hiderevs = smartset.baseset(
            repo.revs("%ld - (heads(%ld) + roots(%ld))", obsrevs, obsrevs, obsrevs),
            repo=repo,
        )
        revs = repo.revs("(%ld - %ld) + %ld", revs, hiderevs, heads)

    return subset & revs


@revsetpredicate("draftbranch([set])")
def draftbranchrevset(repo, subset, x):
    """``draftbranch(set)``
    The draft branches containing the given changesets.
    """
    args = revset.getargs(x, 1, 1, _("draftbranch expects one argument"))
    revs = revset.getset(repo, subset, args[0])
    return subset & repo.revs("(draft() & ::%ld)::", revs)


@revsetpredicate("mutrelated([set])")
def mutrelatedrevset(repo, subset, x):
    """``mutrelated([set])``
    Draft changesets that are related via mutations.
    """
    args = revset.getargs(x, 1, 1, _("mutrelated expects one argument"))
    revs = revset.getset(repo, subset, args[0])
    return subset & repo.revs(
        "descendants((predecessors(%ld) + successors(%ld)) & not public())", revs, revs
    )


@revsetpredicate("focusedbranch([set])")
def focusedbranchrevset(repo, subset, x):
    """``focusedbranch([set])``
    The focused branches of the given changesets, being the draft
    stack and any draft changesets that are related via mutations.
    """
    args = revset.getargs(x, 1, 1, _("focusedbranch expects one argument"))
    revs = revset.getset(repo, subset, args[0])
    return subset & repo.revs(
        "draft() & mutrelated(draftbranch(%ld)) + %ld", revs, revs
    )


def smartlognodes(repo, headnodes, masternodes):
    """Calculate nodes based on new DAG abstraction.
    This function does not use revs or revsets.
    """
    # Use "- public()" so if the user specifies secret commits in headnodes
    # it will work as expected.
    draftnodes = repo.dageval(lambda: ancestors(headnodes) - public())
    nodes = repo.dageval(
        lambda: parents(draftnodes) | draftnodes | headnodes | masternodes
    )

    # Protection to avoid running into "very slow" cases. This does not
    # usually happen. But wrong visibleheads might trigger it (ex. large
    # draft() size). Note this is tested before "collapse-obsolete", because
    # "collapse-obsolete" can be very slow if there are too many nodes.
    limit = repo.ui.configint("smartlog", "max-commit-threshold")
    if len(nodes) > limit:
        repo.ui.warn(
            _("smartlog: too many (%d) commits, not rendering all of them\n")
            % (len(nodes),)
        )
        repo.ui.warn(
            _("(consider running '@prog@ doctor' to hide unrelated commits)\n")
        )
        nodes = nodes.take(limit) + headnodes

    # Include the ancestor of above commits to make the graph connected.
    nodes = repo.dageval(lambda: nodes | filter(None, [gcaone(nodes)]))

    # Collapse long obsoleted stack - only keep their heads and roots.
    # This is incompatible with automation (namely, nuclide-core) yet.
    if repo.ui.configbool("smartlog", "collapse-obsolete") and not repo.ui.plain():
        obsnodes = repo.dageval(lambda: nodes & obsolete())
        hidenodes = repo.dageval(lambda: obsnodes - heads(obsnodes) - roots(obsnodes))
        nodes = nodes - hidenodes + headnodes

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
    if hasattr(repo, "names") and "remotebookmarks" in repo.names:
        ns = repo.names["remotebookmarks"]
        for name in _reposnames(repo.ui):
            nodes = ns.namemap(repo, name)
            if nodes:
                heads.add(rev(nodes[0]))

    return subset & smartset.baseset(heads, repo=repo)


@revsetpredicate("interestingmaster()")
def interestingmaster(repo, subset, x):
    """Interesting 'master' commit"""

    names = set(bookmarks.bmstore(repo).keys())
    if hasattr(repo, "names") and "remotebookmarks" in repo.names:
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
    date_now = util.parsedate("now")
    date = templater.evalfuncarg(context, mapping, args[0])
    tz = None
    if len(args) == 2:
        tzname = templater.evalstring(context, mapping, args[1])
        if tzname:
            try:
                import zoneinfo

                tz = zoneinfo.ZoneInfo(tzname)
            except ImportError:
                msg = "Couldn't import ZoneInfo, using default time zone\n"
                repo.ui.warn(msg)
            except zoneinfo._common.ZoneInfoNotFoundError:
                msg = "Unknown time zone: %s\n" % tzname
                repo.ui.warn(msg)
    then = datetime.datetime.fromtimestamp(date[0], tz)
    now = datetime.datetime.fromtimestamp(date_now[0], tz)
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
        ("", "all", False, _("don't hide old local changesets")),
        ("", "commit-info", False, _("show changes in current changeset")),
        (
            "i",
            "interactive",
            False,
            _("enter command line interactive smartlog (EXPERIMENTAL)"),
        ),
    ]
    + logopts,
    _("[OPTION]... [[-r] REV]"),
    cmdtype=registrar.command.readonly,
)
def smartlog(ui, repo, *pats, **opts):
    """show a graph of the commits that are relevant to you

    Includes:

    - Your local commits
    - The master bookmark for your repository
    - Any commits with local bookmarks

    Excludes:

    - All commits under master that aren't related to your commits
    - Your local commits that are older than a specified date

    ``--commit-info`` shows status-like file changes for the current commit, or
    all drafts when using ``--verbose``. Note this is not the working copy
    ``status`` and only works with default templates.

    ``--stat` shows ``diff --stat``-like file changes for all drafts.
    """
    overrides = {}
    if opts.get("commit_info"):
        if ui.verbose:
            config = "'true'"
        else:
            config = "\"{ifeq(graphnode, '@', 'true')}\""
        overrides[("templatealias", "sl_show_file_change_summary")] = config
    with ui.configoverride(overrides):
        return _smartlog(ui, repo, *pats, **opts)


def getrevs(ui, repo, masterstring, headrevs):
    if headrevs:
        headspec = revsetlang.formatspec("%lr", headrevs)
    else:
        headspec = "interestingbookmarks() + heads(draft()) + ."

    revstring = revsetlang.formatspec(
        "smartlog(heads=%r, master=%r)", headspec, masterstring
    )

    revs = set(repo.anyrevs([revstring], user=True))

    if -1 in revs:
        revs.remove(-1)

    return revs


if interactiveui is not None:

    class interactivesmartlog(interactiveui.viewframe):
        def __init__(self, ui, repo, masterstring, headrevs, template, opts):
            super().__init__(ui, repo)
            self.masterstring = masterstring
            self.template = template
            self.headrevs = headrevs
            self.opts = opts
            self.dag_index = 0
            self._compute_graph()
            self.status = ""
            self.rebase_source = None

        # Compute new index of ctx in revdag
        # TODO: Handle case where ctx is no longer in revdag. Currently should be impossible but could happen if we change what the selected_node is when rebasing.
        def _new_index(self, ctx):
            return [node[2] for node in self.revdag].index(ctx)

        def _compute_graph(self):
            revs = getrevs(self.ui, self.repo, self.masterstring, self.headrevs)
            if len(revs) == 0:
                self.finish()
            self.revdag, self.reserved = getdag(
                self.ui, self.repo, sorted(revs), self.masterstring, self.template
            )

        def render(self):
            ui = self.ui
            ui.pushbuffer()
            # Print it!
            displayer = cmdutil.show_changeset(
                self.ui, self.repo, self.opts, buffered=True
            )

            current_line = 0
            selected_rows = None

            def on_output(ctx, output):
                nonlocal current_line
                nonlocal selected_rows
                height = output.count("\n")
                selected_ctx = self.revdag[self.dag_index][2]
                if ctx == selected_ctx:
                    # start and end indices (inclusive)
                    selected_rows = (current_line, current_line + height - 1)
                current_line += height

            cmdutil.displaygraph(
                self.ui,
                self.repo,
                self.revdag,
                displayer,
                reserved=self.reserved,
                props={"highlighted_node": self.revdag[self.dag_index][2].hex()},
                on_output=on_output,
            )
            ui.status(self.status)
            output = ui.popbuffer().splitlines()
            if selected_rows is None:
                return output, None
            return output, (selected_rows[1], interactiveui.Alignment.bottom)

        def handlekeypress(self, key):
            if key == self.KEY_Q:
                self.finish()
            if key == self.KEY_J:
                if self.dag_index < len(self.revdag) - 1:
                    self.dag_index += 1
            if key == self.KEY_K:
                if self.dag_index > 0:
                    self.dag_index -= 1
            if key == self.KEY_RETURN:
                self.ui.pushbuffer(error=True)
                selected_ctx = self.revdag[self.dag_index][2]
                try:
                    if self.rebase_source is None:
                        # Equivalent to `hg update selected_ctx`
                        commands.update(self.ui, self.repo, selected_ctx.hex())
                    else:
                        # Equivalent to `hg rebase -s self.rebase_source -d selected_ctx`
                        rebase.rebase(
                            self.ui,
                            self.repo,
                            source=self.rebase_source[2].hex(),
                            dest=selected_ctx.hex(),
                        )
                        self.rebase_source = None
                        self._compute_graph()
                        self.dag_index = self._new_index(selected_ctx)
                except Exception as ex:
                    self.ui.write_err("operation failed: %s\n" % ex)
                self.status = self.ui.popbuffer()
            if key == self.KEY_R:
                self.rebase_source = self.revdag[self.dag_index]
                self.status = _("rebasing from %s") % (self.rebase_source[2])
            if key == self.KEY_S:
                bindings.commands.run(
                    util.hgcmd()
                    + [
                        "show",
                        self.revdag[self.dag_index][2].hex(),
                        "--config",
                        "pager.interface=fullscreen",
                    ]
                )


def _smartlog(ui, repo, *pats, **opts):
    masterfallback = "interestingmaster()"

    masterstring = (
        opts.get("master") or ui.config("smartlog", "master") or masterfallback
    )

    template = opts.get("template") or ""
    headrevs = opts.get("rev")

    if opts.get("interactive"):
        if interactiveui is None:
            raise error.Abort(_("interactive ui is not supported on Windows"))

        ui.write_err(_("warning: interactive mode is WIP\n"))

        viewobj = interactivesmartlog(ui, repo, masterstring, headrevs, template, opts)
        interactiveui.view(viewobj)
        return

    revs = getrevs(ui, repo, masterstring, headrevs)

    if len(revs) == 0:
        return
    # Print it!
    revdag, reserved = getdag(ui, repo, sorted(revs), masterstring, template)
    displayer = cmdutil.show_changeset(ui, repo, opts, buffered=True)
    ui.pager("smartlog")
    cmdutil.displaygraph(ui, repo, revdag, displayer, reserved=reserved)

    try:
        with open(repo.localvfs.join("completionhints"), "w+") as f:
            for rev in revdag:
                commit_hash = rev[2].node()
                f.write(nodemod.short(commit_hash) + "\n")
    except IOError:
        # No write access. No big deal.
        pass
