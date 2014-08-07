# transaction.py - simple journaling scheme for mercurial
#
# This transaction scheme is intended to gracefully handle program
# errors and interruptions. More serious failures like system crashes
# can be recovered with an fsck-like tool. As the whole repository is
# effectively log-structured, this should amount to simply truncating
# anything that isn't referenced in the changelog.
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import errno
import error, util

def active(func):
    def _active(self, *args, **kwds):
        if self.count == 0:
            raise error.Abort(_(
                'cannot use transaction when it is already committed/aborted'))
        return func(self, *args, **kwds)
    return _active

def _playback(journal, report, opener, entries, backupentries, unlink=True):
    for f, o, ignore in entries:
        if o or not unlink:
            try:
                fp = opener(f, 'a')
                fp.truncate(o)
                fp.close()
            except IOError:
                report(_("failed to truncate %s\n") % f)
                raise
        else:
            try:
                opener.unlink(f)
            except (IOError, OSError), inst:
                if inst.errno != errno.ENOENT:
                    raise

    backupfiles = []
    for f, b, ignore in backupentries:
        filepath = opener.join(f)
        backuppath = opener.join(b)
        try:
            util.copyfile(backuppath, filepath)
            backupfiles.append(b)
        except IOError:
            report(_("failed to recover %s\n") % f)
            raise

    opener.unlink(journal)
    backuppath = "%s.backupfiles" % journal
    if opener.exists(backuppath):
        opener.unlink(backuppath)
    for f in backupfiles:
        opener.unlink(f)

class transaction(object):
    def __init__(self, report, opener, journal, after=None, createmode=None,
            onclose=None, onabort=None):
        """Begin a new transaction

        Begins a new transaction that allows rolling back writes in the event of
        an exception.

        * `after`: called after the transaction has been committed
        * `createmode`: the mode of the journal file that will be created
        * `onclose`: called as the transaction is closing, but before it is
        closed
        * `onabort`: called as the transaction is aborting, but before any files
        have been truncated
        """
        self.count = 1
        self.usages = 1
        self.report = report
        self.opener = opener
        self.after = after
        self.onclose = onclose
        self.onabort = onabort
        self.entries = []
        self.backupentries = []
        self.map = {}
        self.backupmap = {}
        self.journal = journal
        self._queue = []
        # a dict of arguments to be passed to hooks
        self.hookargs = {}

        self.backupjournal = "%s.backupfiles" % journal
        self.file = opener.open(self.journal, "w")
        self.backupsfile = opener.open(self.backupjournal, 'w')
        if createmode is not None:
            opener.chmod(self.journal, createmode & 0666)
            opener.chmod(self.backupjournal, createmode & 0666)

        # hold file generations to be performed on commit
        self._filegenerators = {}

    def __del__(self):
        if self.journal:
            self._abort()

    @active
    def startgroup(self):
        self._queue.append(([], []))

    @active
    def endgroup(self):
        q = self._queue.pop()
        self.entries.extend(q[0])
        self.backupentries.extend(q[1])

        offsets = []
        backups = []
        for f, o, _ in q[0]:
            offsets.append((f, o))

        for f, b, _ in q[1]:
            backups.append((f, b))

        d = ''.join(['%s\0%d\n' % (f, o) for f, o in offsets])
        self.file.write(d)
        self.file.flush()

        d = ''.join(['%s\0%s\0' % (f, b) for f, b in backups])
        self.backupsfile.write(d)
        self.backupsfile.flush()

    @active
    def add(self, file, offset, data=None):
        if file in self.map or file in self.backupmap:
            return
        if self._queue:
            self._queue[-1][0].append((file, offset, data))
            return

        self.entries.append((file, offset, data))
        self.map[file] = len(self.entries) - 1
        # add enough data to the journal to do the truncate
        self.file.write("%s\0%d\n" % (file, offset))
        self.file.flush()

    @active
    def addbackup(self, file, hardlink=True):
        """Adds a backup of the file to the transaction

        Calling addbackup() creates a hardlink backup of the specified file
        that is used to recover the file in the event of the transaction
        aborting.

        * `file`: the file path, relative to .hg/store
        * `hardlink`: use a hardlink to quickly create the backup
        """

        if file in self.map or file in self.backupmap:
            return
        backupfile = "%s.backup.%s" % (self.journal, file)
        if self.opener.exists(file):
            filepath = self.opener.join(file)
            backuppath = self.opener.join(backupfile)
            util.copyfiles(filepath, backuppath, hardlink=hardlink)
        else:
            self.add(file, 0)
            return

        if self._queue:
            self._queue[-1][1].append((file, backupfile))
            return

        self.backupentries.append((file, backupfile, None))
        self.backupmap[file] = len(self.backupentries) - 1
        self.backupsfile.write("%s\0%s\0" % (file, backupfile))
        self.backupsfile.flush()

    @active
    def addfilegenerator(self, genid, filenames, genfunc, order=0):
        """add a function to generates some files at transaction commit

        The `genfunc` argument is a function capable of generating proper
        content of each entry in the `filename` tuple.

        At transaction close time, `genfunc` will be called with one file
        object argument per entries in `filenames`.

        The transaction itself is responsible for the backup, creation and
        final write of such file.

        The `genid` argument is used to ensure the same set of file is only
        generated once. Call to `addfilegenerator` for a `genid` already
        present will overwrite the old entry.

        The `order` argument may be used to control the order in which multiple
        generator will be executed.
        """
        self._filegenerators[genid] = (order, filenames, genfunc)

    @active
    def find(self, file):
        if file in self.map:
            return self.entries[self.map[file]]
        if file in self.backupmap:
            return self.backupentries[self.backupmap[file]]
        return None

    @active
    def replace(self, file, offset, data=None):
        '''
        replace can only replace already committed entries
        that are not pending in the queue
        '''

        if file not in self.map:
            raise KeyError(file)
        index = self.map[file]
        self.entries[index] = (file, offset, data)
        self.file.write("%s\0%d\n" % (file, offset))
        self.file.flush()

    @active
    def nest(self):
        self.count += 1
        self.usages += 1
        return self

    def release(self):
        if self.count > 0:
            self.usages -= 1
        # if the transaction scopes are left without being closed, fail
        if self.count > 0 and self.usages == 0:
            self._abort()

    def running(self):
        return self.count > 0

    @active
    def close(self):
        '''commit the transaction'''
        # write files registered for generation
        for order, filenames, genfunc in sorted(self._filegenerators.values()):
            files = []
            try:
                for name in filenames:
                    self.addbackup(name)
                    files.append(self.opener(name, 'w', atomictemp=True))
                genfunc(*files)
            finally:
                for f in files:
                    f.close()

        if self.count == 1 and self.onclose is not None:
            self.onclose()

        self.count -= 1
        if self.count != 0:
            return
        self.file.close()
        self.backupsfile.close()
        self.entries = []
        if self.after:
            self.after()
        if self.opener.isfile(self.journal):
            self.opener.unlink(self.journal)
        if self.opener.isfile(self.backupjournal):
            self.opener.unlink(self.backupjournal)
            for f, b, _ in self.backupentries:
                self.opener.unlink(b)
        self.backupentries = []
        self.journal = None

    @active
    def abort(self):
        '''abort the transaction (generally called on error, or when the
        transaction is not explicitly committed before going out of
        scope)'''
        self._abort()

    def _abort(self):
        self.count = 0
        self.usages = 0
        self.file.close()
        self.backupsfile.close()

        if self.onabort is not None:
            self.onabort()

        try:
            if not self.entries and not self.backupentries:
                if self.journal:
                    self.opener.unlink(self.journal)
                if self.backupjournal:
                    self.opener.unlink(self.backupjournal)
                return

            self.report(_("transaction abort!\n"))

            try:
                _playback(self.journal, self.report, self.opener,
                          self.entries, self.backupentries, False)
                self.report(_("rollback completed\n"))
            except Exception:
                self.report(_("rollback failed - please run hg recover\n"))
        finally:
            self.journal = None


def rollback(opener, file, report):
    """Rolls back the transaction contained in the given file

    Reads the entries in the specified file, and the corresponding
    '*.backupfiles' file, to recover from an incomplete transaction.

    * `file`: a file containing a list of entries, specifying where
    to truncate each file.  The file should contain a list of
    file\0offset pairs, delimited by newlines. The corresponding
    '*.backupfiles' file should contain a list of file\0backupfile
    pairs, delimited by \0.
    """
    entries = []
    backupentries = []

    fp = opener.open(file)
    lines = fp.readlines()
    fp.close()
    for l in lines:
        try:
            f, o = l.split('\0')
            entries.append((f, int(o), None))
        except ValueError:
            report(_("couldn't read journal entry %r!\n") % l)

    backupjournal = "%s.backupfiles" % file
    if opener.exists(backupjournal):
        fp = opener.open(backupjournal)
        data = fp.read()
        if len(data) > 0:
            parts = data.split('\0')
            for i in xrange(0, len(parts), 2):
                f, b = parts[i:i + 1]
                backupentries.append((f, b, None))

    _playback(file, report, opener, entries, backupentries)
