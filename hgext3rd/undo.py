# undo.py: records data in revlog for future undo functionality
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import (
    dispatch,
    extensions,
    revlog,
)

from mercurial.util import (
    os,
    makedate,
)

from mercurial.node import (
    hex,
    nullid,
)

staticcommand = []

# Wrappers

def _runcommandwrapper(orig, lui, repo, cmd, fullargs, *args):
    # This wrapper executes whenever a command is run.
    # Some commands (eg hg sl) don't actually modify anything
    # ie can't be undone, but the command doesn't know this.
    global staticcommand
    staticcommand = fullargs
    return orig(lui, repo, cmd, fullargs, *args)

# Hooks

def writeloghook(ui, repo, **kwargs):
    nodes = {
        'bookmarks': _logbookmarks(repo),
        'draftheads': _logdraftheads(repo),
        'workingparent': _logworkingparent(repo),
        'date': _logdate(repo),
        'command': _logcommand(repo),
    }
    _logindex(repo, nodes)

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

def _logcommand(repo):
    global staticcommand
    assert staticcommand
    revstring = "\0".join(staticcommand)
    staticcommand = []
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

def reposetup(ui, repo):
    repo.ui.setconfig("hooks", "pretxnclose.undo", writeloghook)

def extsetup(ui):
    extensions.wrapfunction(dispatch, 'runcommand', _runcommandwrapper)
