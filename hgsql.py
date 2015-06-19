# hgsql.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""
CREATE TABLE revisions(
repo CHAR(64) BINARY NOT NULL,
path VARCHAR(512) BINARY NOT NULL,
chunk INT UNSIGNED NOT NULL,
chunkcount INT UNSIGNED NOT NULL,
linkrev INT UNSIGNED NOT NULL,
rev INT UNSIGNED NOT NULL,
node CHAR(40) BINARY NOT NULL,
entry BINARY(64) NOT NULL,
data0 CHAR(1) NOT NULL,
data1 LONGBLOB NOT NULL,
createdtime DATETIME NOT NULL,
INDEX linkrevs (repo, linkrev),
PRIMARY KEY (repo, path, rev, chunk)
);

CREATE TABLE revision_references(
autoid INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
repo CHAR(32) BINARY NOT NULL,
namespace CHAR(32) BINARY NOT NULL,
name VARCHAR(256) BINARY,
value char(40) BINARY NOT NULL,
UNIQUE KEY bookmarkindex (repo, namespace, name)
);
"""

from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import changelog, error, cmdutil, revlog, localrepo, transaction
from mercurial import wireproto, bookmarks, repair, commands, hg, mdiff, phases
from mercurial import util, changegroup, exchange
import MySQLdb, struct, time, Queue, threading, _mysql_exceptions
from MySQLdb import cursors
import warnings
import sys

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

writelock = 'write_lock'

INITIAL_SYNC_NORMAL = 'normal'
INITIAL_SYNC_DISABLE = 'disabled'
INITIAL_SYNC_FORCE = 'force'

initialsync = INITIAL_SYNC_NORMAL

class CorruptionException(Exception):
    pass

def uisetup(ui):
    # Enable SQL for local commands that write to the repository.
    wrapcommand(commands.table, 'pull', pull)
    wrapcommand(commands.table, 'commit', commit)

    wrapcommand(commands.table, 'bookmark', bookmarkcommand)
    wrapfunction(exchange, '_localphasemove', _localphasemove)
    wrapfunction(exchange, 'push', push)

    # Enable SQL for remote commands that write to the repository
    wrapfunction(wireproto, 'unbundle', unbundle)
    wireproto.commands['unbundle'] = (wireproto.unbundle, 'heads')
    wrapfunction(exchange, 'unbundle', unbundle)

    wrapfunction(wireproto, 'pushkey', pushkey)
    wireproto.commands['pushkey'] = (wireproto.pushkey, 'namespace key old new')

    wrapfunction(bookmarks, 'updatefromremote', updatefromremote)
    wrapfunction(changegroup, 'addchangegroup', addchangegroup)

    # Record revlog writes
    def writeentry(orig, self, transaction, ifh, dfh, entry, data, link, offset):
        """records each revlog write to the repo's pendingrev list"""
        if not util.safehasattr(transaction, "repo"):
            return orig(self, transaction, ifh, dfh, entry, data, link, offset)

        e = struct.unpack(revlog.indexformatng, entry)
        node = hex(e[7])
        data0 = data[0] or ''
        transaction.repo.pendingrevs.append((self.indexfile, link,
            len(self) - 1, node, entry, data0, data[1]))
        return orig(self, transaction, ifh, dfh, entry, data, link, offset)
    wrapfunction(revlog.revlog, '_writeentry', writeentry)

    # Reorder incoming revs to be in linkrev order
    wrapfunction(revlog.revlog, 'addgroup', addgroup)

    # Write SQL bookmarks at the same time as local bookmarks
    wrapfunction(bookmarks.bmstore, 'write', bookmarkwrite)

def extsetup(ui):
    if ui.configbool('hgsql', 'enabled'):
        commands.globalopts.append(
                ('', 'forcesync', False,
                 _('force hgsql sync even on read-only commands'),
                 _('TYPE')))

    # Directly examining argv seems like a terrible idea, but it seems
    # neccesary unless we refactor mercurial dispatch code. This is because
    # the first place we have access to parsed options is in the same function
    # (dispatch.dispatch) that created the repo and the repo creation initiates
    # the sync operation in which the lock is elided unless we set this.
    if '--forcesync' in sys.argv:
        ui.debug('forcesync enabled\n')
        global initialsync
        initialsync = INITIAL_SYNC_FORCE

def reposetup(ui, repo):
    if repo.ui.configbool("hgsql", "enabled"):
        wraprepo(repo)

        if initialsync != INITIAL_SYNC_DISABLE:
            # Use a noop to force a sync
            def noop():
                pass
            waitforlock = (initialsync == INITIAL_SYNC_FORCE)
            executewithsql(repo, noop, waitforlock=waitforlock)

# Incoming commits are only allowed via push and pull
def unbundle(orig, *args, **kwargs):
    repo = args[0]
    if repo.ui.configbool("hgsql", "enabled"):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)

def pull(orig, *args, **kwargs):
    repo = args[1]
    if repo.ui.configbool("hgsql", "enabled"):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)

def push(orig, *args, **kwargs):
    repo = args[0]
    if repo.ui.configbool("hgsql", "enabled"):
        # A push locks the local repo in order to update phase data, so we need
        # to take the lock for the local repo during a push.
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)

def commit(orig, *args, **kwargs):
    repo = args[1]
    if repo.ui.configbool("hgsql", "enabled"):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)

def updatefromremote(orig, *args, **kwargs):
    repo = args[1]
    if repo.ui.configbool("hgsql", "enabled"):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)

def addchangegroup(orig, *args, **kwargs):
    repo = args[0]
    if repo.ui.configbool("hgsql", "enabled"):
        return executewithsql(repo, orig, True, *args, **kwargs)
    else:
        return orig(*args, **kwargs)

def _localphasemove(orig, pushop, *args, **kwargs):
    repo = pushop.repo
    if repo.ui.configbool("hgsql", "enabled"):
        return executewithsql(repo, orig, True, pushop, *args, **kwargs)
    else:
        return orig(pushop, *args, **kwargs)

def executewithsql(repo, action, sqllock=False, *args, **kwargs):
    """Executes the given action while having a SQL connection open.
    If a locks are specified, those locks are held for the duration of the
    action.
    """
    # executewithsql can be executed in a nested scenario (ex: writing
    # bookmarks during a pull), so track whether this call performed
    # the connect.

    waitforlock = sqllock
    if 'waitforlock' in kwargs:
        if not waitforlock:
            waitforlock = kwargs['waitforlock']
        del kwargs['waitforlock']

    connected = False
    if not repo.sqlconn:
        repo.sqlconnect()
        connected = True

    locked = False
    if sqllock and not writelock in repo.heldlocks:
            repo.sqllock(writelock)
            locked = True

    result = None
    success = False
    try:
        if connected:
            repo.syncdb(waitforlock=waitforlock)
        result = action(*args, **kwargs)
        success = True
    finally:
        try:
            # Release the locks in the reverse order they were obtained
            if locked:
                repo.sqlunlock(writelock)
            if connected:
                repo.sqlclose()
        except _mysql_exceptions.ProgrammingError, ex:
            if success:
                raise
            # If the action caused an exception, hide sql cleanup exceptions,
            # so the real exception is propagated up.
            pass

    return result

def wraprepo(repo):
    class sqllocalrepo(repo.__class__):
        def sqlconnect(self):
            if self.sqlconn:
                raise Exception("SQL connection already open")
            if self.sqlcursor:
                raise Exception("SQL cursor already open without connection")
            self.sqlconn = MySQLdb.connect(**self.sqlargs)
            self.sqlconn.autocommit(False)
            waittimeout = self.ui.config('hgsql', 'waittimeout', '300')
            waittimeout = self.sqlconn.escape_string("%s" % (waittimeout,))
            locktimeout = self.ui.config('hgsql', 'locktimeout', '60')
            locktimeout = self.sqlconn.escape_string("%s" % (locktimeout,))
            self.sqlconn.query("SET wait_timeout=%s" % waittimeout)
            self.sqlconn.query("SET innodb_lock_wait_timeout=%s" % locktimeout)
            self.sqlcursor = self.sqlconn.cursor()

        def sqlclose(self):
            with warnings.catch_warnings():
                warnings.simplefilter("ignore")
                self.sqlcursor.close()
                self.sqlconn.close()
            self.sqlcursor = None
            self.sqlconn = None

        def sqllock(self, name, timeout=60):
            escapedname = self.sqlconn.escape_string("%s_%s" % (name, self.sqlreponame))
            # cast to int to prevent passing bad sql data
            timeout = int(timeout)
            self.sqlconn.query("SELECT GET_LOCK('%s', %s)" % (escapedname, timeout))
            result = self.sqlconn.store_result().fetch_row()[0][0]
            if result != 1:
                raise Exception("unable to obtain %s lock" % escapedname)
            self.heldlocks.add(name)

        def hassqllock(self, name):
            if not name in self.heldlocks:
                return False

            escapedname = self.sqlconn.escape_string("%s_%s" % (name, self.sqlreponame))
            self.sqlconn.query("SELECT IS_USED_LOCK('%s')" % (escapedname,))
            lockheldby = self.sqlconn.store_result().fetch_row()[0][0]
            if lockheldby == None:
                raise Exception("unable to check %s lock" % escapedname)

            self.sqlconn.query("SELECT CONNECTION_ID()")
            myconnectid = self.sqlconn.store_result().fetch_row()[0][0]
            if myconnectid == None:
                raise Exception("unable to read connection id")

            return lockheldby == myconnectid

        def sqlunlock(self, name):
            escapedname = self.sqlconn.escape_string("%s_%s" % (name, self.sqlreponame))
            self.sqlconn.query("SELECT RELEASE_LOCK('%s')" % (escapedname,))
            self.sqlconn.store_result().fetch_row()
            self.heldlocks.discard(name)

        def transaction(self, *args, **kwargs):
            tr = super(sqllocalrepo, self).transaction(*args, **kwargs)
            if tr.count > 1:
                return tr

            validator = tr.validator
            def pretxnclose(tr):
                validator(tr)
                self.committodb(tr)
                del self.pendingrevs[:]
            tr.validator = pretxnclose

            def transactionabort(orig):
                del self.pendingrevs[:]
                return orig()
            wrapfunction(tr, "_abort", transactionabort)

            tr.repo = self
            return tr

        def needsync(self):
            """Returns True if the local repo is not in sync with the database.
            If it returns False, the heads and bookmarks match the database.
            """
            self.sqlcursor.execute("""SELECT namespace, name, value
                FROM revision_references WHERE repo = %s""", (self.sqlreponame))
            sqlheads = set()
            sqlbookmarks = {}
            tip = -1
            for namespace, name, value in self.sqlcursor:
                if namespace == "heads":
                    sqlheads.add(bin(value))
                elif namespace == "bookmarks":
                    sqlbookmarks[name] = bin(value)
                elif namespace == "tip":
                    tip = int(value)

            heads = set(self.heads())
            bookmarks = self._bookmarks

            outofsync = heads != sqlheads or bookmarks != sqlbookmarks or tip != len(self) - 1
            return outofsync, sqlheads, sqlbookmarks, tip

        def syncdb(self, waitforlock=False):
            ui = self.ui
            if not self.needsync()[0]:
                ui.debug("syncing not needed\n")
                return
            ui.debug("syncing with mysql\n")

            try:
                lock = self.lock(wait=waitforlock)
            except error.LockHeld:
                # Oh well. Don't block this non-critical read-only operation.
                ui.debug("skipping sync for current operation\n")
                return

            configbackups = []
            try:
                # Disable all pretxnclose hooks, since these revisions are
                # technically already commited.
                for name, value in ui.configitems("hooks"):
                    if name.startswith("pretxnclose"):
                        configbackups.append(ui.backupconfig("hooks", name))
                        ui.setconfig("hooks", name, None)
                # The hg-ssh wrapper installs a hook to block all writes. We need to
                # circumvent this when we sync from the server.
                configbackups.append(ui.backupconfig("hooks", "pretxnopen.hg-ssh"))
                self.ui.setconfig("hooks", "pretxnopen.hg-ssh", None)
                # Someone else may have synced us while we were waiting.
                # Restart the transaction so we have access to the latest rows.
                self.sqlconn.rollback()
                outofsync, sqlheads, sqlbookmarks, fetchend = self.needsync()
                if not outofsync:
                    return

                transaction = self.transaction("syncdb")

                self.hook('presyncdb', throw=True)

                try:
                    # Inspect the changelog now that we have the lock
                    fetchstart = len(self.changelog)

                    queue = Queue.Queue()
                    abort = threading.Event()

                    t = threading.Thread(target=self.fetchthread,
                        args=(queue, abort, fetchstart, fetchend))
                    t.setDaemon(True)
                    try:
                        t.start()
                        addentries(self, queue, transaction)
                    finally:
                        abort.set()

                    phases.advanceboundary(self, transaction, phases.public,
                                           self.heads())

                    transaction.close()
                finally:
                    transaction.release()

                # We circumvent the changelog and manifest when we add entries to
                # the revlogs. So clear all the caches.
                self.invalidate()

                heads = set(self.heads())
                heads.discard(nullid)
                if heads != sqlheads:
                    raise CorruptionException("heads don't match after sync")

                if len(self) - 1 != fetchend:
                    raise CorruptionException("tip doesn't match after sync")

                self.disablesync = True
                try:
                    bm = self._bookmarks
                    bm.clear()
                    self.sqlcursor.execute("""SELECT name, value FROM revision_references
                        WHERE namespace = 'bookmarks' AND repo = %s""",
                        (self.sqlreponame))
                    for name, node in self.sqlcursor:
                        node = bin(node)
                        if node in self:
                            bm[name] = node
                    bm.write()
                finally:
                    self.disablesync = False

                if bm != sqlbookmarks:
                    raise CorruptionException("bookmarks don't match after sync")
            finally:
                for backup in configbackups:
                    ui.restoreconfig(backup)
                lock.release()

        def fetchthread(self, queue, abort, fetchstart, fetchend):
            """Fetches every revision from fetchstart to fetchend (inclusive)
            and places them on the queue. This function is meant to run on a
            background thread and listens to the abort event to abort early.
            """
            ui = self.ui
            clrev = fetchstart
            chunksize = 1000
            while True:
                if abort.isSet():
                    break

                maxrev = min(clrev + chunksize, fetchend + 1)
                self.sqlcursor.execute("""SELECT path, chunk, chunkcount,
                    linkrev, entry, data0, data1 FROM revisions WHERE repo = %s
                    AND linkrev > %s AND linkrev < %s ORDER BY linkrev ASC""",
                    (self.sqlreponame, clrev - 1, maxrev))

                # Put split chunks back together into a single revision
                groupedrevdata = {}
                for revdata in self.sqlcursor:
                    name = revdata[0]
                    chunk = revdata[1]
                    linkrev = revdata[3]
                    groupedrevdata.setdefault((name, linkrev), {})[chunk] = revdata

                if not groupedrevdata:
                    break

                fullrevisions = []
                for chunks in groupedrevdata.itervalues():
                    chunkcount = chunks[0][2]
                    if chunkcount == 1:
                        fullrevisions.append(chunks[0])
                    elif chunkcount == len(chunks):
                        fullchunk = list(chunks[0])
                        data1 = ""
                        for i in range(0, chunkcount):
                            data1 += chunks[i][6]
                        fullchunk[6] = data1
                        fullrevisions.append(tuple(fullchunk))
                    else:
                        raise Exception("missing revision chunk - expected %s got %s" %
                            (chunkcount, len(chunks)))

                fullrevisions = sorted(fullrevisions, key=lambda revdata: revdata[3])
                for revdata in fullrevisions:
                    queue.put(revdata)

                clrev += chunksize

            queue.put(False)

        def pushkey(self, namespace, key, old, new):
            def _pushkey():
                return super(sqllocalrepo, self).pushkey(namespace, key, old, new)

            return executewithsql(self, _pushkey, namespace == 'bookmarks')

        def committodb(self, tr):
            """Commits all pending revisions to the database
            """
            if self.sqlconn == None:
                raise util.Abort("invalid repo change - only hg push and pull" +
                    " are allowed")

            if not self.pendingrevs and not 'bookmark_moved' in tr.hookargs:
                return

            reponame = self.sqlreponame
            cursor = self.sqlcursor
            maxcommitsize = self.maxcommitsize
            maxrowsize = self.maxrowsize

            if self.pendingrevs:
                self._validatependingrevs()

            try:
                datasize = 0
                for revision in self.pendingrevs:
                    path, linkrev, rev, node, entry, data0, data1 = revision

                    start = 0
                    chunk = 0
                    datalen = len(data1)
                    chunkcount = datalen / maxrowsize
                    if datalen % maxrowsize != 0 or datalen == 0:
                        chunkcount += 1

                    # We keep row size down by breaking large revisions down into
                    # smaller chunks.
                    while chunk == 0 or start < len(data1):
                        end = min(len(data1), start + maxrowsize)
                        datachunk = data1[start:end]
                        cursor.execute("""INSERT INTO revisions(repo, path, chunk,
                            chunkcount, linkrev, rev, node, entry, data0, data1, createdtime)
                            VALUES(%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)""",
                            (reponame, path, chunk, chunkcount, linkrev, rev,
                             node, entry, data0, datachunk,
                             time.strftime('%Y-%m-%d %H:%M:%S')))
                        chunk += 1
                        start = end

                        # MySQL transactions can only reach a certain size, so we commit
                        # every so often.  As long as we don't update the tip pushkey,
                        # this is ok.
                        datasize += len(datachunk)
                        if datasize > maxcommitsize:
                            self.sqlconn.commit()
                            datasize = 0

                if datasize > 0:
                    # commit at the end just to make sure we're clean
                    self.sqlconn.commit()

                cursor.execute("""DELETE FROM revision_references WHERE repo = %s
                               AND namespace = 'heads'""", (reponame,))

                # Write the bookmarks that are part of this transaction. This
                # may write them even if nothing has changed, but that's not
                # a big deal.
                cursor.execute("""DELETE FROM revision_references WHERE repo = %s AND
                               namespace = 'bookmarks'""", (repo.sqlreponame))
                tmpl = []
                values = []
                for head in self.heads():
                    tmpl.append("(%s, 'heads', NULL, %s)")
                    values.append(reponame)
                    values.append(hex(head))

                for k, v in repo._bookmarks.iteritems():
                    tmpl.append("(%s, 'bookmarks', %s, %s)")
                    values.append(repo.sqlreponame)
                    values.append(k)
                    values.append(hex(v))

                cursor.execute("INSERT INTO revision_references(repo, namespace, name, value) " +
                               "VALUES %s" % ','.join(tmpl), tuple(values))

                # revision_references has multiple keys (primary key, and a unique index), so
                # mysql gives a warning when using ON DUPLICATE KEY since it would only update one
                # row despite multiple key duplicates. This doesn't matter for us, since we know
                # there is only one row that will share the same key. So suppress the warning.
                cursor.execute("""INSERT INTO revision_references(repo, namespace, name, value)
                               VALUES(%s, 'tip', 'tip', %s) ON DUPLICATE KEY UPDATE value=%s""",
                               (reponame, len(self) - 1, len(self) - 1))

                # Just to be super sure, check the write lock before doing the final commit
                if not self.hassqllock(writelock):
                    raise Exception("attempting to write to sql without holding %s (precommit)"
                        % writelock)

                self.sqlconn.commit()
            except:
                self.sqlconn.rollback()
                raise
            finally:
                del self.pendingrevs[:]

        def _validatependingrevs(self):
            """Validates that the current pending revisions will be valid when
            written to the database.
            """
            reponame = self.sqlreponame
            cursor = self.sqlcursor

            # Ensure we hold the write lock
            if not self.hassqllock(writelock):
                raise Exception("attempting to write to sql without holding %s (prevalidate)"
                    % writelock)

            # Validate that we are appending to the correct linkrev
            cursor.execute("""SELECT value FROM revision_references WHERE repo = %s AND
                namespace = 'tip'""", reponame)
            tipresults = cursor.fetchall()
            if len(tipresults) == 0:
                maxlinkrev = -1
            elif len(tipresults) == 1:
                maxlinkrev = int(tipresults[0][0])
            else:
                raise CorruptionException(("multiple tips for %s in " +
                    " the database") % reponame)

            minlinkrev = min(self.pendingrevs, key= lambda x: x[1])[1]
            if maxlinkrev == None or maxlinkrev != minlinkrev - 1:
                raise CorruptionException("attempting to write non-sequential " +
                    "linkrev %s, expected %s" % (minlinkrev, maxlinkrev + 1))

            # Clean up excess revisions left from interrupted commits.
            # Since MySQL can only hold so much data in a transaction, we allow
            # committing across multiple db transactions. That means if
            # the commit is interrupted, the next transaction needs to clean
            # up bad revisions.
            cursor.execute("""DELETE FROM revisions WHERE repo = %s AND
                linkrev > %s""", (reponame, maxlinkrev))

            # Validate that all rev dependencies (base, p1, p2) have the same
            # node in the database
            pending = set([(path, rev) for path, _, rev, _, _, _, _ in self.pendingrevs])
            expectedrevs = set()
            for revision in self.pendingrevs:
                path, linkrev, rev, node, entry, data0, data1 = revision
                e = struct.unpack(revlog.indexformatng, entry)
                _, _, _, base, _, p1r, p2r, _ = e

                if p1r != nullrev and not (path, p1r) in pending:
                    expectedrevs.add((path, p1r))
                if p2r != nullrev and not (path, p2r) in pending:
                    expectedrevs.add((path, p2r))
                if (base != nullrev and base != rev and
                    not (path, base) in pending):
                    expectedrevs.add((path, base))

            if not expectedrevs:
                return

            whereclauses = []
            args = []
            args.append(reponame)
            for path, rev in expectedrevs:
                whereclauses.append("(path, rev, chunk) = (%s, %s, 0)")
                args.append(path)
                args.append(rev)

            whereclause = ' OR '.join(whereclauses)
            cursor.execute("""SELECT path, rev, node FROM revisions WHERE
                repo = %s AND (""" + whereclause + ")",
                args)

            for path, rev, node in cursor:
                rev = int(rev)
                expectedrevs.remove((path, rev))
                rl = None
                if path == '00changelog.i':
                    rl = self.changelog
                elif path == '00manifest.i':
                    rl = self.manifest
                else:
                    rl = revlog.revlog(self.sopener, path)
                localnode = hex(rl.node(rev))
                if localnode != node:
                    raise CorruptionException(("expected node %s at rev %d of " +
                    "%s but found %s") % (node, rev, path, localnode))

            if len(expectedrevs) > 0:
                raise CorruptionException(("unable to verify %d dependent " +
                    "revisions before adding a commit") % (len(expectedrevs)))

    ui = repo.ui

    sqlargs = {}
    sqlargs['host'] = ui.config("hgsql", "host")
    sqlargs['db'] = ui.config("hgsql", "database")
    sqlargs['user'] = ui.config("hgsql", "user")
    sqlargs['port'] = ui.configint("hgsql", "port")
    password = ui.config("hgsql", "password", "")
    if password:
        sqlargs['passwd'] = password
    sqlargs['cursorclass'] = cursors.SSCursor

    repo.sqlargs = sqlargs

    repo.sqlreponame = ui.config("hgsql", "reponame")
    if not repo.sqlreponame:
        raise Exception("missing hgsql.reponame")
    repo.maxcommitsize = ui.configbytes("hgsql", "maxcommitsize", 52428800)
    repo.maxrowsize = ui.configbytes("hgsql", "maxrowsize", 1048576)
    repo.sqlconn = None
    repo.sqlcursor = None
    repo.disablesync = False
    repo.pendingrevs = []
    repo.heldlocks = set()

    repo.__class__ = sqllocalrepo

class bufferedopener(object):
    """Opener implementation that buffers all writes in memory until
    flush or close is called.
    """
    def __init__(self, opener, path, mode):
        self.opener = opener
        self.path = path
        self.mode = mode
        self.buffer = []
        self.closed = False

    def write(self, value):
        if self.closed:
            raise Exception("attempted to write to a closed bufferedopener")
        self.buffer.append(value)

    def flush(self):
        buffer = self.buffer
        self.buffer = []

        if buffer:
            fp = self.opener(self.path, self.mode)
            fp.write(''.join(buffer))
            fp.close()

    def close(self):
        self.flush()
        self.closed = True

def addentries(repo, queue, transaction):
    """Reads new rev entries from a queue and writes them to a buffered
    revlog. At the end it flushes all the new data to disk.
    """
    opener = repo.sopener

    revlogs = {}
    def writeentry(revdata):
        # Instantiates pseudo-revlogs for us to write data directly to
        path, chunk, chunkcount, link, entry, data0, data1 = revdata
        revlog = revlogs.get(path)
        if not revlog:
            revlog = EntryRevlog(opener, path)
            revlogs[path] = revlog

        # Replace the existing openers with buffered ones so we can
        # perform the flush to disk all at once at the end.
        if not hasattr(revlog, 'ifh') or revlog.ifh.closed:
            dfh = None
            if not revlog._inline:
                dfh = bufferedopener(opener, revlog.datafile, "a")
            ifh = bufferedopener(opener, revlog.indexfile, "a+")
            revlog.ifh = ifh
            revlog.dfh = dfh

        revlog.addentry(transaction, revlog.ifh, revlog.dfh, entry,
                        data0, data1)

    leftover = None
    exit = False

    # Read one linkrev at a time from the queue
    while not exit:
        currentlinkrev = -1

        revisions = []
        if leftover:
            revisions.append(leftover)
            leftover = None

        # Read everything from the current linkrev
        while True:
            revdata = queue.get()
            if not revdata:
                exit = True
                break

            linkrev = revdata[3]
            if currentlinkrev == -1:
                currentlinkrev = linkrev
            if linkrev == currentlinkrev:
                revisions.append(revdata)
            elif linkrev < currentlinkrev:
                raise CorruptionException("SQL data is not in linkrev order")
            else:
                leftover = revdata
                currentlinkrev = linkrev
                break

        if not revisions:
            continue

        for revdata in revisions:
            writeentry(revdata)

    # Flush filelogs, then manifest, then changelog
    changelog = revlogs.pop("00changelog.i", None)
    manifest = revlogs.pop("00manifest.i", None)

    def flushrevlog(revlog):
        if not revlog.ifh.closed:
            revlog.ifh.flush()
        if revlog.dfh and not revlog.dfh.closed:
            revlog.dfh.flush()

    for filelog in revlogs.itervalues():
        flushrevlog(filelog)

    if manifest:
        flushrevlog(manifest)
    if changelog:
        flushrevlog(changelog)

class EntryRevlog(revlog.revlog):
    """Pseudo-revlog implementation that allows applying data directly to
    the end of the revlog.
    """
    def addentry(self, transaction, ifh, dfh, entry, data0, data1):
        curr = len(self)
        offset = self.end(curr - 1)

        e = struct.unpack(revlog.indexformatng, entry)
        offsettype, datalen, textlen, base, link, p1r, p2r, node = e

        # The first rev has special metadata encoded in it that should be
        # stripped before being added to the index.
        if curr == 0:
            elist = list(e)
            type = revlog.gettype(offsettype)
            offsettype = revlog.offset_type(0, type)
            elist[0] = offsettype
            e = tuple(elist)

        # Verify that the rev's parents and base appear earlier in the revlog
        if p1r >= curr or p2r >= curr:
            raise CorruptionException("parent revision is not in revlog: %s" %
                self.indexfile)
        if base > curr:
            raise CorruptionException("base revision is not in revlog: %s" %
                self.indexfile)

        expectedoffset = revlog.getoffset(offsettype)
        if expectedoffset != 0 and expectedoffset != offset:
            raise CorruptionException("revision offset doesn't match prior length " +
                "(%s offset vs %s length): %s" %
                (expectedoffset, offset, self.indexfile))

        if node not in self.nodemap:
            self.index.insert(-1, e)
            self.nodemap[node] = len(self) - 1

        if not self._inline:
            transaction.add(self.datafile, offset)
            transaction.add(self.indexfile, curr * len(entry))
            if data0:
                dfh.write(data0)
            dfh.write(data1)
            ifh.write(entry)
        else:
            offset += curr * self._io.size
            transaction.add(self.indexfile, offset, curr)
            ifh.write(entry)
            ifh.write(data0)
            ifh.write(data1)
            self.checkinlinesize(transaction, ifh)

def addgroup(orig, self, bundle, linkmapper, transaction):
    """Copy paste of revlog.addgroup, but we ensure that the revisions are
    added in linkrev order.
    """
    if not util.safehasattr(transaction, "repo"):
        return orig(self, bundle, linkmapper, transaction)

    # track the base of the current delta log
    content = []
    node = None

    r = len(self)
    end = 0
    if r:
        end = self.end(r - 1)
    ifh = self.opener(self.indexfile, "a+")
    isize = r * self._io.size
    if self._inline:
        transaction.add(self.indexfile, end + isize, r)
        dfh = None
    else:
        transaction.add(self.indexfile, isize, r)
        transaction.add(self.datafile, end)
        dfh = self.opener(self.datafile, "a")

    try:
        # loop through our set of deltas
        chunkdatas = []
        chunkmap = {}

        lastlinkrev = -1
        reorder = False

        # Read all of the data from the stream
        chain = None
        while True:
            chunkdata = bundle.deltachunk(chain)
            if not chunkdata:
                break

            node = chunkdata['node']
            cs = chunkdata['cs']
            link = linkmapper(cs)
            if link < lastlinkrev:
                reorder = True
            lastlinkrev = link
            chunkdatas.append((link, chunkdata))
            chunkmap[node] = chunkdata
            chain = node

        # If we noticed a incoming rev was not in linkrev order
        # we reorder all the revs appropriately.
        if reorder:
            chunkdatas = sorted(chunkdatas)

            fulltexts = {}
            def getfulltext(node):
                if node in fulltexts:
                    return fulltexts[node]
                if node in self.nodemap:
                    return self.revision(node)

                chunkdata = chunkmap[node]
                deltabase = chunkdata['deltabase']
                delta = chunkdata['delta']

                deltachain = []
                currentbase = deltabase
                while True:
                    if currentbase in fulltexts:
                        deltachain.append(fulltexts[currentbase])
                        break
                    elif currentbase in self.nodemap:
                        deltachain.append(self.revision(currentbase))
                        break
                    elif currentbase == nullid:
                        break
                    else:
                        deltachunk = chunkmap[currentbase]
                        currentbase = deltachunk['deltabase']
                        deltachain.append(deltachunk['delta'])

                prevtext = deltachain.pop()
                while deltachain:
                    prevtext = mdiff.patch(prevtext, deltachain.pop())

                fulltext = mdiff.patch(prevtext, delta)
                fulltexts[node] = fulltext
                return fulltext

            visited = set()
            prevnode = self.node(len(self) - 1)
            for link, chunkdata in chunkdatas:
                node = chunkdata['node']
                deltabase = chunkdata['deltabase']
                if (not deltabase in self.nodemap and
                    not deltabase in visited):
                    fulltext = getfulltext(node)
                    ptext = getfulltext(prevnode)
                    delta = mdiff.textdiff(ptext, fulltext)

                    chunkdata['delta'] = delta
                    chunkdata['deltabase'] = prevnode

                prevnode = node
                visited.add(node)

        # Apply the reordered revs to the revlog
        for link, chunkdata in chunkdatas:
            node = chunkdata['node']
            p1 = chunkdata['p1']
            p2 = chunkdata['p2']
            cs = chunkdata['cs']
            deltabase = chunkdata['deltabase']
            delta = chunkdata['delta']

            content.append(node)

            link = linkmapper(cs)
            if node in self.nodemap:
                # this can happen if two branches make the same change
                continue

            for p in (p1, p2):
                if p not in self.nodemap:
                    raise LookupError(p, self.indexfile,
                                      _('unknown parent'))

            if deltabase not in self.nodemap:
                raise LookupError(deltabase, self.indexfile,
                                  _('unknown delta base'))

            baserev = self.rev(deltabase)
            self._addrevision(node, None, transaction, link, p1, p2,
                              revlog.REVIDX_DEFAULT_FLAGS, (baserev, delta),
                              ifh, dfh)
            if not dfh and not self._inline:
                # addrevision switched from inline to conventional
                # reopen the index
                ifh.close()
                dfh = self.opener(self.datafile, "a")
                ifh = self.opener(self.indexfile, "a")
    finally:
        if dfh:
            dfh.close()
        ifh.close()

    return content

def bookmarkcommand(orig, ui, repo, *names, **opts):
    if not repo.ui.configbool("hgsql", "enabled"):
        return orig(ui, repo, *names, **opts)

    write = (opts.get('delete') or opts.get('rename')
             or opts.get('inactive') or names)

    def _bookmarkcommand():
        return orig(ui, repo, *names, **opts)

    if write:
        return executewithsql(repo, _bookmarkcommand, True)
    else:
        return _bookmarkcommand()

def bookmarkwrite(orig, self):
    repo = self._repo
    if not repo.ui.configbool("hgsql", "enabled") or repo.disablesync:
        return orig(self)

    if not repo.sqlconn:
        raise util.Abort("attempted bookmark write without sql connection")
    elif not repo.hassqllock(writelock):
        raise util.Abort("attempted bookmark write without write lock")

    try:
        cursor = repo.sqlcursor
        cursor.execute("""DELETE FROM revision_references WHERE repo = %s AND
                       namespace = 'bookmarks'""", (repo.sqlreponame))

        for k, v in self.iteritems():
            cursor.execute("""INSERT INTO revision_references(repo, namespace, name, value)
                           VALUES(%s, 'bookmarks', %s, %s)""",
                           (repo.sqlreponame, k, hex(v)))
        repo.sqlconn.commit()
        return orig(self)
    except:
        repo.sqlconn.rollback()
        raise

def pushkey(orig, repo, proto, namespace, key, old, new):
    if repo.ui.configbool("hgsql", "enabled"):
        def commitpushkey():
            return orig(repo, proto, namespace, key, old, new)

        return executewithsql(repo, commitpushkey, namespace == 'bookmarks')
    else:
        return orig(repo, proto, namespace, key, old, new)

# recover must be a norepo command because loading the repo fails
commands.norepo += " sqlrecover sqlstrip"

@command('^sqlrecover', [
    ('f', 'force', '', _('strips as far back as necessary'), ''),
    ('', 'no-backup', '', _('does not produce backup bundles for strips'), ''),
    ], _('hg sqlrecover'))
def sqlrecover(ui, *args, **opts):
    """
    Strips commits from the local repo until it is back in sync with the SQL
    server.
    """

    global initialsync
    initialsync = INITIAL_SYNC_DISABLE
    repo = hg.repository(ui, ui.environ['PWD'])
    repo.disablesync = True

    if repo.recover():
        ui.status("recovered from incomplete transaction")

    def iscorrupt():
        repo.sqlconnect()
        try:
            repo.syncdb()
        except:
            return True
        finally:
            repo.sqlclose()

        return False

    reposize = len(repo)

    stripsize = 10
    while iscorrupt() and len(repo) > 0:
        if not opts.get('force') and reposize > len(repo) + 10000:
            ui.warn("unable to fix repo after stripping 10000 commits " +
                    "(use -f to strip more)")

        striprev = max(0, len(repo) - stripsize)
        nodelist = [repo[striprev].node()]
        ui.status("stripping back to %s commits" % (striprev))

        backup = "none" if opts.get("no-backup") else "all"
        repair.strip(ui, repo, nodelist, backup=backup, topic="sqlrecover")

        stripsize = min(stripsize * 5, 10000)

    if len(repo) == 0:
        ui.warn(_("unable to fix repo corruption\n"))
    elif len(repo) == reposize:
        ui.status(_("local repo was not corrupt - no action taken\n"))
    else:
        ui.status(_("local repo now matches SQL\n"))

@command('^sqlstrip', [
    ('', 'i-know-what-i-am-doing', None, _('only run sqlstrip if you know ' +
        'exactly what you\'re doing')),
    ], _('hg sqlstrip [OPTIONS] REV'))
def sqlstrip(ui, rev, *args, **opts):
    """strips all revisions greater than or equal to the given revision from the sql database

    Deletes all revisions with linkrev >= the given revision from the local
    repo and from the sql database. This is permanent and cannot be undone.
    Once the revisions are deleted from the database, you will need to run
    this command on each master server before proceeding to write new revisions.
    """

    if not opts.get('i_know_what_i_am_doing'):
        raise util.Abort("You must pass --i-know-what-i-am-doing to run this " +
            "command. If you have multiple servers using the database, this " +
            "command will break your servers until you run it on each one. " +
            "Only the Mercurial server admins should ever run this.")

    global initialsync
    initialsync = INITIAL_SYNC_DISABLE
    repo = hg.repository(ui, ui.environ['PWD'])
    repo.disablesync = True

    try:
        rev = int(rev)
    except ValueError:
        raise util.Abort("specified rev must be an integer: '%s'" % rev)

    lock = repo.lock()
    try:
        repo.sqlconnect()
        try:
            repo.sqllock(writelock)

            if rev not in repo:
                raise util.Abort("revision %s is not in the repo" % rev)

            reponame = repo.sqlreponame
            cursor = repo.sqlcursor
            changelog = repo.changelog

            revs = repo.revs('%s:' % rev)
            # strip locally
            ui.status("stripping locally\n")
            repair.strip(ui, repo, [changelog.node(r) for r in revs], "all")

            ui.status("stripping from the database\n")
            ui.status("deleting old references\n")
            cursor.execute("""DELETE FROM revision_references WHERE repo = %s""", (reponame,))

            ui.status("adding new head references\n")
            for head in repo.heads():
                cursor.execute("""INSERT INTO revision_references(repo, namespace, value)
                               VALUES(%s, 'heads', %s)""",
                               (reponame, hex(head)))

            ui.status("adding new tip reference\n")
            cursor.execute("""INSERT INTO revision_references(repo, namespace, name, value)
                           VALUES(%s, 'tip', 'tip', %s)""",
                           (reponame, len(repo) - 1))

            ui.status("adding new bookmark references\n")
            for k, v in repo._bookmarks.iteritems():
                cursor.execute("""INSERT INTO revision_references(repo, namespace, name, value)
                               VALUES(%s, 'bookmarks', %s, %s)""",
                               (reponame, k, hex(v)))

            ui.status("deleting revision data\n")
            cursor.execute("""DELETE FROM revisions WHERE repo = %s and linkrev > %s""",
                              (reponame, rev))

            repo.sqlconn.commit()
        finally:
            repo.sqlunlock(writelock)
            repo.sqlclose()
    finally:
        lock.release()
