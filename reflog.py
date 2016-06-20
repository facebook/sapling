# reflog.py
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""show the previous position of bookmarks and the working copy"""

from mercurial import util, cmdutil, commands, hg, scmutil, localrepo, error
from mercurial import bookmarks, dispatch, dirstate
from mercurial.extensions import wrapcommand, wrapfunction, find
from mercurial.node import nullid, hex, bin
from mercurial.i18n import _
import errno, os, getpass, time, sys

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

version = 0

bookmarktype = 'bookmark'
workingcopyparenttype = 'workingcopyparent'
remotebookmarktype = 'remotebookmark'

currentcommand = []

def runcommand(orig, lui, repo, cmd, fullargs, *args):
    currentcommand[:] = fullargs
    return orig(lui, repo, cmd, fullargs, *args)

def extsetup(ui):
    wrapfunction(bookmarks.bmstore, '_write', recordbookmarks)
    wrapfunction(dirstate.dirstate, '_writedirstate', recorddirstateparents)
    wrapfunction(dispatch, 'runcommand', runcommand)

    try:
        remotenames = find('remotenames')
        if remotenames:
            wrapfunction(remotenames, 'saveremotenames', recordremotebookmarks)
    except KeyError:
        pass

    def wrapdirstate(orig, self):
        dirstate = orig(self)
        dirstate.reflogrepo = self
        return dirstate
    wrapfunction(localrepo.localrepository.dirstate, 'func', wrapdirstate)

def reposetup(ui, repo):
    if repo.local():
        repo.reflog = reflog(repo)

def recordbookmarks(orig, self, fp):
    """Records all bookmark changes to the reflog."""
    repo = self._repo
    oldmarks = bookmarks.bmstore(repo)
    for mark, value in self.iteritems():
        oldvalue = oldmarks.get(mark, nullid)
        if value != oldvalue:
            repo.reflog.addentry(bookmarktype, mark, oldvalue, value)
    return orig(self, fp)

def recordremotebookmarks(orig, repo, remotepath, branches=None,
                          bookmarks=None):
    """Records all remote bookmark movements to the reflog."""
    if bookmarks is None:
        bookmarks = {}

    if branches is None:
        branches = {}

    if bookmarks:
        remotenames = find('remotenames')
        oldremotenames = remotenames.readremotenames(repo)

        oldbookmarks = {}
        for oldnode, nametype, oldremote, oldname in oldremotenames:
            if nametype == 'bookmarks' and oldremote == remotepath:
                oldbookmarks[oldname] = oldnode

        for rmbookmark, node in bookmarks.iteritems():
            oldnode = oldbookmarks.get(rmbookmark, hex(nullid))
            if oldnode != node:
                joinedremotename = remotenames.joinremotename(
                    remotepath, rmbookmark)
                repo.reflog.addentry(remotebookmarktype,
                        joinedremotename, bin(oldnode), bin(node))

    return orig(repo, remotepath, branches, bookmarks)

def recorddirstateparents(orig, self, dirstatefp):
    """Records all dirstate parent changes to the reflog."""
    oldparents = [nullid, nullid]
    try:
        fp = self._opener("dirstate")
        st = fp.read(40)
        fp.close()
        l = len(st)
        if l == 40:
            oldparents = [st[:20]]
            oldparents.append(st[20:40])
    except IOError as err:
        pass

    parents = self.parents()
    if oldparents != parents:
        oldhashes = [oldparents[0]]
        if oldparents[1] != nullid:
            oldhashes.append(oldparents[1])
        newhashes = [parents[0]]
        if parents[1] != nullid:
            newhashes.append(parents[1])
        self.reflogrepo.reflog.addentry(workingcopyparenttype, '.', oldhashes,
            newhashes)
    return orig(self, dirstatefp)

@command('reflog',
    [('', 'all', None, 'show history for all refs'),
    ('c', 'commits', None, 'show commit metadata'),
     ] + commands.logopts, '[OPTION]... [REFNAME]')
def reflog(ui, repo, *args, **opts):
    """show the previous position of bookmarks and the working copy

    The reflog is used to see the previous commits that bookmarks and the
    working copy pointed to. By default it shows the previous locations of the
    working copy.  Passing a bookmark name will show all the previous
    positions of that bookmark. Passing --all will show the previous
    locations of all bookmarks and the working copy.

    `hg backups --recover <hash>` can be used to recover a commit if it's no
    longer in your repository.

    By default the reflog only shows the commit hash and the command that was
    running at that time. -v/--verbose will show the prior hash, the user, and
    the time at which it happened.

    `hg reflog -T json` can be used to produce machine readable output.
    """
    refname = '.'
    if args:
        refname = args[0]
    if opts.get('all'):
        refname = None

    fm = ui.formatter('reflog', opts)

    if opts.get("template") != "json":
        ui.status(_("Previous locations of '%s':\n") % refname)

    count = 0
    for entry in repo.reflog.iter(refnamecond=refname):
        count += 1
        timestamp, user, command, reftype, refname, oldhashes, newhashes = entry
        newhashesstr = ','.join([hash[:12] for hash in newhashes])
        oldhashesstr = ','.join([hash[:12] for hash in oldhashes])

        fm.startitem()
        fm.condwrite(ui.verbose, 'oldhashes', '%s -> ', oldhashesstr)
        fm.write('newhashes', '%s', newhashesstr)
        fm.condwrite(ui.verbose, 'user', ' %s', user.ljust(8))

        timestruct = time.localtime(timestamp[0])
        timestring = time.strftime('%Y-%m-%d %H:%M:%S', timestruct)
        fm.condwrite(ui.verbose, 'date', ' %s', timestring)
        fm.write('command', '  %s\n', command)

        if opts.get("commits"):
            displayer = cmdutil.show_changeset(ui, repo, opts, buffered=False)
            for hash in newhashes:
                try:
                    ctx = repo[hash]
                    displayer.show(ctx)
                except error.RepoLookupError as e:
                    fm.write('repolookuperror', "%s\n\n", str(e))
            displayer.close()

    fm.end()

    if count == 0:
        ui.status(_("no recorded locations\n"))

class reflog(object):
    def __init__(self, repo):
        self.repo = repo
        self.user = getpass.getuser()
        if repo.shared():
            self.path = repo.sharedpath + "/reflog"
        else:
            self.path = repo.join('reflog')

    def __iter__(self):
        return self._read()

    def iter(self, reftypecond=None, refnamecond=None):
        for entry in self._read():
            time, user, command, reftype, refname, old, new = entry
            if reftypecond and reftype != reftypecond:
                continue
            if refnamecond and refname != refnamecond:
                continue
            yield entry

    @property
    def command(self):
        commandstr = ' '.join(map(util.shellquote, currentcommand))
        if '\n' in commandstr:
            # truncate multi-line commands
            commandstr = commandstr.split('\n', 1)[0] + ' ...'
        return commandstr

    def _read(self):
        if not os.path.exists(self.path):
            raise StopIteration()

        f = open(self.path, 'r')
        try:
            raw = f.read()
        finally:
            f.close()

        version = raw[0]
        lines = raw.split('\0')
        # Skip the first line. It's a version number.
        lines = lines[1:]
        for line in reversed(lines):
            if not line:
                continue
            parts = line.split('\n')
            time, user, command, reftype, refname, oldhashes, newhashes = parts
            timeparts = time.split()
            time = (int(timeparts[0]), int(timeparts[1]))
            oldhashes = oldhashes.split(',')
            newhashes = newhashes.split(',')
            yield (time, user, command, reftype, refname, oldhashes, newhashes)

    def addentry(self, reftype, refname, oldhashes, newhashes):
        if isinstance(oldhashes, str):
            oldhashes = [oldhashes]
        if isinstance(newhashes, str):
            newhashes = [newhashes]

        timestamp, tz = util.makedate()
        date = "%s %s" % (int(timestamp), int(tz))
        oldhashes = ','.join([hex(hash) for hash in oldhashes])
        newhashes = ','.join([hex(hash) for hash in newhashes])
        data = (date, self.user, self.command, reftype, refname, oldhashes,
                newhashes)
        data = '\n'.join(data)

        newreflog = not os.path.exists(self.path)
        f = open(self.path, 'a+')
        try:
            if newreflog:
                f.write(str(version) + '\0')
            f.write(data + '\0')
        finally:
            f.close()
