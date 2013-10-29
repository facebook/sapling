# db.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

#CREATE TABLE revs(
#id INT(2) NOT NULL AUTO_INCREMENT PRIMARY KEY,
#path VARCHAR(256) NOT NULL,
#linkrev INT NOT NULL,
#entry BINARY(64) NOT NULL,
#data0 CHAR(1),
#data1 LONGBLOB,
#INDEX linkrev_index (linkrev)
#);

#CREATE TABLE headsbookmarks(
#id INT(2) NOT NULL AUTO_INCREMENT PRIMARY KEY,
#node char(40) NOT NULL,
#name VARCHAR(256) UNIQUE,
#);

# SET OPTION SQL_BIG_SELECTS = 1;

testedwith = 'internal'

from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import changelog, error, cmdutil, revlog, localrepo, transaction
from mercurial import wireproto, bookmarks
import MySQLdb, struct, time
from MySQLdb import cursors

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

disablesync = False

class CorruptionException(Exception):
    pass

def uisetup(ui):
    wrapfunction(revlog.revlog, '_addrevision', addrevision)
    wrapfunction(localrepo, 'instance', repoinstance)
    wrapfunction(transaction.transaction, '_abort', transactionclose)
    wrapfunction(transaction.transaction, 'close', transactionclose)
    wrapfunction(wireproto, 'unbundle', unbundle)
    wrapfunction(bookmarks.bmstore, 'write', bookmarkwrite)

    wireproto.commands['unbundle'] = (wireproto.unbundle, 'heads')

def repoinstance(orig, *args):
    repo = orig(*args)
    if repo.ui.configbool("hgsql", "enabled"):
        conn = MySQLdb.connect(**dbargs)
        cur = conn.cursor()
        try:
            pulldb(repo, cur)
        finally:
            cur.close()
            conn.close()

    return repo

def reposetup(ui, repo):
    if repo.ui.configbool("hgsql", "enabled"):
        ui.setconfig("hooks", "pretxnchangegroup.remotefilelog", pretxnchangegroup)
        ui.setconfig("hooks", "pretxncommit.remotefilelog", pretxnchangegroup)

# Sync with db

def needsync(repo, cur):
    # Check latest db rev number
    cur.execute("SELECT * FROM headsbookmarks")
    sqlheads = set()
    sqlbookmarks = {}
    for _, node, name in cur:
        if not name:
            sqlheads.add(bin(node))
        else:
            sqlbookmarks[name] = bin(node)
    
    heads = repo.heads()
    bookmarks = repo._bookmarks

    if (not sqlheads or len(heads) != len(sqlheads) or 
        len(bookmarks) != len(sqlbookmarks)):
        return True

    for head in sqlheads:
        if head not in heads:
            return True

    for bookmark in sqlbookmarks:
        if (not bookmark in bookmarks or
            bookmarks[bookmark] != sqlbookmarks[bookmark]):
            return True

    return False

def pulldb(repo, cur):
    global disablesync

    if not needsync(repo, cur):
        return

    repo.ui.debug("syncing with mysql\n")

    lock = None
    try:
        lock = repo.lock(wait=False)
    except error.LockHeld:
        # If the lock is held, someone else is doing the pull for us.
        # Wait until they are done.
        # TODO: is actually true?
        lock = repo.lock()
        lock.release()
        return

    transaction = repo.transaction("pulldb")

    revlogs = {}
    try:
        # Refresh the changelog now that we have the lock
        del repo.changelog
        cl = repo.changelog
        clrev = len(cl) - 1

        count = 1
        chunksize = 5000
        while count:
            # Fetch new revs from db
            cur.execute("SELECT * FROM revs WHERE linkrev > %s AND linkrev < %s ORDER BY id ASC", (clrev, clrev + chunksize))

            # Add the new revs
            newentries = addentries(repo, cur, transaction, revlogs)
            clrev += chunksize - 1

            if newentries > 35000 and chunksize > 1000:
                chunksize -= 100
            if newentries < 25000:
                chunksize += 100

            count += newentries
            if count > 50000 or newentries == 0:
                #print "Flushing (chunksize %s)" % chunksize
                count = 1
                for revlog in revlogs.itervalues():
                    if not revlog.ifh.closed:
                        revlog.ifh.flush()
                        revlog.ifh.close()
                    if revlog.dfh and not revlog.dfh.closed:
                        revlog.dfh.flush()
                        revlog.dfh.close()
                revlogs = {}

            if newentries == 0:
                break

        transaction.close()
    finally:
        for revlog in revlogs.itervalues():
            if not revlog.ifh.closed:
                revlog.ifh.close()
            if revlog.dfh and not revlog.dfh.closed:
                revlog.dfh.close()
        transaction.release()
        lock.release()

    del repo.changelog

    disablesync = True
    try:
        cur.execute("SELECT * FROM headsbookmarks WHERE name IS NOT NULL")
        bm = repo._bookmarks
        bm.clear()
        for _, node, name in cur:
            node = bin(node)
            if node in repo:
                bm[name] = node
        bm.write()
    finally:
        disablesync = False

def addentries(repo, revisions, transaction, revlogs):
    opener = repo.sopener

    results = False
    latest = 0
    count = 0

    # TODO: write filelogs, then manifests, then changelogs
    for revdata in revisions:
        results = True
        _, path, link, entry, data0, data1 = revdata
        if link > latest:
            latest = link
        count += 1
        revlog = revlogs.get(path)
        if not revlog:
            revlog = EntryRevlog(opener, path)
            revlogs[path] = revlog

        if not hasattr(revlog, 'ifh') or revlog.ifh.closed:
            dfh = None
            if not revlog._inline:
                dfh = opener(revlog.datafile, "a")
            ifh = opener(revlog.indexfile, "a+")
            revlog.ifh = ifh
            revlog.dfh = dfh

        revlog.addentry(transaction, revlog.ifh, revlog.dfh, entry,
                        data0, data1)

    return count

class EntryRevlog(revlog.revlog):
    def addentry(self, transaction, ifh, dfh, entry, data0, data1):
        curr = len(self)
        offset = self.end(curr)

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
        actualoffset = self.end(curr - 1)
        if expectedoffset != 0 and expectedoffset != actualoffset:
            raise CorruptionException("revision offset doesn't match prior length " +
                "(%s offset vs %s length): %s" %
                (expectedoffset, actualoffset, self.indexfile))

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

# Handle incoming commits

conn = None
cur = None

def unbundle(orig, repo, proto, heads):
    global conn
    global cur
    conn = MySQLdb.connect(**dbargs)
    conn.query("SELECT GET_LOCK('commit_lock', 60)")
    result = conn.store_result().fetch_row()[0][0]
    if result != 1:
        raise Exception("unable to obtain write lock")

    cur = conn.cursor()
    try:
        # TODO: Verify we are synced with the server
        pulldb(repo, cur)
        return orig(repo, proto, heads)
    finally:
        cur.close()
        conn.query("SELECT RELEASE_LOCK('commit_lock')")
        conn.close()
        cur = None
        conn = None

pending = []

class interceptopener(object):
    def __init__(self, fp, onwrite):
        object.__setattr__(self, 'fp', fp)
        object.__setattr__(self, 'onwrite', onwrite)

    def write(self, data):
        self.fp.write(data)
        self.onwrite(data)

    def __getattr__(self, attr):
        return getattr(self.fp, attr)

    def __setattr__(self, attr, value):
        return setattr(self.fp, attr, value)

    def __delattr__(self, attr):
        return delattr(self.fp, attr)

def addrevision(orig, self, node, text, transaction, link, p1, p2,
                cachedelta, ifh, dfh):
    entry = []
    data0 = []
    data1 = []
    def iwrite(data):
        if not entry:
            # sometimes data0 is skipped
            if data0 and not data1:
                data1.append(data0[0])
                del data0[:]
            entry.append(data)
        elif not data0:
            data0.append(data)
        elif not data1:
            data1.append(data)

    def dwrite(data):
        if not data0:
            data0.append(data)
        elif not data1:
            data1.append(data)

    iopener = interceptopener(ifh, iwrite)
    dopener = interceptopener(dfh, dwrite) if dfh else None

    result = orig(self, node, text, transaction, link, p1, p2, cachedelta,
                  iopener, dopener)

    try:
        pending.append((-1, self.indexfile, link, entry[0], data0[0] if data0 else '', data1[0]))
    except:
        import pdb
        pdb.set_trace()
        raise

    return result

def pretxnchangegroup(ui, repo, *args, **kwargs):
    if conn == None:
        raise Exception("Invalid update. Only hg push is allowed.")

    # Commit to db
    try:
        for revision in pending:
            _, path, linkrev, entry, data0, data1 = revision
            cur.execute("""INSERT INTO revs(path, linkrev, entry, data0, data1)
                VALUES(%s, %s, %s, %s, %s)""", (path, linkrev, entry, data0, data1))

        cur.execute("""DELETE FROM headsbookmarks WHERE name IS NULL""")

        for head in repo.heads():
            cur.execute("""INSERT INTO headsbookmarks(node) VALUES(%s)""",
                (hex(head)))

        conn.commit()
    except Exception:
        conn.rollback()
        raise
    finally:
        del pending[:]

def bookmarkwrite(orig, self):
    if disablesync:
        return orig(self)

    conn = MySQLdb.connect(**dbargs)
    conn.query("SELECT GET_LOCK('bookmark_lock', 60)")
    result = conn.store_result().fetch_row()[0][0]
    if result != 1:
        raise Exception("unable to obtain write lock")
    try:
        cur = conn.cursor()

        cur.execute("""DELETE FROM headsbookmarks WHERE name IS NOT NULL""")

        for k, v in self.iteritems():
            cur.execute("""INSERT INTO headsbookmarks(node, name) VALUES(%s, %s)""",
                (hex(v), k))
        conn.commit()
        return orig(self)
    finally:
        cur.close()
        conn.query("SELECT RELEASE_LOCK('bookmark_lock')")
        conn.close()

def transactionclose(orig, self):
    result = orig(self)
    if self.count == 0:
        del pending[:]
    return result
