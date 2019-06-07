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


@command(b"debugmutation", [], _("[REV]"))
def debugmutation(ui, repo, *revs, **opts):
    """display the mutation history of a commit"""
    repo = repo.unfiltered()
    opts = pycompat.byteskwargs(opts)
    for rev in scmutil.revrange(repo, revs):
        ctx = repo[rev]
        nodestack = [[ctx.node()]]
        while nodestack:
            node = nodestack[-1].pop()
            ui.status(("%s%s") % ("  " * len(nodestack), nodemod.hex(node)))
            entry = mutation.lookup(repo, node)
            if entry is not None:
                preds = entry.preds()
                mutop = entry.op()
                mutuser = entry.user()
                mutdate = util.shortdatetime((entry.time(), entry.tz()))
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
                if mutsplit is not None:
                    extra += " (split into this and: %s)" % ", ".join(
                        [nodemod.hex(n) for n in mutsplit]
                    )
                ui.status(
                    (" %s by %s at %s%s%s from:")
                    % (mutop, mutuser, mutdate, extra, origin)
                )

                # Check for duplicate predecessors.  There shouldn't be any.
                if len(preds) != len(set(preds)):
                    predcount = {}
                    for pred in preds:
                        predcount.setdefault(pred, []).append(0)
                    predinfo = [
                        "%s x %s" % (len(c), nodemod.hex(n))
                        for n, c in predcount.iteritems()
                        if len(c) > 1
                    ]
                    ui.status(
                        ("\n%sDUPLICATE PREDECESSORS: %s")
                        % ("  " * len(nodestack), ", ".join(predinfo))
                    )

                    preds = util.removeduplicates(preds)
                nodestack.append(list(reversed(preds)))
            ui.status(("\n"))
            while nodestack and not nodestack[-1]:
                nodestack.pop()
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
