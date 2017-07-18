# hiddenoverride.py - lightweight hidden-ness override
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import os

from mercurial import (
    dispatch,
    error,
    extensions,
    lock as lockmod,
    obsolete,
    repoview,
    util,
    vfs as vfsmod,
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

def shouldpinnodes(repo):
    """get nodes that should be pinned: working parent + bookmarks for now"""
    result = set()
    if repo and repo.local():
        # working copy parent
        try:
            wnode = repo.vfs('dirstate').read(20)
            result.add(wnode)
        except Exception:
            pass
        # bookmarks
        result.update(repo._bookmarks.values())
    return result

@contextlib.contextmanager
def flock(lockpath):
    # best effort lightweight lock
    try:
        import fcntl
        fcntl.flock
    except ImportError:
        # fallback to Mercurial lock
        vfs = vfsmod.vfs(os.path.dirname(lockpath))
        with lockmod.lock(vfs, os.path.basename(lockpath)):
            yield
        return
    # make sure lock file exists
    util.makedirs(os.path.dirname(lockpath))
    with open(lockpath, 'a'):
        pass
    lockfd = os.open(lockpath, os.O_RDONLY | os.O_CREAT, 0o664)
    fcntl.flock(lockfd, fcntl.LOCK_EX)
    try:
        yield
    finally:
        fcntl.flock(lockfd, fcntl.LOCK_UN)
        os.close(lockfd)

def savepinnednodes(repo, newpin, newunpin):
    # take a narrowed lock so it does not affect repo lock
    with flock(repo.svfs.join('obsinhibit.lock')):
        nodes = set(loadpinnednodes(repo))
        nodes |= set(newpin)
        nodes -= set(newunpin)
        with util.atomictempfile(repo.svfs.join('obsinhibit')) as f:
            f.write(''.join(nodes))

def runcommand(orig, lui, repo, cmd, fullargs, *args):
    shouldpinbefore = shouldpinnodes(repo)
    result = orig(lui, repo, cmd, fullargs, *args)
    # after a command completes, make sure working copy parent and all
    # bookmarks get "pinned".
    newpin = shouldpinnodes(repo) - shouldpinbefore
    newunpin = getattr(repo, '_tounpinnodes', set())
    # only do a write if something has changed
    if newpin or newunpin:
        savepinnednodes(repo, newpin, newunpin)
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
