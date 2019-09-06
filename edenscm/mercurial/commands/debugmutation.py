# debugmutation.py - command processing for debug commands for mutation and visbility
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .. import mutation, node as nodemod, pycompat, scmutil, util, visibility
from ..i18n import _
from .cmdtable import command


@command(
    b"debugmutation",
    [("s", "successors", False, _("show successors instead of predecessors"))],
    _("[REV]"),
)
def debugmutation(ui, repo, *revs, **opts):
    """display the mutation history (or future) of a commit"""
    repo = repo.unfiltered()
    opts = pycompat.byteskwargs(opts)

    def describe(entry, showsplit=False, showfoldwith=None):
        mutop = entry.op()
        mutuser = entry.user()
        mutdate = util.shortdatetime((entry.time(), entry.tz()))
        mutpreds = entry.preds()
        mutsplit = entry.split() or None
        origin = entry.origin()
        origin = {
            None: "",
            mutation.ORIGIN_LOCAL: "",
            mutation.ORIGIN_COMMIT: " (from remote commit)",
            mutation.ORIGIN_OBSMARKER: " (from obsmarker)",
            mutation.ORIGIN_SYNTHETIC: " (synthetic)",
        }.get(origin, " (unknown origin %s)" % origin)
        extra = ""
        if showsplit and mutsplit is not None:
            extra += " (split into this and: %s)" % ", ".join(
                [nodemod.hex(n) for n in mutsplit]
            )
        if showfoldwith is not None:
            foldwith = [pred for pred in mutpreds if pred != showfoldwith]
            if foldwith:
                extra += " (folded with: %s)" % ", ".join(
                    [nodemod.hex(n) for n in foldwith]
                )
        return ("%s by %s at %s%s%s") % (mutop, mutuser, mutdate, extra, origin)

    def expandhistory(node):
        entry = mutation.lookup(repo, node)
        if entry is not None:
            desc = describe(entry, showsplit=True) + " from:"
            preds = util.removeduplicates(entry.preds())
            return [(desc, preds)]
        else:
            return []

    def expandfuture(node):
        succsets = mutation.lookupsuccessors(repo, node)
        edges = []
        for succset in succsets:
            entry = mutation.lookupsplit(repo, succset[0])
            desc = describe(entry, showfoldwith=node) + " into:"
            edges.append((desc, succset))
        return edges

    expand = expandfuture if opts.get("successors") else expandhistory

    def rendernodes(prefix, nodes):
        if len(nodes) == 1:
            render(prefix, prefix, nodes[0])
        elif len(nodes) > 1:
            for node in nodes[:-1]:
                render(prefix + "|-  ", prefix + "|   ", node)
            render(prefix + "'-  ", prefix + "    ", nodes[-1])

    def render(firstprefix, prefix, nextnode):
        while nextnode:
            node = nextnode
            nextnode = None
            ui.status(("%s%s") % (firstprefix, nodemod.hex(node)))
            firstprefix = prefix
            edges = expand(node)
            if len(edges) == 0:
                ui.status(("\n"))
            elif len(edges) == 1:
                desc, nextnodes = edges[0]
                ui.status((" %s\n") % desc)
                if len(nextnodes) == 1:
                    # Simple case, don't use recursion
                    nextnode = nextnodes[0]
                else:
                    rendernodes(prefix, nextnodes)
            elif len(edges) > 1:
                ui.status((" diverges\n"))
                for edge in edges[:-1]:
                    desc, nextnodes = edge
                    ui.status(("%s:=  %s\n") % (prefix, desc))
                    rendernodes(prefix + ":   ", nextnodes)
                desc, nextnodes = edges[-1]
                ui.status(("%s'=  %s\n") % (prefix, desc))
                rendernodes(prefix + "    ", nextnodes)

    for rev in scmutil.revrange(repo, revs):
        render(" *  ", "    ", repo[rev].node())

    return 0


@command(b"debugmutationfromobsmarkers", [])
def debugmutationfromobsmarkers(ui, repo, **opts):
    """convert obsolescence markers to mutation records"""
    entries, commits, written = mutation.convertfromobsmarkers(repo)
    repo.ui.write(
        _("wrote %s of %s entries for %s commits\n") % (written, entries, commits)
    )
    return 0


@command("debugvisibility", [], subonly=True)
def debugvisibility(ui, repo):
    """control visibility tracking"""


subcmd = debugvisibility.subcommand()


@subcmd("start", [])
def debugvisibilitystart(ui, repo):
    """start tracking commit visibility explicitly"""
    visibility.starttracking(repo)
    return 0


@subcmd("stop", [])
def debugvisibilitystop(ui, repo):
    """stop tracking commit visibility explicitly"""
    visibility.stoptracking(repo)
    return 0
