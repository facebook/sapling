# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from edenscm.mercurial import (
    bundlerepo,
    extensions,
    mutation,
    node as nodemod,
    util,
    wireproto,
)
from edenscm.mercurial.commands import debug as debugcommands
from edenscm.mercurial.i18n import _x

from . import constants


def isserver(ui):
    return ui.configbool("infinitepush", "server")


def reposetup(ui, repo):
    repo._scratchbranchmatcher = scratchbranchmatcher(ui)


def extsetup(ui):
    wireproto.commands["knownnodes"] = (wireprotoknownnodes, "nodes *")
    extensions.wrapfunction(debugcommands, "_debugbundle2part", debugbundle2part)
    extensions.wrapfunction(
        bundlerepo.bundlerepository, "_handlebundle2part", bundlerepohandlebundle2part
    )


def wireprotoknownnodes(repo, proto, nodes, others):
    """similar to 'known' but also check in infinitepush storage"""
    nodes = wireproto.decodelist(nodes)
    knownlocally = repo.known(nodes)
    for index, known in enumerate(knownlocally):
        # TODO: make a single query to the bundlestore.index
        if not known and repo.bundlestore.index.getnodebyprefix(
            nodemod.hex(nodes[index])
        ):
            knownlocally[index] = True
    return "".join(b and "1" or "0" for b in knownlocally)


def debugbundle2part(orig, ui, part, all, **opts):
    if part.type == constants.scratchmutationparttype:
        entries = mutation.mutationstore.unbundle(part.read())
        ui.write(_x("    %s entries\n") % len(entries))
        for entry in entries:
            pred = ",".join([nodemod.hex(p) for p in entry.preds()])
            succ = nodemod.hex(entry.succ())
            split = entry.split()
            if split:
                succ = ",".join([nodemod.hex(s) for s in split] + [succ])
            ui.write(
                _x("      %s -> %s (%s by %s at %s)\n")
                % (pred, succ, entry.op(), entry.user(), entry.time())
            )

    orig(ui, part, all, **opts)


def bundlerepohandlebundle2part(orig, self, bundle, part):
    if part.type == constants.scratchmutationparttype:
        entries = mutation.mutationstore.unbundle(part.read())
        self._mutationstore.addbundleentries(entries)
    else:
        orig(self, bundle, part)


class scratchbranchmatcher(object):
    def __init__(self, ui):
        scratchbranchpat = ui.config("infinitepush", "branchpattern")
        if scratchbranchpat:
            _, _, matchfn = util.stringmatcher(scratchbranchpat)
        else:
            matchfn = lambda x: False
        self._matchfn = matchfn

    def match(self, bookmark):
        return self._matchfn(bookmark)
