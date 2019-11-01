# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# hiddenoverride.py - lightweight hidden-ness override

from __future__ import absolute_import

from edenscm.hgext import extutil
from edenscm.mercurial import (
    dispatch,
    error,
    extensions,
    obsolete,
    repoview,
    scmutil,
    util,
    visibility,
)
from edenscm.mercurial.node import short


def uisetup(ui):
    extensions.wrapfunction(repoview, "pinnedrevs", pinnedrevs)
    extensions.wrapfunction(dispatch, "runcommand", runcommand)
    extensions.wrapfunction(obsolete, "createmarkers", createmarkers)
    extensions.wrapfunction(scmutil, "cleanupnodes", cleanupnodes)


def pinnedrevs(orig, repo):
    revs = orig(repo)
    if "visibleheads" not in repo.storerequirements and not visibility.enabled(repo):
        nodemap = repo.changelog.nodemap
        pinnednodes = set(loadpinnednodes(repo))
        tounpin = getattr(repo, "_tounpinnodes", set())
        pinnednodes -= tounpin
        revs.update(nodemap[n] for n in pinnednodes)
    return revs


def loadpinnednodes(repo):
    """yield pinned nodes that are obsoleted and should be visible"""
    if repo is None or not repo.local():
        return
    # the "pinned nodes" file name is "obsinhibit" for compatibility reason
    content = repo.svfs.tryread("obsinhibit") or ""
    unfi = repo.unfiltered()
    nodemap = unfi.changelog.nodemap
    offset = 0
    result = []
    while True:
        node = content[offset : offset + 20]
        if not node:
            break
        if node in nodemap:
            result.append(node)
        offset += 20
    return result


def shouldpinnodes(repo):
    """get nodes that should be pinned: working parent + bookmarks for now"""
    result = set()
    if repo and repo.local():
        # working copy parent
        try:
            wnode = repo.localvfs("dirstate").read(20)
            result.add(wnode)
        except Exception:
            pass
        # bookmarks
        result.update(repo.unfiltered()._bookmarks.values())
    return result


def savepinnednodes(repo, newpin, newunpin, fullargs):
    # take a narrowed lock so it does not affect repo lock
    with extutil.flock(repo.svfs.join("obsinhibit.lock"), "save pinned nodes"):
        orignodes = loadpinnednodes(repo)
        nodes = set(orignodes)
        nodes |= set(newpin)
        nodes -= set(newunpin)
        with util.atomictempfile(repo.svfs.join("obsinhibit")) as f:
            f.write("".join(nodes))

        desc = lambda s: [short(n) for n in s]
        repo.ui.log(
            "pinnednodes",
            "pinnednodes: %r newpin=%r newunpin=%r " "before=%r after=%r\n",
            fullargs,
            desc(newpin),
            desc(newunpin),
            desc(orignodes),
            desc(nodes),
        )


def runcommand(orig, lui, repo, cmd, fullargs, *args):
    # return directly for non-repo command
    if not repo or "visibleheads" in repo.storerequirements:
        return orig(lui, repo, cmd, fullargs, *args)

    shouldpinbefore = shouldpinnodes(repo) | set(loadpinnednodes(repo))
    result = orig(lui, repo, cmd, fullargs, *args)
    # after a command completes, make sure working copy parent and all
    # bookmarks get "pinned".
    newpin = shouldpinnodes(repo) - shouldpinbefore
    newunpin = getattr(repo.unfiltered(), "_tounpinnodes", set())
    # filter newpin by obsolte - ex. if newpin is on a non-obsoleted commit,
    # ignore it.
    if newpin:
        unfi = repo.unfiltered()
        obsoleted = unfi.revs("obsolete()")
        nodemap = unfi.changelog.nodemap
        newpin = set(n for n in newpin if n in nodemap and nodemap[n] in obsoleted)
    # only do a write if something has changed
    if newpin or newunpin:
        savepinnednodes(repo, newpin, newunpin, fullargs)
    return result


def createmarkers(orig, repo, rels, *args, **kwargs):
    # this is a way to unpin revs - precursors are unpinned
    # note: hg debugobsolete does not call this function
    if "visibleheads" not in repo.storerequirements:
        unfi = repo.unfiltered()
        tounpin = getattr(unfi, "_tounpinnodes", set())
        for r in rels:
            try:
                tounpin.add(r[0].node())
            except error.RepoLookupError:
                pass
        unfi._tounpinnodes = tounpin
    return orig(repo, rels, *args, **kwargs)


def cleanupnodes(orig, repo, mapping, *args, **kwargs):
    # this catches cases where cleanupnodes is called but createmarkers is not
    # called. unpin nodes from mapping
    if "visibleheads" not in repo.storerequirements:
        unfi = repo.unfiltered()
        tounpin = getattr(unfi, "_tounpinnodes", set())
        tounpin.update(mapping)
        unfi._tounpinnodes = tounpin
    return orig(repo, mapping, *args, **kwargs)
