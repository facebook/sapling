# hiddenoverride.py - lightweight hidden-ness override
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    dispatch,
    error,
    extensions,
    obsolete,
    repoview,
)

def uisetup(ui):
    extensions.wrapfunction(repoview, 'pinnedrevs', pinnedrevs)
    extensions.wrapfunction(dispatch, 'runcommand', runcommand)
    extensions.wrapfunction(obsolete, 'createmarkers', createmarkers)

def pinnedrevs(orig, repo):
    revs = orig(repo)
    nodemap = repo.changelog.nodemap
    pinned = list(nodemap[n] for n in loadpinnednodes(repo) if n in nodemap)
    revs.update(pinned)
    return revs

def loadpinnednodes(repo):
    """yield pinned nodes that should be visible"""
    if repo is None or not repo.local():
        return
    # the "pinned nodes" file name is "obsinhibit" for compatibility reason
    content = repo.svfs.tryread('obsinhibit') or ''
    nodemap = repo.unfiltered().changelog.nodemap
    offset = 0
    while True:
        node = content[offset:offset + 20]
        if not node:
            break
        if node in nodemap:
            yield node
        offset += 20

def savepinnednodes(repo, nodes):
    with repo.svfs.open('obsinhibit', 'wb', atomictemp=True) as f:
        f.write(''.join(nodes))

def runcommand(orig, lui, repo, cmd, fullargs, *args):
    result = orig(lui, repo, cmd, fullargs, *args)
    # after a command completes, make sure working copy parent and all
    # bookmarks get "pinned".
    if repo and repo.local():
        unfi = repo.unfiltered()
        wnode = None
        try:
            # read dirstate directly to avoid dirstate object overhead
            wnode = repo.vfs('dirstate').read(20)
        except Exception:
            pass
        tounpin = getattr(unfi, '_tounpinnodes', set())
        pinned = set(loadpinnednodes(repo)) - tounpin
        if wnode:
            pinned.add(wnode)
        pinned.update(unfi._bookmarks.values())
        savepinnednodes(repo, pinned)
    return result

def createmarkers(orig, repo, rels, *args, **kwargs):
    # this is a way to unpin revs - precursors are unpinned
    unfi = repo.unfiltered()
    tounpin = getattr(unfi, '_tounpinnodes', set())
    for r in rels:
        try:
            tounpin.add(r[0].node())
        except error.RepoLookupError:
            pass
    unfi._tounpinnodes = tounpin
    return orig(repo, rels, *args, **kwargs)
