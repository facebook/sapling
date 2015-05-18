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

version = 2

def active(func):
    def _active(self, *args, **kwds):
        if self.count == 0:
            raise error.Abort(_(
                'cannot use transaction when it is already committed/aborted'))
        return func(self, *args, **kwds)
    return _active

def _playback(journal, report, opener, vfsmap, entries, backupentries,
              unlink=True):
    for f, o, _ignore in entries:
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
    for l, f, b, c in backupentries:
        if l not in vfsmap and c:
            report("couldn't handle %s: unknown cache location %s\n"
                        % (b, l))
        vfs = vfsmap[l]
        try:
            if f and b:
                filepath = vfs.join(f)
                backuppath = vfs.join(b)
                try:
                    util.copyfile(backuppath, filepath)
                    backupfiles.append(b)
                except IOError:
                    report(_("failed to recover %s\n") % f)
            else:
                target = f or b
                try:
                    vfs.unlink(target)
                except (IOError, OSError), inst:
                    if inst.errno != errno.ENOENT:
                        raise
        except (IOError, OSError, util.Abort), inst:
            if not c:
                raise

    opener.unlink(journal)
    backuppath = "%s.backupfiles" % journal
    if opener.exists(backuppath):
        opener.unlink(backuppath)
    try:
        for f in backupfiles:
            if opener.exists(f):
                opener.unlink(f)
    except (IOError, OSError, util.Abort), inst:
        # only pure backup file remains, it is sage to ignore any error
        pass

class transaction(object):
    def __init__(self, report, opener, vfsmap, journalname, undoname=None,
                 after=None, createmode=None, validator=None):
        """Begin a new transaction

        Begins a new transaction that allows rolling back writes in the event of
        an exception.

        * `after`: called after the transaction has been committed
        * `createmode`: the mode of the journal file that will be created
        """
        self.count = 1
        self.usages = 1
        self.report = report
        # a vfs to the store content
        self.opener = opener
        # a map to access file in various {location -> vfs}
        vfsmap = vfsmap.copy()
        vfsmap[''] = opener  # set default value
        self._vfsmap = vfsmap
        self.after = after
        self.entries = []
        self.map = {}
        self.journal = journalname
        self.undoname = undoname
        self._queue = []
        # A callback to validate transaction content before closing it.
        # should raise exception is anything is wrong.
        # target user is repository hooks.
        if validator is None:
            validator = lambda tr: None
        self.validator = validator
        # a dict of arguments to be passed to hooks
        self.hookargs = {}
        self.file = opener.open(self.journal, "w")

        # a list of ('location', 'path', 'backuppath', cache) entries.
        # - if 'backuppath' is empty, no file existed at backup time
        # - if 'path' is empty, this is a temporary transaction file
        # - if 'location' is not empty, the path is outside main opener reach.
        #   use 'location' value as a key in a vfsmap to find the right 'vfs'
        # (cache is currently unused)
        self._backupentries = []
        self._backupmap = {}
        self._backupjournal = "%s.backupfiles" % self.journal
        self._backupsfile = opener.open(self._backupjournal, 'w')
        self._backupsfile.write('%d\n' % version)

        if createmode is not None:
            opener.chmod(self.journal, createmode & 0666)
            opener.chmod(self._backupjournal, createmode & 0666)

        # hold file generations to be performed on commit
        self._filegenerators = {}
        # hold callback to write pending data for hooks
        self._pendingcallback = {}
        # True is any pending data have been written ever
        self._anypending = False
        # holds callback to call when writing the transaction
        self._finalizecallback = {}
        # hold callback for post transaction close
        self._postclosecallback = {}
        # holds callbacks to call during abort
        self._abortcallback = {}

    def __del__(self):
        if self.journal:
            self._abort()

    @active
    def startgroup(self):
        """delay registration of file entry

        This is used by strip to delay vision of strip offset. The transaction
        sees either none or all of the strip actions to be done."""
        self._queue.append([])

    @active
    def endgroup(self):
        """apply delayed registration of file entry.

        This is used by strip to delay vision of strip offset. The transaction
        sees either none or all of the strip actions to be done."""
        q = self._queue.pop()
        for f, o, data in q:
            self._addentry(f, o, data)

    @active
    def add(self, file, offset, data=None):
        """record the state of an append-only file before update"""
        if file in self.map or file in self._backupmap:
            return
        if self._queue:
            self._queue[-1].append((file, offset, data))
            return

        self._addentry(file, offset, data)

    def _addentry(self, file, offset, data):
        """add a append-only entry to memory and on-disk state"""
        if file in self.map or file in self._backupmap:
            return
        self.entries.append((file, offset, data))
        self.map[file] = len(self.entries) - 1
        # add enough data to the journal to do the truncate
        self.file.write("%s\0%d\n" % (file, offset))
        self.file.flush()

    @active
    def addbackup(self, file, hardlink=True, location=''):
        """Adds a backup of the file to the transaction

        Calling addbackup() creates a hardlink backup of the specified file
        that is used to recover the file in the event of the transaction
        aborting.

        * `file`: the file path, relative to .hg/store
        * `hardlink`: use a hardlink to quickly create the backup
        """
        if self._queue:
            msg = 'cannot use transaction.addbackup inside "group"'
            raise RuntimeError(msg)

        if file in self.map or file in self._backupmap:
            return
        vfs = self._vfsmap[location]
        dirname, filename = vfs.split(file)
        backupfilename = "%s.backup.%s" % (self.journal, filename)
        backupfile = vfs.reljoin(dirname, backupfilename)
        if vfs.exists(file):
            filepath = vfs.join(file)
            backuppath = vfs.join(backupfile)
            util.copyfile(filepath, backuppath, hardlink=hardlink)
        else:
            backupfile = ''

        self._addbackupentry((location, file, backupfile, False))

    def _addbackupentry(self, entry):
        """register a new backup entry and write it to disk"""
        self._backupentries.append(entry)
        self._backupmap[file] = len(self._backupentries) - 1
        self._backupsfile.write("%s\0%s\0%s\0%d\n" % entry)
        self._backupsfile.flush()

    @active
    def registertmp(self, tmpfile, location=''):
        """register a temporary transaction file

        Such files will be deleted when the transaction exits (on both
        failure and success).
        """
        self._addbackupentry((location, '', tmpfile, False))

    @active
    def addfilegenerator(self, genid, filenames, genfunc, order=0,
                         location=''):
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

        The `location` arguments may be used to indicate the files are located
        outside of the the standard directory for transaction. It should match
        one of the key of the `transaction.vfsmap` dictionary.
        """
        # For now, we are unable to do proper backup and restore of custom vfs
        # but for bookmarks that are handled outside this mechanism.
        self._filegenerators[genid] = (order, filenames, genfunc, location)

    def _generatefiles(self, suffix=''):
        # write files registered for generation
        any = False
        for entry in sorted(self._filegenerators.values()):
            any = True
            order, filenames, genfunc, location = entry
            vfs = self._vfsmap[location]
            files = []
            try:
                for name in filenames:
                    name += suffix
                    if suffix:
                        self.registertmp(name, location=location)
                    else:
                        self.addbackup(name, location=location)
                    files.append(vfs(name, 'w', atomictemp=True))
                genfunc(*files)
            finally:
                for f in files:
                    f.close()
        return any

    @active
    def find(self, file):
        if file in self.map:
            return self.entries[self.map[file]]
        if file in self._backupmap:
            return self._backupentries[self._backupmap[file]]
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

    def addpending(self, category, callback):
        """add a callback to be called when the transaction is pending

        The transaction will be given as callback's first argument.

        Category is a unique identifier to allow overwriting an old callback
        with a newer callback.
        """
        self._pendingcallback[category] = callback

    @active
    def writepending(self):
        '''write pending file to temporary version

        This is used to allow hooks to view a transaction before commit'''
        categories = sorted(self._pendingcallback)
        for cat in categories:
            # remove callback since the data will have been flushed
            any = self._pendingcallback.pop(cat)(self)
            self._anypending = self._anypending or any
        self._anypending |= self._generatefiles(suffix='.pending')
        return self._anypending

    @active
    def addfinalize(self, category, callback):
        """add a callback to be called when the transaction is closed

        The transaction will be given as callback's first argument.

        Category is a unique identifier to allow overwriting old callbacks with
        newer callbacks.
        """
        self._finalizecallback[category] = callback

    @active
    def addpostclose(self, category, callback):
        """add a callback to be called after the transaction is closed

        The transaction will be given as callback's first argument.

        Category is a unique identifier to allow overwriting an old callback
        with a newer callback.
        """
        self._postclosecallback[category] = callback

    @active
    def addabort(self, category, callback):
        """add a callback to be called when the transaction is aborted.

        The transaction will be given as the first argument to the callback.

        Category is a unique identifier to allow overwriting an old callback
        with a newer callback.
        """
        self._abortcallback[category] = callback

    @active
    def close(self):
        '''commit the transaction'''
        if self.count == 1:
            self.validator(self)  # will raise exception if needed
            self._generatefiles()
            categories = sorted(self._finalizecallback)
            for cat in categories:
                self._finalizecallback[cat](self)

        self.count -= 1
        if self.count != 0:
            return
        self.file.close()
        self._backupsfile.close()
        # cleanup temporary files
        for l, f, b, c in self._backupentries:
            if l not in self._vfsmap and c:
                self.report("couldn't remote %s: unknown cache location %s\n"
                            % (b, l))
                continue
            vfs = self._vfsmap[l]
            if not f and b and vfs.exists(b):
                try:
                    vfs.unlink(b)
                except (IOError, OSError, util.Abort), inst:
                    if not c:
                        raise
                    # Abort may be raise by read only opener
                    self.report("couldn't remote %s: %s\n"
                                % (vfs.join(b), inst))
        self.entries = []
        self._writeundo()
        if self.after:
            self.after()
        if self.opener.isfile(self.journal):
            self.opener.unlink(self.journal)
        if self.opener.isfile(self._backupjournal):
            self.opener.unlink(self._backupjournal)
            for l, _f, b, c in self._backupentries:
                if l not in self._vfsmap and c:
                    self.report("couldn't remote %s: unknown cache location"
                                "%s\n" % (b, l))
                    continue
                vfs = self._vfsmap[l]
                if b and vfs.exists(b):
                    try:
                        vfs.unlink(b)
                    except (IOError, OSError, util.Abort), inst:
                        if not c:
                            raise
                        # Abort may be raise by read only opener
                        self.report("couldn't remote %s: %s\n"
                                    % (vfs.join(b), inst))
        self._backupentries = []
        self.journal = None
        # run post close action
        categories = sorted(self._postclosecallback)
        for cat in categories:
            self._postclosecallback[cat](self)

    @active
    def abort(self):
        '''abort the transaction (generally called on error, or when the
        transaction is not explicitly committed before going out of
        scope)'''
        self._abort()

    def _writeundo(self):
        """write transaction data for possible future undo call"""
        if self.undoname is None:
            return
        undobackupfile = self.opener.open("%s.backupfiles" % self.undoname, 'w')
        undobackupfile.write('%d\n' % version)
        for l, f, b, c in self._backupentries:
            if not f:  # temporary file
                continue
            if not b:
                u = ''
            else:
                if l not in self._vfsmap and c:
                    self.report("couldn't remote %s: unknown cache location"
                                "%s\n" % (b, l))
                    continue
                vfs = self._vfsmap[l]
                base, name = vfs.split(b)
                assert name.startswith(self.journal), name
                uname = name.replace(self.journal, self.undoname, 1)
                u = vfs.reljoin(base, uname)
                util.copyfile(vfs.join(b), vfs.join(u), hardlink=True)
            undobackupfile.write("%s\0%s\0%s\0%d\n" % (l, f, u, c))
        undobackupfile.close()


    def _abort(self):
        self.count = 0
        self.usages = 0
        self.file.close()
        self._backupsfile.close()

        try:
            if not self.entries and not self._backupentries:
                if self.journal:
                    self.opener.unlink(self.journal)
                if self._backupjournal:
                    self.opener.unlink(self._backupjournal)
                return

            self.report(_("transaction abort!\n"))

            try:
                for cat in sorted(self._abortcallback):
                    self._abortcallback[cat](self)
                _playback(self.journal, self.report, self.opener, self._vfsmap,
                          self.entries, self._backupentries, False)
                self.report(_("rollback completed\n"))
            except BaseException:
                self.report(_("rollback failed - please run hg recover\n"))
        finally:
            self.journal = None


def rollback(opener, vfsmap, file, report):
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
        lines = fp.readlines()
        if lines:
            ver = lines[0][:-1]
            if ver == str(version):
                for line in lines[1:]:
                    if line:
                        # Shave off the trailing newline
                        line = line[:-1]
                        l, f, b, c = line.split('\0')
                        backupentries.append((l, f, b, bool(c)))
            else:
                report(_("journal was created by a different version of "
                         "Mercurial\n"))

    _playback(file, report, opener, vfsmap, entries, backupentries)
