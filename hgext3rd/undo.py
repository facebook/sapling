# undo.py: records data in revlog for future undo functionality
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial.i18n import _

from mercurial import (
    dispatch,
    error,
    extensions,
    localrepo,
    lock as lockmod,
    registrar,
    revlog,
    revset,
    revsetlang,
    smartset,
    transaction,
    util,
)

from mercurial.node import (
    bin,
    hex,
    nullid,
)

cmdtable = {}
command = registrar.command(cmdtable)

# Setup

def extsetup(ui):
    extensions.wrapfunction(dispatch, 'runcommand', _runcommandwrapper)

    # undo has its own locking, whitelist itself to bypass repo lock audit
    localrepo.localrepository._wlockfreeprefix.add('undolog/')

# Wrappers

def _runcommandwrapper(orig, lui, repo, cmd, fullargs, *args):
    # This wrapper executes whenever a command is run.
    # Some commands (eg hg sl) don't actually modify anything
    # ie can't be undone, but the command doesn't know this.
    command = fullargs

    # Check wether undolog is consistent
    # ie check wether the undo ext was
    # off before this command
    safelog(repo, [""])

    result = orig(lui, repo, cmd, fullargs, *args)

    # record changes to repo
    safelog(repo, command)
    return result

# Write: Log control

def safelog(repo, command):
    '''boilerplate for log command

    input:
        repo: mercurial.localrepo
        command: list of strings, first is string of command run
    output: bool
        True if changes have been recorded, False otherwise
    '''
    changes = False
    if repo is not None:# some hg commands don't require repo
        # undolog specific lock
        # allows running command during other commands when
        # otherwise legal.  Could cause weird undolog states,
        # which gap handling generally covers.
        repo.vfs.makedirs('undolog')
        with lockmod.lock(repo.vfs, "undolog/lock", desc="undolog"):
            # developer config: undo._duringundologlock
            if repo.ui.configbool('undo', '_duringundologlock'):
                repo.hook("duringundologlock")
            tr = lighttransaction(repo)
            with tr:
                changes = log(repo.filtered('visible'), command, tr)
    return changes

def lighttransaction(repo):
    # full fledged transactions have two serious issues:
    # 1. they may cause infite loops through hooks
    #    that run commands
    # 2. they are really expensive performance wise
    #
    # ligtthransaction avoids certain hooks from being
    # executed, doesn't check repo locks, doesn't check
    # abandoned tr's (since we only record info) and doesn't
    # do any tag handling
    vfsmap = {'plain': repo.vfs}
    tr = transaction.transaction(repo.ui.warn, repo.vfs, vfsmap,
                                 "undolog/tr.journal", "undolog/tr.undo")
    return tr

def log(repo, command, tr):
    '''logs data neccesary for undo if repo state has changed

    input:
        repo: mercurial.localrepo
        command: los, first is command to be recorded as run
        tr: transaction
    output: bool
        True if changes recorded
        False if no changes to record
    '''
    newnodes = {
        'bookmarks': _logbookmarks(repo, tr),
        'draftheads': _logdraftheads(repo, tr),
        'workingparent': _logworkingparent(repo, tr),
    }
    try:
        exsistingnodes = _readindex(repo, 0)
    except IndexError:
        exsistingnodes = {}
    if all(newnodes.get(x) == exsistingnodes.get(x) for x in newnodes.keys()):
        # no changes to record
        return False
    else:
        newnodes.update({
            'date': _logdate(repo, tr),
            'command': _logcommand(repo, tr, command),
        })
        _logindex(repo, tr, newnodes)
        # changes have been recorded
        return True

# Write: Logs

def writelog(repo, tr, name, revstring):
    if tr is None:
        raise error.ProgrammingError
    rlog = _getrevlog(repo, name)
    node = rlog.addrevision(revstring, tr, 1, nullid, nullid)
    return hex(node)

def _logdate(repo, tr):
    revstring = " ".join(str(x) for x in util.makedate())
    return writelog(repo, tr, "date.i", revstring)

def _logdraftheads(repo, tr):
    revs = repo.revs('heads(draft())')
    tonode = repo.changelog.node
    hexnodes = [hex(tonode(x)) for x in revs]
    revstring = "\n".join(sorted(hexnodes))
    return writelog(repo, tr, "draftheads.i", revstring)

def _logcommand(repo, tr, command):
    revstring = "\0".join(command)
    return writelog(repo, tr, "command.i", revstring)

def _logbookmarks(repo, tr):
    revstring = "\n".join(sorted('%s %s' % (name, hex(node))
        for name, node in repo._bookmarks.iteritems()))
    return writelog(repo, tr, "bookmarks.i", revstring)

def _logworkingparent(repo, tr):
    revstring = repo['.'].hex()
    return writelog(repo, tr, "workingparent.i", revstring)

def _logindex(repo, tr, nodes):
    revstring = "\n".join(sorted('%s %s' % (k, v) for k, v in nodes.items()))
    return writelog(repo, tr, "index.i", revstring)

# Read

def _readindex(repo, reverseindex, prefetchedrevlog=None):
    if prefetchedrevlog is None:
        rlog = _getrevlog(repo, 'index.i')
    else:
        rlog = prefetchedrevlog
    index = _invertindex(rlog, reverseindex)
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
    rlog = _getrevlog(repo, filename)
    return rlog.revision(bin(hexnode))

# Visualize

"""debug commands and instrumentation for the undo extension

Adds the `debugundohistory` and `debugundosmartlog` commands to visualize
operational history and to give a preview of how undo will behave.
"""

@command('debugundohistory', [
    ('n', 'index', 0, _("details about specific operation")),
    ('l', 'list', False, _("list recent undo-able operation"))
])
def debugundohistory(ui, repo, *args, **opts):
    """ Print operational history
        0 is the most recent operation
    """
    if repo is not None:
        if opts.get('list'):
            if args and args[0].isdigit():
                offset = int(args[0])
            else:
                offset = 0
            _debugundolist(ui, repo, offset)
        else:
            reverseindex = opts.get('index')
            if 0 == reverseindex and args and args[0].isdigit():
                reverseindex = int(args[0])
            _debugundoindex(ui, repo, reverseindex)

def _debugundolist(ui, repo, offset):
    offset = abs(offset)

    template = "{sub('\0', ' ', undo)}\n"
    fm = ui.formatter('debugundohistory', {'template': template})
    prefetchedrevlog = _getrevlog(repo, 'index.i')
    recentrange = min(5, len(prefetchedrevlog) - offset)
    if 0 == recentrange:
        fm.startitem()
        fm.write('undo', '%s', "None")
    for i in range(recentrange):
        nodedict = _readindex(repo, i + offset, prefetchedrevlog)
        commandstr = _readnode(repo, 'command.i', nodedict['command'])
        if "" == commandstr:
            commandstr = " -- gap in log -- "
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
            except IndexError: # index is oldest log
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

# Revset logic

def _getolddrafts(repo, reverseindex):
    nodedict = _readindex(repo, reverseindex)
    olddraftheads = _readnode(repo, "draftheads.i", nodedict["draftheads"])
    oldheadslist = olddraftheads.split("\n")
    oldlogrevstring = revsetlang.formatspec('draft() & ancestors(%ls)',
            oldheadslist)
    urepo = repo.unfiltered()
    return urepo.revs(oldlogrevstring)

revsetpredicate = registrar.revsetpredicate()

@revsetpredicate('olddraft')
def _olddraft(repo, subset, x):
    """``olddraft([index])``
    previous draft commits

    'index' is how many undoable commands you want to look back
    an undoable command is one that changed draft heads, bookmarks
    and or working copy parent
    Note: this revset may include hidden commits
    """
    args = revset.getargsdict(x, 'olddraftrevset', 'reverseindex')
    reverseindex = revsetlang.getinteger(args.get('reverseindex'),
                _('index must be a positive integer'), 1)
    revs = _getolddrafts(repo, reverseindex)
    return smartset.baseset(revs)

# Tools

def _invertindex(rlog, indexorreverseindex):
    return len(rlog) - 1 - indexorreverseindex

def _getrevlog(repo, filename):
    path = 'undolog/' + filename
    return revlog.revlog(repo.vfs, path)
