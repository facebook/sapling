# debugmutation.py - command processing for debugmutation* commands
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from .. import mutation, node as nodemod, pycompat, registrar, scmutil, util
from ..i18n import _


command = registrar.command()


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
                nodestack.append(list(reversed(preds)))
            ui.status(("\n"))
            while nodestack and not nodestack[-1]:
                nodestack.pop()
    return 0


@command(b"debugmutationfromobsmarkers", [])
def debugmutationfromobsmarkers(ui, repo, **opts):
    """convert obsolescence markers to mutation records"""
    obsmarkers = repo.obsstore._all
    # Sort obsmarkers by date.  Applying them in probable date order gives us
    # a better chance of resolving cycles in the right way.
    obsmarkers.sort(key=lambda x: x[4])
    newmut = {}

    def checkloopfree(pred, succs):
        candidates = {pred}
        while candidates:
            candidate = candidates.pop()
            if candidate in newmut:
                mutpreds, mutsplit, _markers = newmut[candidate]
                for succ in succs:
                    if succ in mutpreds:
                        repo.ui.debug(
                            "ignoring loop: %s -> %s\n  history loops at %s -> %s\n"
                            % (
                                nodemod.hex(pred),
                                ", ".join([nodemod.hex(s) for s in succs]),
                                nodemod.hex(succ),
                                nodemod.hex(candidate),
                            )
                        )
                        return False
                candidates.update(mutpreds)
        return True

    dropprune = 0
    droprevive = 0
    dropundo = 0
    droploop = 0
    dropinvalid = 0

    for obsmarker in obsmarkers:
        obspred, obssuccs, obsflag, obsmeta, obsdate, obsparents = obsmarker
        if not obssuccs:
            # Skip prune markers
            dropprune += 1
            continue
        if obssuccs == (obspred,):
            # Skip revive markers
            droprevive += 1
            continue
        obsmeta = dict(obsmeta)
        if obsmeta.get("operation") in ("undo", "uncommit", "unamend"):
            # Skip undo-style markers
            dropundo += 1
            continue
        if not checkloopfree(obspred, obssuccs):
            # Skip markers that introduce loops
            droploop += 1
            continue
        if len(obssuccs) > 1:
            # Split marker
            succ = obssuccs[-1]
            if succ in newmut:
                preds, split, markers = newmut[succ]
                if obsmarker in markers:
                    # duplicate
                    continue
                repo.ui.debug(
                    "invalid obsmarker found: %s -> %s is both split and folded\n"
                    % (nodemod.hex(obspred), nodemod.hex(succ))
                )
                dropinvalid += 1
                continue
            newmut[succ] = ([obspred], obssuccs[:-1], [obsmarker])
        elif obssuccs[0] in newmut:
            preds, split, markers = newmut[obssuccs[0]]
            if obsmarker in markers:
                # duplicate
                continue
            # Fold marker
            preds.append(obspred)
            markers.append(obsmarker)
        else:
            # Normal marker
            newmut[obssuccs[0]] = ([obspred], None, [obsmarker])

    repo.ui.debug(
        "dropped markers: prune: %s, revive: %s, undo: %s, loop: %s, invalid: %s\n"
        % (dropprune, droprevive, dropundo, droploop, dropinvalid)
    )

    entries = []

    for succ, (preds, split, obsmarkers) in newmut.items():
        if mutation.lookup(repo, succ) is not None:
            # Have already converted this successor, or already know about it
            continue
        mutop = ""
        mutuser = ""
        mutdate = None
        for obsmarker in obsmarkers:
            obspred, obssuccs, obsflag, obsmeta, obsdate, obsparents = obsmarker
            obsmeta = dict(obsmeta)
            obsop = obsmeta.get("operation", "")
            if not mutop or obsop not in ("", "copy"):
                mutop = obsop
            obsuser = obsmeta.get("user", "")
            if not mutuser and obsuser:
                mutuser = obsuser
            if not mutdate:
                mutdate = obsdate

        entries.append(
            mutation.createsyntheticentry(
                repo,
                mutation.ORIGIN_OBSMARKER,
                preds,
                succ,
                mutop,
                split,
                mutuser,
                mutdate,
            )
        )

    repo.ui.write(
        _("generated %s entries for %s commits\n") % (len(entries), len(newmut))
    )
    with repo.lock():
        count = mutation.recordentries(repo, entries, skipexisting=False)
        repo.ui.write(_("wrote %s entries\n") % count)
