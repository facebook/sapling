# journal.py
#
# Copyright 2014-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Track previous positions of bookmarks

This extension adds a new command: `hg journal`, which shows you where
bookmarks were previously located.

"""

from __future__ import absolute_import

import collections
import errno
import os

from mercurial.i18n import _

from hgext import share
from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    dirstate,
    dispatch,
    error,
    extensions,
    hg,
    localrepo,
    node,
    util,
)

cmdtable = {}
command = cmdutil.command(cmdtable)

# Note for extension authors: ONLY specify testedwith = 'internal' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'internal'

# storage format version; increment when the format changes
storage_version = 0

# namespaces
bookmarktype = 'bookmark'
remotebookmarktype = 'remotebookmark'
wdirparenttype = 'wdirparent'
# In a shared repository, what shared feature name is used
# to indicate this namespace is shared with the source?
sharednamespaces = {
    bookmarktype: hg.sharedbookmarks,
    remotebookmarktype: hg.sharedbookmarks,
}

# Journal recording, register hooks and storage object
def extsetup(ui):
    extensions.wrapfunction(dispatch, 'runcommand', runcommand)
    extensions.wrapfunction(bookmarks.bmstore, '_write', recordbookmarks)
    extensions.wrapfunction(
        dirstate.dirstate, '_writedirstate', recorddirstateparents)
    extensions.wrapfunction(
        localrepo.localrepository.dirstate, 'func', wrapdirstate)
    extensions.wrapfunction(hg, 'postshare', wrappostshare)
    extensions.wrapcommand(share.cmdtable, 'unshare', unsharejournal)

    def hasremotenames(loaded):
        if not loaded:
            return
        remotenames = extensions.find('remotenames')
        extensions.wrapfunction(
            remotenames, 'saveremotenames', recordremotebookmarks)
    extensions.afterloaded('remotenames', hasremotenames)

def reposetup(ui, repo):
    if repo.local():
        repo.journal = journalstorage(repo)

def runcommand(orig, lui, repo, cmd, fullargs, *args):
    """Track the command line options for recording in the journal"""
    journalstorage.recordcommand(*fullargs)
    return orig(lui, repo, cmd, fullargs, *args)

# hooks to record dirstate changes
def wrapdirstate(orig, repo):
    dirstate = orig(repo)
    if util.safehasattr(repo, 'journal'):
        dirstate.journalrepo = repo
    return dirstate

def recorddirstateparents(orig, dirstate, dirstatefp):
    """Records all dirstate parent changes in the journal."""
    if util.safehasattr(dirstate, 'journalrepo'):
        old = [node.nullid, node.nullid]
        nodesize = len(node.nullid)
        try:
            # The only source for the old state is in the dirstate file
            # still on disk; the in-memory dirstate object only contains
            # the new state.
            with dirstate._opener(dirstate._filename) as fp:
                state = fp.read(2 * nodesize)
            if len(state) == 2 * nodesize:
                old = [state[:nodesize], state[nodesize:]]
        except IOError:
            pass

        new = dirstate.parents()
        if old != new:
            # only record two hashes if there was a merge
            oldhashes = old[:1] if old[1] == node.nullid else old
            newhashes = new[:1] if new[1] == node.nullid else new
            dirstate.journalrepo.journal.record(
                wdirparenttype, '.', oldhashes, newhashes)

    return orig(dirstate, dirstatefp)

# hooks to record bookmark changes (both local and remote)
def recordbookmarks(orig, store, fp):
    """Records all bookmark changes in the journal."""
    repo = store._repo
    if util.safehasattr(repo, 'journal'):
        oldmarks = bookmarks.bmstore(repo)
        for mark, value in store.iteritems():
            oldvalue = oldmarks.get(mark, node.nullid)
            if value != oldvalue:
                repo.journal.record(bookmarktype, mark, oldvalue, value)
    return orig(store, fp)

def recordremotebookmarks(
        orig, repo, remotepath, branches=None, bookmarks=None):
    """Records all remote bookmark movements in the journal."""
    if util.safehasattr(repo, 'journal'):
        if bookmarks is None:
            bookmarks = {}

        if branches is None:
            branches = {}

        if bookmarks:
            remotenames = extensions.find('remotenames')
            oldremotenames = remotenames.readremotenames(repo)

            oldbookmarks = dict(
                (oldname, oldnode)
                for oldnode, nametype, oldremote, oldname in oldremotenames
                if nametype == 'bookmarks' and oldremote == remotepath)

            for rmbookmark, newnode in bookmarks.iteritems():
                oldnode = oldbookmarks.get(rmbookmark, node.hex(node.nullid))
                if oldnode != newnode:
                    joinedremotename = remotenames.joinremotename(
                        remotepath, rmbookmark)
                    repo.journal.record(
                        remotebookmarktype, joinedremotename,
                        node.bin(oldnode), node.bin(newnode))

    return orig(repo, remotepath, branches, bookmarks)

# shared repository support
def _readsharedfeatures(repo):
    """A set of shared features for this repository"""
    try:
        return set(repo.vfs.read('shared').splitlines())
    except IOError as inst:
        if inst.errno != errno.ENOENT:
            raise
        return set()

def _mergeentriesiter(*iterables, **kwargs):
    """Given a set of sorted iterables, yield the next entry in merged order

    Note that by default entries go from most recent to oldest.
    """
    order = kwargs.pop('order', max)
    iterables = [iter(it) for it in iterables]
    # this tracks still active iterables; iterables are deleted as they are
    # exhausted, which is why this is a dictionary and why each entry also
    # stores the key. Entries are mutable so we can store the next value each
    # time.
    iterable_map = {}
    for key, it in enumerate(iterables):
        try:
            iterable_map[key] = [next(it), key, it]
        except StopIteration:
            # empty entry, can be ignored
            pass
    if not iterable_map:
        # all iterables where empty
        return

    while True:
        value, key, it = order(iterable_map.itervalues())
        yield value
        try:
            iterable_map[key][0] = next(it)
        except StopIteration:
            # this iterable is empty, remove it from consideration
            del iterable_map[key]
            if not iterable_map:
                # all iterables are empty
                return

def wrappostshare(orig, sourcerepo, destrepo, **kwargs):
    orig(sourcerepo, destrepo, **kwargs)
    with destrepo.vfs('shared', 'a') as fp:
        fp.write('journal\n')

def unsharejournal(orig, ui, repo):
    # do the work *before* the unshare command does it, as otherwise
    # we no longer have access to the source repo. We also can't wrap
    # copystore as we need a wlock while unshare takes the store lock.
    if repo.shared() and util.safehasattr(repo, 'journal'):
        sharedrepo = share._getsrcrepo(repo)
        sharedfeatures = _readsharedfeatures(repo)
        if sharedrepo and sharedfeatures > set(['journal']):
            # there is a shared repository and there are shared journal entries
            # to copy. move shared date over from source to destination but
            # move the local file first
            if repo.vfs.exists('journal'):
                journalpath = repo.join('journal')
                util.rename(journalpath, journalpath + '.bak')
            storage = repo.journal
            local = storage._open(
                repo, filename='journal.bak', _newestfirst=False)
            shared = (
                e for e in storage._open(sharedrepo, _newestfirst=False)
                if sharednamespaces.get(e.namespace) in sharedfeatures)
            for entry in _mergeentriesiter(local, shared, order=min):
                storage._write(repo, entry)

    return orig(ui, repo)

class journalentry(collections.namedtuple(
        'journalentry',
        'timestamp user command namespace name oldhashes newhashes')):
    """Individual journal entry

    * timestamp: a mercurial (time, timezone) tuple
    * user: the username that ran the command
    * namespace: the entry namespace, an opaque string
    * name: the name of the changed item, opaque string with meaning in the
      namespace
    * command: the hg command that triggered this record
    * oldhashes: a tuple of one or more binary hashes for the old location
    * newhashes: a tuple of one or more binary hashes for the new location

    Handles serialisation from and to the storage format. Fields are
    separated by newlines, hashes are written out in hex separated by commas,
    timestamp and timezone are separated by a space.

    """
    @classmethod
    def fromstorage(cls, line):
        (time, user, command, namespace, name,
         oldhashes, newhashes) = line.split('\n')
        timestamp, tz = time.split()
        timestamp, tz = float(timestamp), int(tz)
        oldhashes = tuple(node.bin(hash) for hash in oldhashes.split(','))
        newhashes = tuple(node.bin(hash) for hash in newhashes.split(','))
        return cls(
            (timestamp, tz), user, command, namespace, name,
            oldhashes, newhashes)

    def __str__(self):
        """String representation for storage"""
        time = ' '.join(map(str, self.timestamp))
        oldhashes = ','.join([node.hex(hash) for hash in self.oldhashes])
        newhashes = ','.join([node.hex(hash) for hash in self.newhashes])
        return '\n'.join((
            time, self.user, self.command, self.namespace, self.name,
            oldhashes, newhashes))

class journalstorage(object):
    """Storage for journal entries

    Entries are divided over two files; one with entries that pertain to the
    local working copy *only*, and one with entries that are shared across
    multiple working copies when shared using the share extension.

    Entries are stored with NUL bytes as separators. See the journalentry
    class for the per-entry structure.

    The file format starts with an integer version, delimited by a NUL.

    """
    _currentcommand = ()

    def __init__(self, repo):
        self.repo = repo
        self.user = util.getuser()

    # track the current command for recording in journal entries
    @property
    def command(self):
        commandstr = ' '.join(
            map(util.shellquote, journalstorage._currentcommand))
        if '\n' in commandstr:
            # truncate multi-line commands
            commandstr = commandstr.partition('\n')[0] + ' ...'
        return commandstr

    @classmethod
    def recordcommand(cls, *fullargs):
        """Set the current hg arguments, stored with recorded entries"""
        # Set the current command on the class because we may have started
        # with a non-local repo (cloning for example).
        cls._currentcommand = fullargs

    @util.propertycache
    def sharedfeatures(self):
        return _readsharedfeatures(self.repo)

    def record(self, namespace, name, oldhashes, newhashes):
        """Record a new journal entry

        * namespace: an opaque string; this can be used to filter on the type
          of recorded entries.
        * name: the name defining this entry; for bookmarks, this is the
          bookmark name. Can be filtered on when retrieving entries.
        * oldhashes and newhashes: each a single binary hash, or a list of
          binary hashes. These represent the old and new position of the named
          item.

        """
        if not isinstance(oldhashes, list):
            oldhashes = [oldhashes]
        if not isinstance(newhashes, list):
            newhashes = [newhashes]

        entry = journalentry(
            util.makedate(), self.user, self.command, namespace, name,
            oldhashes, newhashes)

        repo = self.repo
        if self.repo.shared() and 'journal' in self.sharedfeatures:
            # write to the shared repository if this feature is being
            # shared between working copies.
            if sharednamespaces.get(namespace) in self.sharedfeatures:
                srcrepo = share._getsrcrepo(repo)
                if srcrepo is not None:
                    repo = srcrepo

        self._write(repo, entry)

    def _write(self, repo, entry, _wait=False):
        try:
            with repo.wlock(wait=_wait):
                version = None
                # open file in amend mode to ensure it is created if missing
                with repo.vfs('journal', mode='a+b', atomictemp=True) as f:
                    f.seek(0, os.SEEK_SET)
                    # Read just enough bytes to get a version number (up to 2
                    # digits plus separator)
                    version = f.read(3).partition('\0')[0]
                    if version and version != str(storage_version):
                        # different version of the storage. Exit early (and
                        # not write anything) if this is not a version we can
                        # handle or the file is corrupt. In future, perhaps
                        # rotate the file instead?
                        repo.ui.warn(
                            _("unsupported journal file version '%s'\n") %
                            version)
                        return
                    if not version:
                        # empty file, write version first
                        f.write(str(storage_version) + '\0')
                    f.seek(0, os.SEEK_END)
                    f.write(str(entry) + '\0')
        except error.LockHeld as lockerr:
            lock = repo._wlockref and repo._wlockref()
            if lock and lockerr.locker == '%s:%s' % (lock._host, lock.pid):
                # the dirstate can be written out during wlock unlock, before
                # the lockfile is removed. Re-run the write as a postrelease
                # function instead.
                lock.postrelease.append(
                    lambda: self._write(repo, entry, _wait=True))
            else:
                # another process holds the lock, retry and wait
                self._write(repo, entry, _wait=True)

    def filtered(self, namespace=None, name=None):
        """Yield all journal entries with the given namespace or name

        Both the namespace and the name are optional; if neither is given all
        entries in the journal are produced.

        """
        for entry in self:
            if namespace is not None and entry.namespace != namespace:
                continue
            if name is not None and entry.name != name:
                continue
            yield entry

    def __iter__(self):
        """Iterate over the storage

        Yields journalentry instances for each contained journal record.

        """
        local = self._open(self.repo)

        if not self.repo.shared() or 'journal' not in self.sharedfeatures:
            return local
        sharedrepo = share._getsrcrepo(self.repo)
        if sharedrepo is None:
            return local

        # iterate over both local and shared entries, but only those
        # shared entries that are among the currently shared features
        shared = (
            e for e in self._open(sharedrepo)
            if sharednamespaces.get(e.namespace) in self.sharedfeatures)
        return _mergeentriesiter(local, shared)

    def _open(self, repo, filename='journal', _newestfirst=True):
        if not repo.vfs.exists(filename):
            return

        with repo.wlock():
            with repo.vfs(filename) as f:
                raw = f.read()

        lines = raw.split('\0')
        version = lines and lines[0]
        if version != str(storage_version):
            version = version or _('not available')
            raise error.Abort(_("unknown journal file version '%s'") % version)

        # Skip the first line, it's a version number. Normally we iterate over
        # these in reverse order to list newest first; only when copying across
        # a shared storage do we forgo reversing.
        lines = lines[1:]
        if _newestfirst:
            lines = reversed(lines)
        for line in lines:
            if not line:
                continue
            yield journalentry.fromstorage(line)

# journal reading
# log options that don't make sense for journal
_ignore_opts = ('no-merges', 'graph')
@command(
    'journal', [
        ('', 'all', None, 'show history for all names'),
        ('c', 'commits', None, 'show commit metadata'),
    ] + [opt for opt in commands.logopts if opt[1] not in _ignore_opts],
    '[OPTION]... [NAME]')
def journal(ui, repo, *args, **opts):
    """show the previous position of bookmarks and the working copy

    The journal is used to see the previous commits that bookmarks and the
    working copy pointed to. By default the previous locations for the working
    copy.  Passing a bookmark name will show all the previous positions of
    that bookmark. Use the --all switch to show previous locations for all
    bookmarks and the working copy; each line will then include the bookmark
    name, or '.' for the working copy, as well.

    By default hg journal only shows the commit hash and the command that was
    running at that time. -v/--verbose will show the prior hash, the user, and
    the time at which it happened.

    Use -c/--commits to output log information on each commit hash; at this
    point you can use the usual `--patch`, `--git`, `--stat` and `--template`
    switches to alter the log output for these.

    `hg journal -T json` can be used to produce machine readable output.

    """
    name = '.'
    if opts.get('all'):
        if args:
            raise error.Abort(
                _("You can't combine --all and filtering on a name"))
        name = None
    if args:
        name = args[0]

    fm = ui.formatter('journal', opts)

    if opts.get("template") != "json":
        if name is None:
            displayname = _('the working copy and bookmarks')
        else:
            displayname = "'%s'" % name
        ui.status(_("Previous locations of %s:\n") % displayname)

    limit = cmdutil.loglimit(opts)
    entry = None
    for count, entry in enumerate(repo.journal.filtered(name=name)):
        if count == limit:
            break
        newhashesstr = ','.join([node.short(hash) for hash in entry.newhashes])
        oldhashesstr = ','.join([node.short(hash) for hash in entry.oldhashes])

        fm.startitem()
        fm.condwrite(ui.verbose, 'oldhashes', '%s -> ', oldhashesstr)
        fm.write('newhashes', '%s', newhashesstr)
        fm.condwrite(ui.verbose, 'user', ' %s', entry.user.ljust(8))
        fm.condwrite(opts.get('all'), 'name', '  %s', entry.name.ljust(8))

        timestring = util.datestr(entry.timestamp, '%Y-%m-%d %H:%M %1%2')
        fm.condwrite(ui.verbose, 'date', ' %s', timestring)
        fm.write('command', '  %s\n', entry.command)

        if opts.get("commits"):
            displayer = cmdutil.show_changeset(ui, repo, opts, buffered=False)
            for hash in entry.newhashes:
                try:
                    ctx = repo[hash]
                    displayer.show(ctx)
                except error.RepoLookupError as e:
                    fm.write('repolookuperror', "%s\n\n", str(e))
            displayer.close()

    fm.end()

    if entry is None:
        ui.status(_("no recorded locations\n"))
