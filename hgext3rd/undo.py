# undo.py: records data in revlog for future undo functionality
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import
import os

from mercurial.i18n import _

from mercurial import (
    dispatch,
    error,
    extensions,
    registrar,
    revlog,
    util,
)

from mercurial.node import (
    bin,
    hex,
    nullid,
)

cmdtable = {}
command = registrar.command(cmdtable)

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
    try:
        exsistingnodes = _readindex(repo, 0)
    except IndexError:
        exsistingnodes = {}
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
    revstring = " ".join(str(x) for x in util.makedate())
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

# Visualize

"""debug commands and instrumentation for the undo extension

Adds the `debugundohistory` and `debugundosmartlog` commands to visualize
operational history and to give a preview of how undo will behave.
"""

@command('debugundohistory', [
    ('n', 'index', 0, _("details about specific operation")),
    ('l', 'list', False, _("list recent undo-able operation"))
])
def debugundohistory(ui, repo, *args, **kwargs):
    """ Print operational history
        0 is the most recent operation
    """
    if repo is not None:
        if kwargs.get('list'):
            if args and args[0].isdigit():
                offset = int(args[0])
            else:
                offset = 0
            _debugundolist(ui, repo, offset)
        else:
            reverseindex = kwargs.get('index')
            if 0 == reverseindex and args and args[0].isdigit():
                reverseindex = int(args[0])
            _debugundoindex(ui, repo, reverseindex)

def _debugundolist(ui, repo, offset):
    offset = abs(offset)

    template = "{sub('\0', ' ', undo)}\n"
    fm = ui.formatter('debugundohistory', {'template': template})
    path = os.path.join('undolog', 'index.i')

    prefetchedrevlog = revlog.revlog(repo.svfs, path)
    recentrange = min(5, len(prefetchedrevlog) - offset)
    if 0 == recentrange:
        fm.startitem()
        fm.write('undo', '%s', "None")
    for i in range(recentrange):
        nodedict = _readindex(repo, i + offset, prefetchedrevlog)
        commandstr = _readnode(repo, 'command.i', nodedict['command'])
        fm.startitem()
        fm.write('undo', '%s', str(i + offset) + ": " + commandstr)
    fm.end()

def _debugundoindex(ui, repo, reverseindex):
    try:
        nodedict = _readindex(repo, reverseindex)
    except IndexError:
        raise error.Abort(_("index out of bounds"))
        return
    template = "{tabindent(sub('\0', ' ', content))}\n"
    fm = ui.formatter('debugundohistory', {'template': template})
    cabinet = ('command.i', 'bookmarks.i', 'date.i',
            'draftheads.i', 'workingparent.i')
    for filename in cabinet:
        header = filename[:-2] + ":\n"
        rawcontent = _readnode(repo, filename, nodedict[filename[:-2]])
        if "date.i" == filename:
            splitdate = rawcontent.split(" ")
            datetuple = (float(splitdate[0]), int(splitdate[1]))
            content = util.datestr(datetuple)
        elif "draftheads.i" == filename:
            try:
                oldnodes = _readindex(repo, reverseindex + 1)
                oldheads = _readnode(repo, filename, oldnodes[filename[:-2]])
            except IndexError:# Index is oldest log
                content = rawcontent
            else:
                content = "ADDED:\n\t" + "\n\t".join(sorted(
                        set(rawcontent.split("\n"))
                        - set(oldheads.split("\n"))
                        ))
                content += "\nREMOVED:\n\t" + "\n\t".join(sorted(
                        set(oldheads.split("\n"))
                        - set(rawcontent.split("\n"))
                        ))
        elif "command.i" == filename and "" == rawcontent:
            content = "unkown command(s) run, gap in log"
        else:
            content = rawcontent
        fm.startitem()
        fm.write('content', '%s', header + content)
    fm.end()

# Read

def _readindex(repo, reverseindex, prefetchedrevlog=None):
    if prefetchedrevlog is None:
        path = os.path.join('undolog', 'index.i')
        rlog = revlog.revlog(repo.svfs, path)
    else:
        rlog = prefetchedrevlog
    index = len(rlog) - reverseindex - 1
    if index < 0 or index > len(rlog) - 1:
        raise IndexError
    chunk = rlog.revision(index)
    indexdict = {}
    for row in chunk.split("\n"):
        kvpair = row.split(' ', 1)
        if kvpair[0]:
            indexdict[kvpair[0]] = kvpair[1]
    return indexdict

def _readnode(repo, filename, hexnode):
    path = os.path.join('undolog', filename)
    rlog = revlog.revlog(repo.svfs, path)
    return rlog.revision(bin(hexnode))

# Setup

def extsetup(ui):
    extensions.wrapfunction(dispatch, 'runcommand', _runcommandwrapper)
