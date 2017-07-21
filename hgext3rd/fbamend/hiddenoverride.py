# hiddenoverride.py - lightweight hidden-ness override
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import os

from mercurial.node import short
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
    pinnednodes = set(loadpinnednodes(repo))
    tounpin = getattr(repo, '_tounpinnodes', set())
    pinnednodes -= tounpin
    revs.update(nodemap[n] for n in pinnednodes)
    return revs

def loadpinnednodes(repo):
    """yield pinned nodes that are obsoleted and should be visible"""
    if repo is None or not repo.local():
        return
    # the "pinned nodes" file name is "obsinhibit" for compatibility reason
    content = repo.svfs.tryread('obsinhibit') or ''
    unfi = repo.unfiltered()
    nodemap = unfi.changelog.nodemap
    offset = 0
    result = []
    while True:
        node = content[offset:offset + 20]
        if not node:
            break
        # remove unnecessary (non-obsoleted) nodes since pinnedrevs should only
        # affect obsoleted revs.
        if node in nodemap and unfi[node].obsolete():
            result.append(node)
        offset += 20
    return result

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

def savepinnednodes(repo, newpin, newunpin, fullargs):
    # take a narrowed lock so it does not affect repo lock
    with flock(repo.svfs.join('obsinhibit.lock')):
        orignodes = loadpinnednodes(repo)
        nodes = set(orignodes)
        nodes |= set(newpin)
        nodes -= set(newunpin)
        with util.atomictempfile(repo.svfs.join('obsinhibit')) as f:
            f.write(''.join(nodes))

        desc = lambda s: [short(n) for n in s]
        repo.ui.log('pinnednodes', 'pinnednodes: %r newpin=%r newunpin=%r '
                    'before=%r after=%r\n', fullargs, desc(newpin),
                    desc(newunpin), desc(orignodes), desc(nodes))

def runcommand(orig, lui, repo, cmd, fullargs, *args):
    # return directly for non-repo command
    if not repo:
        return orig(lui, repo, cmd, fullargs, *args)

    shouldpinbefore = shouldpinnodes(repo) | set(loadpinnednodes(repo))
    result = orig(lui, repo, cmd, fullargs, *args)
    # after a command completes, make sure working copy parent and all
    # bookmarks get "pinned".
    newpin = shouldpinnodes(repo) - shouldpinbefore
    newunpin = getattr(repo.unfiltered(), '_tounpinnodes', set())
    # filter newpin by obsolte - ex. if newpin is on a non-obsoleted commit,
    # ignore it.
    unfi = repo.unfiltered()
    newpin = set(n for n in newpin if unfi[n].obsolete())
    # only do a write if something has changed
    if newpin or newunpin:
        savepinnednodes(repo, newpin, newunpin, fullargs)
    return result

def createmarkers(orig, repo, rels, *args, **kwargs):
    # this is a way to unpin revs - precursors are unpinned
    # note: hg debugobsolete does not call this function
    unfi = repo.unfiltered()
    tounpin = getattr(unfi, '_tounpinnodes', set())
    for r in rels:
        try:
            tounpin.add(r[0].node())
        except error.RepoLookupError:
            pass
    unfi._tounpinnodes = tounpin
    return orig(repo, rels, *args, **kwargs)
