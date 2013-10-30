# db.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

#CREATE TABLE revs(
#id INT(2) NOT NULL AUTO_INCREMENT PRIMARY KEY,
#path VARCHAR(256) NOT NULL,
#chunk INT(4) NOT NULL,
#chunkcount INT(4) NOT NULL,
#linkrev INT(4) NOT NULL,
#entry BINARY(64) NOT NULL,
#data0 CHAR(1) NOT NULL,
#data1 LONGBLOB NOT NULL,
#createdtime DATETIME NOT NULL,
#INDEX linkrev_index (linkrev)
#);

#CREATE TABLE headsbookmarks(
#id INT(2) NOT NULL AUTO_INCREMENT PRIMARY KEY,
#node char(40) NOT NULL,
#name VARCHAR(256) UNIQUE
#);

# SET OPTION SQL_BIG_SELECTS = 1;

from mercurial.node import bin, hex, nullid, nullrev
from mercurial.i18n import _
from mercurial.extensions import wrapfunction, wrapcommand
from mercurial import changelog, error, cmdutil, revlog, localrepo, transaction
from mercurial import wireproto, bookmarks, repair, commands, hg, mdiff
import MySQLdb, struct, time
from MySQLdb import cursors

cmdtable = {}
command = cmdutil.command(cmdtable)
testedwith = 'internal'

maxrecordsize = 1024 * 1024

disablesync = False

class CorruptionException(Exception):
    pass

def uisetup(ui):
    wrapcommand(commands.table, 'pull', pull)

    wrapfunction(revlog.revlog, '_addrevision', addrevision)
    wrapfunction(revlog.revlog, 'addgroup', addgroup)
    wrapfunction(localrepo, 'instance', repoinstance)
    wrapfunction(transaction.transaction, '_abort', transactionclose)
    wrapfunction(transaction.transaction, 'close', transactionclose)
    wrapfunction(wireproto, 'unbundle', unbundle)
    wrapfunction(bookmarks.bmstore, 'write', bookmarkwrite)

    wireproto.commands['unbundle'] = (wireproto.unbundle, 'heads')

def repoinstance(orig, *args):
    global disablesync
    repo = orig(*args)
    if repo.ui.configbool("hgsql", "enabled") and not disablesync:
        conn = MySQLdb.connect(**dbargs)
        cur = conn.cursor()
        try:
            syncdb(repo, cur)
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

def syncdb(repo, cur):
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
        # TODO: I don't think this is actually true...
        lock = repo.lock()
        lock.release()
        return

    transaction = repo.transaction("syncdb")

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
            cur.execute("SELECT * FROM revs WHERE linkrev > %s AND linkrev < %s ORDER BY linkrev ASC", (clrev, clrev + chunksize))

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

    def writeentry(revdata):
        _, path, chunk, chunkcount, link, entry, data0, data1, createdtime = revdata
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

    # put split chunks back together
    groupedrevdata = {}
    for revdata in revisions:
        name = revdata[1]
        chunk = revdata[2]
        linkrev = revdata[4]
        groupedrevdata.setdefault((name, linkrev), {})[chunk] = revdata

    fullrevisions = []
    for _, chunks in groupedrevdata.iteritems():
        chunkcount = chunks[0][3]
        if chunkcount == 1:
            fullrevisions.append(chunks[0])
        elif chunkcount == len(chunks):
            fullchunk = list(chunks[0])
            data1 = ""
            for i in range(0, chunkcount):
                data1 += chunks[i][7]
            fullchunk[7] = data1
            fullrevisions.append(tuple(fullchunk))
        else:
            raise Exception("missing revision chunk - expected %s got %s" %
                (chunkcount, len(chunks)))

    fullrevisions = sorted(fullrevisions, key=lambda revdata: revdata[4])

    # Write filelogs first, then manifests, then changelogs,
    # just like Mercurial does normally.
    changelog = []
    manifest = []
    for revdata in fullrevisions:
        name = revdata[1]
        if name == "00changelog.i":
            changelog.append(revdata)
        elif name == "00manifest.i":
            manifest.append(revdata)
        else:
            writeentry(revdata)

    for revdata in manifest:
        writeentry(revdata)

    for revdata in changelog:
        writeentry(revdata)

    return len(fullrevisions)

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
        syncdb(repo, cur)
        return orig(repo, proto, heads)
    finally:
        cur.close()
        conn.query("SELECT RELEASE_LOCK('commit_lock')")
        conn.close()
        cur = None
        conn = None

def pull(orig, ui, repo, source="default", **opts):
    global conn
    global cur
    conn = MySQLdb.connect(**dbargs)
    conn.query("SELECT GET_LOCK('commit_lock', 60)")
    result = conn.store_result().fetch_row()[0][0]
    if result != 1:
        raise Exception("unable to obtain write lock")

    cur = conn.cursor()
    try:
        syncdb(repo, cur)
        return orig(ui, repo, source, **opts)
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

def pretxnchangegroup(ui, repo, *args, **kwargs):
    if conn == None:
        raise Exception("invalid repo change - only hg push and pull are allowed")

    # Commit to db
    try:
        for revision in pending:
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
                cur.execute("""INSERT INTO revs(path, chunk, chunkcount, linkrev, entry, data0,
                    data1, createdtime) VALUES(%s, %s, %s, %s, %s, %s, %s, %s)""",
                    (path, chunk, chunkcount, linkrev, entry, data0, datachunk,
                     time.strftime('%Y-%m-%d %H:%M:%S')))
                chunk += 1
                start = end

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
    global disablesync
    disablesync = True

    repo = hg.repository(ui, ui.environ['PWD'])

    def iscorrupt():
        conn = MySQLdb.connect(**dbargs)
        cur = conn.cursor()
        try:
            syncdb(repo, cur)
        except CorruptionException:
            return True
        finally:
            cur.close()
            conn.close()

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
