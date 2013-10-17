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

# SET OPTION SQL_BIG_SELECTS = 1;

testedwith = 'internal'

from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction
from mercurial import changelog, error, cmdutil, revlog, localrepo, transaction
import MySQLdb, struct, time
from MySQLdb import cursors

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

dbargs = {
    
}

def uisetup(ui):
    wrapfunction(revlog.revlog, '_addrevision', addrevision)
    wrapfunction(localrepo, 'instance', repoinstance)
    wrapfunction(transaction.transaction, '_abort', transactionclose)
    wrapfunction(transaction.transaction, 'close', transactionclose)

def repoinstance(orig, *args):
    repo = orig(*args)
    if repo.ui.configbool("hgsql", "enabled"):
        pulldb(repo)
    return repo

def reposetup(ui, repo):
    if repo.ui.configbool("hgsql", "enabled"):
        ui.setconfig("hooks", "pretxnchangegroup.remotefilelog", pretxnchangegroup)
        ui.setconfig("hooks", "pretxncommit.remotefilelog", pretxnchangegroup)

# Sync with db

def pulldb(repo):
    conn = MySQLdb.connect(**dbargs)
    cur = conn.cursor()
    try:
        # Check latest db rev number
        cur.execute("SELECT MAX(linkrev) FROM revs")
        dbrev = list(cur)[0][0]
        if dbrev == None:
            dbrev = -1
        cl = repo.changelog
        clrev = len(cl) - 1
        if dbrev <= clrev:
            return

        lock = None
        try:
            lock = repo.lock(wait=False)
        except error.LockHeld:
            # If the lock is held, someone else is doing the pull for us.
            # Wait until they are done.
            lock = repo.lock()
            lock.release()
            return

        transaction = repo.transaction("pulldb")
        try:
            # Refresh the changelog now that we have the lock
            del repo.changelog
            cl = repo.changelog
            clrev = len(cl) - 1

            revlogs = {}
            count = 1
            chunksize = 5000
            while count:
                # Fetch new revs from db
                cur.execute("SELECT * FROM revs WHERE linkrev > %s and linkrev < %s ORDER BY id ASC", (clrev, clrev + chunksize))

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
                        revlog.ifh.flush()
                        revlog.ifh.close()
                        if revlog.dfh:
                            revlog.dfh.flush()
                            revlog.dfh.close()
                    revlogs = {}

                if newentries == 0:
                    break

            transaction.close()
        except:
            import traceback
            traceback.print_exc()
            raise
        finally:
            transaction.release()
            lock.release()
    finally:
        cur.close()
        conn.close()

    del repo.changelog

def addentries(repo, revisions, transaction, revlogs):
    opener = repo.sopener
    revlogs = {}

    results = False
    latest = 0
    count = 0
    start = time.time()

    # TODO: write filelogs, then manifests, then changelogs
    for revdata in revisions:
        results = True
        _, path, link, entry, data0, data1 = revdata
        if link > latest:
            latest = link
        count += 1
        revlog = revlogs.get('path')
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

    duration = time.time() - start
    #if count > 0:
        #print "rev: %s (pieces %s) %0.1fs (%0.5f s/p)" % (latest, count, duration, duration / count)
    return count

class EntryRevlog(revlog.revlog):
    def addentry(self, transaction, ifh, dfh, entry, data0, data1):
        e = struct.unpack(revlog.indexformatng, entry)
        if e[7] not in self.nodemap:
            self.index.insert(-1, e)
            self.nodemap[e[7]] = len(self) - 1

        curr = len(self) - 1
        offset = self.end(curr - 1)

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
            #self.checkinlinesize(transaction, ifh)

# Handle incoming commits

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
    # Commit to db
    conn = MySQLdb.connect(**dbargs)
    try:
        cur = conn.cursor()
        for revision in pending:
            _, path, linkrev, entry, data0, data1 = revision
            cur.execute("""INSERT INTO revs(path, linkrev, entry, data0, data1)
                VALUES(%s, %s, %s, %s, %s)""", (path, linkrev, entry, data0, data1))
        conn.commit()
    except:
        conn.rollback()
        raise
    finally:
        conn.close()

    del pending[:]

def transactionclose(orig, self):
    del pending[:]
    return orig(self)
