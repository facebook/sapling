# undo.py: records data in revlog for future undo functionality
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial.i18n import _

from mercurial import (
    cmdutil,
    dispatch,
    error,
    extensions,
    hg,
    localrepo,
    lock as lockmod,
    obsolete,
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
    changes = safelog(repo, [""])
    if changes:
        _recordnewgap(repo)

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
                if changes and not ("undo" == command[0] or "redo" ==
                                    command[0]):
                    _delundoredo(repo)
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

def _logundoredoindex(repo, tr, reverseindex):
    rlog = _getrevlog(repo, 'index.i')
    hexnode = hex(rlog.node(_invertindex(rlog, reverseindex)))
    return repo.svfs.write("undolog/redonode", str(hexnode))

def _delundoredo(repo):
    path = 'undolog' + '/' + 'redonode'
    repo.svfs.tryunlink(path)

def _recordnewgap(repo, absoluteindex=None):
    path = 'undolog' + '/' + 'gap'
    if absoluteindex is None:
        rlog = _getrevlog(repo, 'index.i')
        repo.svfs.write(path, str(len(rlog) - 1))
    else:
        repo.svfs.write(path, str(absoluteindex))

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

def _gapcheck(repo, reverseindex):
    rlog = _getrevlog(repo, 'index.i')
    absoluteindex = _invertindex(rlog, reverseindex)
    path = 'undolog' + '/' + 'gap'
    try:
        result = absoluteindex >= int(repo.svfs.read(path))
    except IOError:
        # recreate file
        repo.ui.debug("failed to read gap file in %s, attempting recreation\n"
                      % path)
        rlog = _getrevlog(repo, 'index.i')
        i = 0
        while i < (len(rlog)):
            indexdict = _readindex(repo, i, rlog)
            if "" == _readnode(repo, "command.i", indexdict["command"]):
                break
            i += 1
        # defaults to before oldest command
        _recordnewgap(repo, _invertindex(rlog, i))
        result = absoluteindex >= _invertindex(rlog, i)
    finally:
        return result

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

# Undo:

@command('undo', [
    ('a', 'absolute', False, _("absolute based on command index instead of "
                               "relative undo")),
    ('f', 'force', False, _("undo across missing undo history (ADVANCED)")),
    ('k', 'keep', False, _("keep working copy changes")),
    ('n', 'index', 1, _("how many steps to undo back")),
])
def undo(ui, repo, *args, **opts):
    """perform an undo

    Undoes an undoable command.  An undoable command is one that changed at
    least one of the following three: bookmarks, working copy parent or
    changesets. Note that this specifically does not include commands like log.
    It will include update if update changes the working copy parent (you update
    to a changeset that isn't the current one).  Note that commands that edit
    public repos can't be undone (specifically push).

    Undo does not preserve the working copy changes.

    To undo to a specific state use the --index and --absolute flags.
    See hg debugundohistory to get a list of indeces and commands run.
    By undoing to a specific index you undo to the state after that command.
    For example, hg undo --index 0 --absolute won't do anything, while
    hg undo -n 1 -a will bring you back to the repo state before the current
    one.

    Without the --absolute flag, your undos will be relative.  This means
    they will behave how you expect them to.  If you run hg undo twice,
    you will move back two repo states from where you ran your first hg undo.
    You can use this in conjunction with hg redo to move up and down repo
    states.  Note that as soon as you execute a different undoable command,
    which isn't hg undo or hg redo, any new undos or redos will be relative to
    the state after this command.  When using --index with relative undos,
    this is equivalent to running index many undos, except for leaving your
    repo state history (hg debugundohistory) less cluttered.

    Undo states are also distinct repo states and can thereby be inspected using
    debugundohistory and specifically jumped to using undo --index --absolute.

    If the undo extension was turned off and on again, you might loose the
    ability to undo to certain repo states.  Undoing to repo states before the
    missing ones can be forced, but isn't advised unless its known how the
    before and after states are connected.

    Use keep to maintain working copy changes.  With keep, undo mimics hg
    unamend and hg uncommit.  Specifically, files that exsist currently that
    don't exist at the repo state we are undoing to will remain in your
    working copy but not in your changeset.  Maintaining your working copy
    has primarily two downsides: firstly your new working copy won't be clean
    so you can't simply redo without cleaning your working copy.  Secondly,
    the operation may be slow if your working copy is large.  If unsure,
    its generally easier try undo without --keep first and redo if you want
    to change this.
    """
    reverseindex = opts.get("index")
    relativeundo = not opts.get("absolute")
    keep = opts.get("keep")

    with repo.wlock(), repo.lock(), repo.transaction("undo"):
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)
        repo = repo.unfiltered()
        if relativeundo:
            reverseindex = _computerelative(repo, reverseindex)
        if not (opts.get("force") or _gapcheck(repo, reverseindex)):
            raise error.Abort(_("attempted risky undo across"
                                " missing history"))
        _undoto(ui, repo, reverseindex, keep=keep)
        # store undo data
        # for absolute undos, think of this as a reset
        # for relative undos, think of this as an update
        _logundoredoindex(repo, repo.currenttransaction(), reverseindex)

@command('redo', [
    ('n', 'index', 1, _("how many commands to redo")),
])
def redo(ui, repo, *args, **opts):
    """ perform a redo

    Performs a redo.  Specifically, redo moves forward a repo state relative to
    the previous undo or redo command.  If you run hg undo -n 10, you can redo
    each of the 10 repo states one by one all the way back to the state from
    which you ran undo.  You can use --index to redo across more states at once,
    and you can use any number of undos/redos up to the current state or back to
    when the undo extension was first active.
    """
    reverseindex = -1 * abs(opts.get("index"))
    reverseindex = _computerelative(repo, reverseindex)

    with repo.wlock(), repo.lock(), repo.transaction("redo"):
        cmdutil.checkunfinished(repo)
        cmdutil.bailifchanged(repo)
        repo = repo.unfiltered()
        _undoto(ui, repo, reverseindex)
        _logundoredoindex(repo, repo.currenttransaction(), reverseindex)

def _undoto(ui, repo, reverseindex, keep=False):
    # undo to specific reverseindex
    # requires inhibit extension
    if repo != repo.unfiltered():
        raise error.ProgrammingError(_("_undoto expects unfilterd repo"))
    try:
        nodedict = _readindex(repo, reverseindex)
    except IndexError:
        raise error.Abort(_("index out of bounds"))

    # bookmarks
    bookstring = _readnode(repo, "bookmarks.i", nodedict["bookmarks"])
    booklist = bookstring.split("\n")
    # copy implementation for bookmarks
    itercopy = []
    for mark in repo._bookmarks.iteritems():
        itercopy.append(mark)
    bmchanges = [(mark[0], None) for mark in itercopy]
    repo._bookmarks.applychanges(repo, repo.currenttransaction(), bmchanges)
    bmchanges = []
    for mark in booklist:
        if mark:
            kv = mark.rsplit(" ", 1)
            bmchanges.append((kv[0], bin(kv[1])))
    repo._bookmarks.applychanges(repo, repo.currenttransaction(), bmchanges)

    # working copy parent
    workingcopyparent = _readnode(repo, "workingparent.i",
                                  nodedict["workingparent"])
    if not keep:
        revealcommits(repo, workingcopyparent)
        hg.updatetotally(ui, repo, workingcopyparent, workingcopyparent,
                         clean=False, updatecheck='abort')
    else:
        # keeps working copy files
        curctx = repo['.']
        precnode = bin(workingcopyparent)
        precctx = repo[precnode]

        changedfiles = []
        wctx = repo[None]
        wctxmanifest = wctx.manifest()
        precctxmanifest = precctx.manifest()
        dirstate = repo.dirstate
        diff = precctxmanifest.diff(wctxmanifest)
        changedfiles.extend(diff.iterkeys())

        with dirstate.parentchange():
            dirstate.rebuild(precnode, precctxmanifest, changedfiles)
            # we want added and removed files to be shown
            # properly, not with ? and ! prefixes
            for filename, data in diff.iteritems():
                if data[0][0] is None:
                    dirstate.add(filename)
                if data[1][0] is None:
                    dirstate.remove(filename)
        obsolete.createmarkers(repo, [(curctx, (precctx,))])

    # visible changesets
    addedrevs = revsetlang.formatspec('olddraft(0) - olddraft(%d)',
                                      reverseindex)
    hidecommits(repo, addedrevs)

    removedrevs = revsetlang.formatspec('olddraft(%d) - olddraft(0)',
                                        reverseindex)
    revealcommits(repo, removedrevs)

def _computerelative(repo, reverseindex):
    # allows for relative undos using
    # redonode storage
    try:
        hexnode = repo.svfs.read("undolog/redonode")
        rlog = _getrevlog(repo, 'index.i')
        rev = rlog.rev(bin(hexnode))
        reverseindex = _invertindex(rlog, rev) + reverseindex
    except IOError:
        # return input index
        pass
    return reverseindex

# hide and reveal commits

def hidecommits(repo, rev):
    ctxs = repo.set(rev)
    for commit in ctxs:
        obsolete.createmarkers(repo, [[commit,[]]])

def revealcommits(repo, rev):
    try:
        inhibit = extensions.find('inhibit')
    except KeyError:
        raise error.Abort(_('undo requires inhibit to work properly'))
    else:
        ctxts = repo.set(rev)
        inhibit.revive(ctxts)

# Tools

def _invertindex(rlog, indexorreverseindex):
    return len(rlog) - 1 - indexorreverseindex

def _getrevlog(repo, filename):
    path = 'undolog/' + filename
    return revlog.revlog(repo.vfs, path)
