# journal.py
#
# Copyright 2014-2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Track previous positions of bookmarks (EXPERIMENTAL)

This extension adds a new command: `hg journal`, which shows you where
bookmarks were previously located.

"""

from __future__ import absolute_import

import collections
import os

from mercurial.i18n import _

from mercurial import (
    bookmarks,
    cmdutil,
    commands,
    dispatch,
    error,
    extensions,
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
storageversion = 0

# namespaces
bookmarktype = 'bookmark'

# Journal recording, register hooks and storage object
def extsetup(ui):
    extensions.wrapfunction(dispatch, 'runcommand', runcommand)
    extensions.wrapfunction(bookmarks.bmstore, '_write', recordbookmarks)

def reposetup(ui, repo):
    if repo.local():
        repo.journal = journalstorage(repo)

def runcommand(orig, lui, repo, cmd, fullargs, *args):
    """Track the command line options for recording in the journal"""
    journalstorage.recordcommand(*fullargs)
    return orig(lui, repo, cmd, fullargs, *args)

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

    Entries are stored with NUL bytes as separators. See the journalentry
    class for the per-entry structure.

    The file format starts with an integer version, delimited by a NUL.

    """
    _currentcommand = ()

    def __init__(self, repo):
        self.repo = repo
        self.user = util.getuser()
        self.vfs = repo.vfs

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

        with self.repo.wlock():
            version = None
            # open file in amend mode to ensure it is created if missing
            with self.vfs('journal', mode='a+b', atomictemp=True) as f:
                f.seek(0, os.SEEK_SET)
                # Read just enough bytes to get a version number (up to 2
                # digits plus separator)
                version = f.read(3).partition('\0')[0]
                if version and version != str(storageversion):
                    # different version of the storage. Exit early (and not
                    # write anything) if this is not a version we can handle or
                    # the file is corrupt. In future, perhaps rotate the file
                    # instead?
                    self.repo.ui.warn(
                        _("unsupported journal file version '%s'\n") % version)
                    return
                if not version:
                    # empty file, write version first
                    f.write(str(storageversion) + '\0')
                f.seek(0, os.SEEK_END)
                f.write(str(entry) + '\0')

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
        if not self.vfs.exists('journal'):
            return

        with self.vfs('journal') as f:
            raw = f.read()

        lines = raw.split('\0')
        version = lines and lines[0]
        if version != str(storageversion):
            version = version or _('not available')
            raise error.Abort(_("unknown journal file version '%s'") % version)

        # Skip the first line, it's a version number. Reverse the rest.
        lines = reversed(lines[1:])
        for line in lines:
            if not line:
                continue
            yield journalentry.fromstorage(line)

# journal reading
# log options that don't make sense for journal
_ignoreopts = ('no-merges', 'graph')
@command(
    'journal', [
        ('c', 'commits', None, 'show commit metadata'),
    ] + [opt for opt in commands.logopts if opt[1] not in _ignoreopts],
    '[OPTION]... [BOOKMARKNAME]')
def journal(ui, repo, *args, **opts):
    """show the previous position of bookmarks

    The journal is used to see the previous commits of bookmarks. By default
    the previous locations for all bookmarks are shown.  Passing a bookmark
    name will show all the previous positions of that bookmark.

    By default hg journal only shows the commit hash and the command that was
    running at that time. -v/--verbose will show the prior hash, the user, and
    the time at which it happened.

    Use -c/--commits to output log information on each commit hash; at this
    point you can use the usual `--patch`, `--git`, `--stat` and `--template`
    switches to alter the log output for these.

    `hg journal -T json` can be used to produce machine readable output.

    """
    bookmarkname = None
    if args:
        bookmarkname = args[0]

    fm = ui.formatter('journal', opts)

    if opts.get("template") != "json":
        if bookmarkname is None:
            name = _('all bookmarks')
        else:
            name = "'%s'" % bookmarkname
        ui.status(_("previous locations of %s:\n") % name)

    limit = cmdutil.loglimit(opts)
    entry = None
    for count, entry in enumerate(repo.journal.filtered(name=bookmarkname)):
        if count == limit:
            break
        newhashesstr = ','.join([node.short(hash) for hash in entry.newhashes])
        oldhashesstr = ','.join([node.short(hash) for hash in entry.oldhashes])

        fm.startitem()
        fm.condwrite(ui.verbose, 'oldhashes', '%s -> ', oldhashesstr)
        fm.write('newhashes', '%s', newhashesstr)
        fm.condwrite(ui.verbose, 'user', ' %s', entry.user.ljust(8))

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
