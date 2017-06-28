# undo.py: records data in revlog for future undo functionality
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import
import os

from mercurial import (
    dispatch,
    extensions,
    revlog,
)

from mercurial.util import (
    makedate,
)

from mercurial.node import (
    hex,
    nullid,
)

# Wrappers

def _runcommandwrapper(orig, lui, repo, cmd, fullargs, *args):
    # This wrapper executes whenever a command is run.
    # Some commands (eg hg sl) don't actually modify anything
    # ie can't be undone, but the command doesn't know this.
    command = fullargs

    # Check wether undolog is consistent
    # ie check wether the undo ext was
    # off before this command
    safelog(repo, "")

    result = orig(lui, repo, cmd, fullargs, *args)

    # record changes to repo
    safelog(repo, command)
    return result

# Log Control

def safelog(repo, command):
    if repo is not None:# some hg commands don't require repo
        with repo.lock():
            with repo.transaction("undolog"):
                log(repo, command)

def log(repo, command):
    newnodes = {
        'bookmarks': _logbookmarks(repo),
        'draftheads': _logdraftheads(repo),
        'workingparent': _logworkingparent(repo),
    }
    exsistingnodes = _readindex(repo, 0)
    if all(newnodes.get(x) == exsistingnodes.get(x) for x in newnodes.keys()):
        return
    else:
        newnodes.update({
            'date': _logdate(repo),
            'command': _logcommand(repo, command),
        })
        _logindex(repo, newnodes)

# Logs

def writelog(repo, name, revstring):
    assert repo.currenttransaction() is not None
    # The transaction code doesn't work with vfs
    # specifically, repo.recover() assumes svfs?
    repo.svfs.makedirs('undolog')
    path = os.path.join('undolog', name)
    rlog = revlog.revlog(repo.svfs, path)
    tr = repo.currenttransaction()
    node = rlog.addrevision(revstring, tr, 1, nullid, nullid)
    return hex(node)

def _logdate(repo):
    revstring = " ".join(str(x) for x in makedate())
    return writelog(repo, "date.i", revstring)

def _logdraftheads(repo):
    revs = repo.revs('heads(draft())')
    tonode = repo.changelog.node
    hexnodes = [hex(tonode(x)) for x in revs]
    revstring = "\n".join(sorted(hexnodes))
    return writelog(repo, "draftheads.i", revstring)

def _logcommand(repo, command):
    revstring = "\0".join(command)
    return writelog(repo, "command.i", revstring)

def _logbookmarks(repo):
    revstring = "\n".join(sorted('%s %s' % (name, hex(node))
        for name, node in repo._bookmarks.iteritems()))
    return writelog(repo, "bookmarks.i", revstring)

def _logworkingparent(repo):
    revstring = repo['.'].hex()
    return writelog(repo, "workingparent.i", revstring)

def _logindex(repo, nodes):
    revstring = "\n".join(sorted('%s %s' % (k, v) for k, v in nodes.items()))
    return writelog(repo, "index.i", revstring)

# Setup

def extsetup(ui):
    extensions.wrapfunction(dispatch, 'runcommand', _runcommandwrapper)

def _readindex(repo, reverseindex, prefetchedrevlog=None):
    if prefetchedrevlog is None:
        path = os.path.join('undolog', 'index.i')
        rlog = revlog.revlog(repo.svfs, path)
    else:
        rlog = prefetchedrevlog
    index = len(rlog) - reverseindex - 1
    # before time
    if index < 0:
        return {}
    # in the future
    if index > len(rlog) - 1:
        raise IndexError
    chunk = rlog.revision(index)
    indexdict = {}
    for row in chunk.split("\n"):
        kvpair = row.split(' ', 1)
        if kvpair[0]:
            indexdict[kvpair[0]] = kvpair[1]
    return indexdict
