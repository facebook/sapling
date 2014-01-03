# hgsql.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""
CREATE TABLE revisions(
repo CHAR(32) NOT NULL,
autoid INT UNSIGNED NOT NULL AUTO_INCREMENT,
path VARCHAR(256) NOT NULL,
chunk INT UNSIGNED NOT NULL,
chunkcount INT UNSIGNED NOT NULL,
linkrev INT UNSIGNED NOT NULL,
entry BINARY(64) NOT NULL,
data0 CHAR(1) NOT NULL,
data1 LONGBLOB NOT NULL,
createdtime DATETIME NOT NULL,
INDEX autoid_index (autoid),
PRIMARY KEY (repo, linkrev, autoid)
);

CREATE TABLE pushkeys(
autoid INT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,
repo CHAR(32) NOT NULL,
namespace CHAR(32) NOT NULL,
name VARCHAR(256),
value char(40) NOT NULL,
UNIQUE KEY bookmarkindex (repo, namespace, name)
);
"""

from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import changelog, error, cmdutil, revlog, localrepo, transaction
from mercurial import wireproto, bookmarks, repair, commands, hg, mdiff, phases
import MySQLdb, struct, time, Queue, threading, _mysql_exceptions
from MySQLdb import cursors

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

bookmarklock = 'bookmark_lock'
commitlock = 'commit_lock'

maxrecordsize = 1024 * 1024
disableinitialsync = False

class CorruptionException(Exception):
    pass

def uisetup(ui):
    wrapcommand(commands.table, 'pull', pull)
    wrapfunction(wireproto, 'unbundle', unbundle)
    wireproto.commands['unbundle'] = (wireproto.unbundle, 'heads')

    def writeentry(orig, self, transaction, ifh, dfh, entry, data, link, offset):
        transaction.repo.pendingrevs.append((-1, self.indexfile, link,
            entry, data[0] if data[0] else '', data[1]))
        return orig(self, transaction, ifh, dfh, entry, data, link, offset)
    wrapfunction(revlog.revlog, '_writeentry', writeentry)

    wrapfunction(revlog.revlog, 'addgroup', addgroup)
    wrapfunction(bookmarks.bmstore, 'write', bookmarkwrite)

def reposetup(ui, repo):
    if repo.ui.configbool("hgsql", "enabled"):
        wraprepo(repo)

        if not disableinitialsync:
            # Use a noop to force a sync
            def noop():
                pass
            executewithsql(repo, noop)

# Handle incoming commits
def unbundle(orig, *args, **kwargs):
    repo = args[0]
    return executewithsql(repo, orig, commitlock, *args, **kwargs)

def pull(orig, *args, **kwargs):
    repo = args[1]
    return executewithsql(repo, orig, commitlock, *args, **kwargs)

def executewithsql(repo, action, lock=None, *args, **kwargs):
    repo.sqlconnect()
    if lock:
        repo.sqllock(lock)

    result = None
    success = False
    try:
        repo.syncdb()
        result = action(*args, **kwargs)
        success = True
    finally:
        try:
            if lock:
                repo.sqlunlock(lock)
            repo.sqlclose()
        except _mysql_exceptions.ProgrammingError, ex:
            if success:
                raise
            # if the action caused an exception, hide sql cleanup exceptions,
            # so the real exception is propagated up
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
            self.sqlconn.query("SET SESSION wait_timeout=300;")
            self.sqlcursor = self.sqlconn.cursor()

        def sqlclose(self):
            self.sqlcursor.close()
            self.sqlconn.close()
            self.sqlcursor = None
            self.sqlconn = None

        def sqllock(self, name, timeout=60):
            name = self.sqlconn.escape_string("%s_%s" % (name, self.sqlreponame))
            # cast to int to prevent passing bad sql data
            timeout = int(timeout)
            self.sqlconn.query("SELECT GET_LOCK('%s', %s)" % (name, timeout))
            result = self.sqlconn.store_result().fetch_row()[0][0]
            if result != 1:
                raise Exception("unable to obtain %s lock" % name)

        def sqlunlock(self, name):
            name = self.sqlconn.escape_string("%s_%s" % (name, self.sqlreponame))
            self.sqlconn.query("SELECT RELEASE_LOCK('%s')" % (name,))
            self.sqlconn.store_result().fetch_row()

        def transaction(self, *args, **kwargs):
            tr = super(sqllocalrepo, self).transaction(*args, **kwargs)

            def transactionclose(orig):
                if tr.count == 1:
                    self.committodb()
                    del self.pendingrevs[:]
                return orig()

            def transactionabort(orig):
                del self.pendingrevs[:]
                return orig()

            wrapfunction(tr, "_abort", transactionabort)
            wrapfunction(tr, "close", transactionclose)
            tr.repo = self
            return tr

        def needsync(self):
            """Returns True if the local repo is not in sync with the database.
            If it returns False, the heads and bookmarks match the database.
            """
            self.sqlcursor.execute("""SELECT namespace, name, value
                FROM pushkeys WHERE repo = %s""", (self.sqlreponame))
            sqlheads = set()
            sqlbookmarks = {}
            for namespace, name, node in self.sqlcursor:
                if namespace == "heads":
                    sqlheads.add(bin(node))
                elif namespace == "bookmarks":
                    sqlbookmarks[name] = bin(node)

            heads = set(self.heads())
            bookmarks = self._bookmarks

            return heads != sqlheads or bookmarks != sqlbookmarks

        def syncdb(self):
            if not self.needsync():
                return

            ui = self.ui
            ui.debug("syncing with mysql\n")

            lock = self.lock()
            try:

                # someone else may have synced us while we were waiting
                if not self.needsync():
                    return

                transaction = self.transaction("syncdb")

                try:
                    # Inspect the changelog now that we have the lock
                    fetchstart = len(self.changelog)

                    queue = Queue.Queue()
                    abort = threading.Event()

                    t = threading.Thread(target=self.fetchthread,
                        args=(queue, abort, fetchstart))
                    t.setDaemon(True)
                    try:
                        t.start()
                        addentries(self, queue, transaction)
                    finally:
                        abort.set()

                    phases.advanceboundary(self, phases.public, self.heads())

                    transaction.close()
                finally:
                    transaction.release()

                # We circumvent the changelog and manifest when we add entries to
                # the revlogs. So clear all the caches.
                self.invalidate()
                self.invalidatedirstate()
                self.invalidatevolatilesets()

                # Manually clear the filecache.
                unfiltered = self.unfiltered()
                for k in self._filecache.iterkeys():
                    if k in unfiltered.__dict__:
                        del unfiltered.__dict__[k]
                self._filecache.clear()

                self.disablesync = True
                try:
                    bm = self._bookmarks
                    bm.clear()
                    self.sqlcursor.execute("""SELECT name, value FROM pushkeys
                        WHERE namespace = 'bookmarks' AND repo = %s""",
                        (self.sqlreponame))
                    for name, node in self.sqlcursor:
                        node = bin(node)
                        if node in self:
                            bm[name] = node
                    bm.write()
                finally:
                    self.disablesync = False
            finally:
                lock.release()

        def fetchthread(self, queue, abort, fetchstart):
            ui = self.ui
            clrev = fetchstart
            chunksize = 1000
            while True:
                if abort.isSet():
                    break

                self.sqlcursor.execute("""SELECT path, chunk, chunkcount,
                    linkrev, entry, data0, data1 FROM revisions WHERE repo = %s
                    AND linkrev > %s AND linkrev < %s ORDER BY linkrev ASC""",
                    (self.sqlreponame, clrev - 1, clrev + chunksize))

                # put split chunks back together
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
                        fullchunk[7] = data1
                        fullrevisions.append(tuple(fullchunk))
                    else:
                        raise Exception("missing revision chunk - expected %s got %s" %
                            (chunkcount, len(chunks)))

                fullrevisions = sorted(fullrevisions, key=lambda revdata: revdata[3])
                for revdata in fullrevisions:
                    queue.put(revdata)

                clrev += chunksize
                if (clrev - fetchstart) % 5000 == 0:
                    ui.debug("Queued %s\n" % (clrev))

            queue.put(False)

        def committodb(self):
            if self.sqlconn == None:
                raise Exception("invalid repo change - only hg push and pull are allowed")

            # Commit to db
            try:
                reponame = self.sqlreponame
                cursor = self.sqlcursor
                for revision in self.pendingrevs:
                    _, path, linkrev, entry, data0, data1 = revision

                    start = 0
                    chunk = 0
                    datalen = len(data1)
                    chunkcount = datalen / maxrecordsize
                    if datalen % maxrecordsize != 0 or datalen == 0:
                        chunkcount += 1
                    while chunk == 0 or start < len(data1):
                        end = min(len(data1), start + maxrecordsize)
                        datachunk = data1[start:end]
                        cursor.execute("""INSERT INTO revisions(repo, path, chunk,
                            chunkcount, linkrev, entry, data0, data1, createdtime)
                            VALUES(%s, %s, %s, %s, %s, %s, %s, %s, %s)""",
                            (reponame, path, chunk, chunkcount, linkrev,
                             entry, data0, datachunk, time.strftime('%Y-%m-%d %H:%M:%S')))
                        chunk += 1
                        start = end

                cursor.execute("""DELETE FROM pushkeys WHERE repo = %s
                               AND namespace = 'heads'""", (reponame))

                for head in self.heads():
                    cursor.execute("""INSERT INTO pushkeys(repo, namespace, value)
                                   VALUES(%s, 'heads', %s)""",
                                   (reponame, hex(head)))

                self.sqlconn.commit()
            except:
                self.sqlconn.rollback()
                raise
            finally:
                del self.pendingrevs[:]

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

    repo.sqlreponame = ui.config("hgsql", "reponame")
    if not repo.sqlreponame:
        raise Exception("missing hgsql.reponame")
    repo.sqlargs = sqlargs
    repo.sqlconn = None
    repo.sqlcursor = None
    repo.disablesync = False
    repo.pendingrevs = []
    repo.__class__ = sqllocalrepo

class bufferedopener(object):
    def __init__(self, opener, path, mode):
        self.opener = opener
        self.path = path
        self.mode = mode
        self.buffer = []
        self.closed = False

    def write(self, value):
        if self.closed:
            raise Exception("")
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
    opener = repo.sopener

    revlogs = {}
    def writeentry(revdata):
        path, chunk, chunkcount, link, entry, data0, data1 = revdata
        revlog = revlogs.get(path)
        if not revlog:
            revlog = EntryRevlog(opener, path)
            revlogs[path] = revlog

        if not hasattr(revlog, 'ifh') or revlog.ifh.closed:
            dfh = None
            if not revlog._inline:
                dfh = bufferedopener(opener, revlog.datafile, "a")
            ifh = bufferedopener(opener, revlog.indexfile, "a+")
            revlog.ifh = ifh
            revlog.dfh = dfh

        revlog.addentry(transaction, revlog.ifh, revlog.dfh, entry,
                        data0, data1)
        revlog.dirty = True

    clrev = len(repo)
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
                raise Exception("SQL data is not in linkrev order")
            else:
                leftover = revdata
                currentlinkrev = linkrev
                break

        if not revisions:
            continue

        for revdata in revisions:
            writeentry(revdata)

        clrev += 1

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
    def addentry(self, transaction, ifh, dfh, entry, data0, data1):
        curr = len(self)
        offset = self.end(curr - 1)

        e = struct.unpack(revlog.indexformatng, entry)
        offsettype, datalen, textlen, base, link, p1r, p2r, node = e
        if curr == 0:
            elist = list(e)
            type = revlog.gettype(offsettype)
            offsettype = revlog.offset_type(0, type)
            elist[0] = offsettype
            e = tuple(elist)

        # Verify that the revlog is in a good state
        if p1r >= curr or p2r >= curr:
            raise CorruptionException("parent revision is not in revlog: %s" % self.indexfile)
        if base > curr:
            raise CorruptionException("base revision is not in revlog: %s" % self.indexfile)

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
    """
    copy paste of revlog.addgroup, but we ensure that the revisions are added
    in linkrev order.
    """
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

            reorders = 0
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
                    reorders += 1

                prevnode = node
                visited.add(node)

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
            self._addrevision(node, None, transaction, link,
                                      p1, p2, (baserev, delta), ifh, dfh)
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

def bookmarkwrite(orig, self):
    repo = self._repo
    if repo.disablesync:
        return orig(self)

    def commitbookmarks():
        try:
            cursor = repo.sqlcursor
            cursor.execute("""DELETE FROM pushkeys WHERE repo = %s AND
                           namespace = 'bookmarks'""", (repo.sqlreponame))

            for k, v in self.iteritems():
                cursor.execute("""INSERT INTO pushkeys(repo, namespace, name, value)
                               VALUES(%s, 'bookmarks', %s, %s)""",
                               (repo.sqlreponame, k, hex(v)))
            repo.sqlconn.commit()
            return orig(self)
        except:
            repo.sqlconn.rollback()
            raise

    executewithsql(repo, commitbookmarks, bookmarklock)

# recover must be a norepo command because loading the repo fails
commands.norepo += " sqlrecover"

@command('^sqlrecover', [
    ('f', 'force', '', _('strips as far back as necessary'), ''),
    ], _('hg sqlrecover'))
def sqlrecover(ui, *args, **opts):
    """
    Strips commits from the local repo until it is back in sync with the SQL
    server.
    """

    global disableinitialsync
    disableinitialsync = True
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
    while iscorrupt():
        if reposize > len(repo) + 10000:
            ui.warn("unable to fix repo after stripping 10000 commits (use -f to strip more)")
        striprev = max(0, len(repo) - stripsize)
        nodelist = [repo[striprev].node()]
        repair.strip(ui, repo, nodelist, backup="none", topic="sqlrecover")
        stripsize *= 5

    if len(repo) == 0:
        ui.warn(_("unable to fix repo corruption\n"))
    elif len(repo) == reposize:
        ui.status(_("local repo was not corrupt - no action taken\n"))
    else:
        ui.status(_("local repo now matches SQL\n"))
