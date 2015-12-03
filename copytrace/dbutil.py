# dbutil.py
#
# Util functions to interact with the moves/copy database
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import scmutil, util, commands, copies
import bundle2
import time

import sqlite3
# user servers don't need to have the mysql module
try:
    import mysql.connector
except:
    pass

localdb = 'moves.db'


def _sqlcmds(name, remote):
    """
    returns a sql command for the given name and remote (MySQL or sqlite)
    """

    if name == 'tableexists':
        if remote:
            return "SHOW TABLES LIKE 'Moves';"
        else:
            return "SELECT name " + \
                   "FROM sqlite_master " + \
                   "WHERE type='table' AND name='Moves';"

    elif name == 'createtable':
        return 'CREATE TABLE Moves(' + \
                    'repo CHAR(64) NOT NULL, ' + \
                    'hash CHAR(40) NOT NULL, ' + \
                    'source TEXT, ' + \
                    'destination TEXT, ' + \
                    'mv CHAR(1) NOT NULL ' + \
                    ');'

    elif name == 'createindex':
        return 'CREATE INDEX ctxindex ON Moves (repo, hash, mv);'

    elif name == 'insertctx':
        if remote:
            return 'INSERT INTO Moves VALUES (%s, %s, %s, %s, %s);'
        else:
            return 'INSERT INTO Moves VALUES (?, ?, ?, ?, ?);'

    elif name == 'retrievemoves':
        return 'SELECT DISTINCT hash, source, destination ' + \
               'FROM Moves ' + \
               'WHERE hash IN (%s) AND mv = %s AND repo = %s;'

    elif name == 'retrieveraw':
        return 'SELECT DISTINCT hash, source, destination, mv ' + \
               'FROM Moves ' + \
               'WHERE hash IN (%s) and repo = %s;'

    elif name == 'retrieveallraw':
        if not remote:
            return 'SELECT DISTINCT hash, source, destination, mv ' + \
                   'FROM Moves ' + \
                   'WHERE repo = ?'

    elif name == 'retrievehashes':
        if not remote:
            return 'SELECT DISTINCT hash ' + \
                   'FROM Moves ' + \
                   'WHERE hash IN (%s) and repo = %s;'

    elif name == 'deletectx':
        if remote:
                return 'DELETE FROM Moves ' + \
                   'WHERE hash = %s AND repo = %s;'
        else:
            return 'DELETE FROM Moves ' + \
                   'WHERE hash = ? AND repo = ?;'


def _connect(repo):
    """
    Connecting to the local sqlite database or remote MySQL database
    """
    _initremote(repo)

    # Local sqlite db
    if not repo.copytraceremote:
        dbname = localdb
        conn = sqlite3.connect(repo.vfs.join(dbname))
        cursor = conn.cursor()

    # Remote SQL db
    else:
        remoteargs = repo.remoteargs
        dbname = remoteargs['database']
        retry = 3
        while True:
            try:
                conn = mysql.connector.connect(force_ipv6=True, **remoteargs)
                break
            except mysql.connector.errors.Error:
                # mysql can be flakey occasionally, so do some minimal
                # retrying.
                retry -= 1
                if retry == 0:
                    raise
                time.sleep(0.2)
        waittimeout = '300'
        waittimeout = conn.converter.escape("%s" % waittimeout)
        cursor = conn.cursor()
        cursor.execute("SET wait_timeout=%s" % waittimeout)

    _exists(cursor, repo.copytraceremote)
    return dbname, conn, cursor


def _initremote(repo):
    """
    detects if its the server or the client
    """
    ui = repo.ui
    repo.copytraceremote = ui.configbool("copytrace", "remote", False)
    if repo.copytraceremote:
        remoteargs = {}
        remoteargs['host'] = ui.config("copytrace", "xdbhost")
        remoteargs['database'] = ui.config("copytrace", "xdb")
        remoteargs['user'] = ui.config("copytrace", "xdbuser")
        remoteargs['port'] = ui.configint("copytrace", "xdbport")
        remoteargs['password'] = ui.config("copytrace", "xdbpassword")
        repo.remoteargs = remoteargs


def _close(conn, cursor):
    cursor.close()
    conn.close()


def _exists(cursor, remote):
    """
    checks the existence of the Moves table and creates it if it doesn't
    """
    cursor.execute(_sqlcmds('tableexists', remote))
    table = cursor.fetchall()
    if not table:
        cursor.execute(_sqlcmds('createtable', remote))
        cursor.execute(_sqlcmds('createindex', remote))


def insertitem(cursor, ctxhash, dic, move, repo):
    """
    inserts {dst:src} in the database using the cursor
    """
    mv = '1' if move else '0'
    insertcmd = _sqlcmds('insertctx', repo.copytraceremote)

    # No rename in this ctx
    if dic == {}:
        insertdata = (repo.root, ctxhash, None, None, mv)
        cursor.execute(insertcmd, insertdata)

    else:
        for dst, src in dic.iteritems():
            insertdata = (repo.root, ctxhash, src, dst, mv)
            cursor.execute(insertcmd, insertdata)


def insertdata(repo, ctx, mvdict, cpdict):
    """
    inserts the mvdict/cpdict = {dst: src} data in the database with '1' if it
    is a move, '0' if it is a copy
    """
    dbname, conn, cursor = _connect(repo)

    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex())

    insertitem(cursor, ctxhash, mvdict, True, repo)
    insertitem(cursor, ctxhash, cpdict, False, repo)
    conn.commit()

    _close(conn, cursor)


def insertrawdata(repo, dic, mapping={}):
    """
    inserts dict = {ctxhash: [src, dst, mv]} for moves and copies into the
    database
    mapping is the mapping of {oldhash: newhash} due to push rebase
    """
    dbname, conn, cursor = _connect(repo)
    movedhashes = mapping.keys()

    for ctxhash, mvlist in dic.iteritems():
        if ctxhash in movedhashes:
            ctxhash = mapping[ctxhash]

        for src, dst, mv in mvlist:
            if src == 'None' and dst == 'None':
                src = None
                dst = None
            insertdata = (repo.root, ctxhash, src, dst, mv)
            cursor.execute(_sqlcmds('insertctx', repo.copytraceremote),
                           insertdata)
    conn.commit()

    _close(conn, cursor)


def retrievedatapkg(repo, ctxlist, move=False, askserver=True, addmissing=True):
    """
    retrieves {ctxhash: {dst: src}} for ctxhash in ctxlist for moves or copies
    """
    # Checks if the database has the data, else, asks it to the server, or adds
    # it manually
    checkpresence(repo, ctxlist, askserver)

    dbname, conn, cursor = _connect(repo)

    # Do we want moves or copies
    mv = '1' if move else '0'
    token = '%s' if repo.copytraceremote else '?'

    # Returns : hash, src, dst
    cursor.execute(_sqlcmds('retrievemoves', repo.copytraceremote) %
              (','.join([token] * len(ctxlist)), token, token),
              ctxlist + [mv, repo.root])

    all_rows = cursor.fetchall()
    _close(conn, cursor)

    ret = {}
    # Building the mvdict and cpdict for each ctxhash:
    for ctxhash, src, dst in all_rows:
        # No move or No copy
        if not dst:
            ret.setdefault(ctxhash.encode('utf8'), {})
        else:
            ret.setdefault(ctxhash.encode('utf8'), {})[dst.encode('utf8')] = \
                 src.encode('utf8')

    return ret


def _builddict(rows):
    """
    building {ctxhash: [src, dst, mv]} from the database rows
    """
    ret = {}
    for ctxhash, src, dst, mv in rows:
        # No move or No copy
        if not src and not dst:
            src = 'None'
            dst = 'None'
        ret.setdefault(ctxhash.encode('utf8'), []).append((src.encode('utf8'),
             dst.encode('utf8'), mv.encode('utf8')))

    return ret


def retrieverawdata(repo, ctxlist):
    """
    retrieves {ctxhash: [src, dst, mv]} for ctxhash in ctxlist for moves or
    copies
    """
    dbname, conn, cursor = _connect(repo)
    token = '%s' if repo.copytraceremote else '?'

    # Returns: hash, src, dst, mv
    cursor.execute(_sqlcmds('retrieveraw', repo.copytraceremote) %
                   (','.join([token] * len(ctxlist)), token),
                   ctxlist + [repo.root])

    all_rows = cursor.fetchall()
    _close(conn, cursor)

    return _builddict(all_rows)


def retrieveallrawdata(repo):
    """
    retrieves all {ctxhash: [src, dst, mv]} in the local database
    """
    dbname, conn, cursor = _connect(repo)

    # Returns hash, src, dst, mv
    cursor.execute(_sqlcmds('retrieveallraw', repo.copytraceremote),
                   [repo.root])
    all_rows = cursor.fetchall()
    _close(conn, cursor)

    return _builddict(all_rows)


def removectx(repo, ctx):
    """
    removes the data concerning the ctx in the database
    """
    dbname, conn, cursor = _connect(repo)
    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex())
    deletedata = [ctxhash, repo.root]
    cursor.execute(_sqlcmds('deletectx', repo.copytraceremote), deletedata)
    conn.commit()
    _close(conn, cursor)


def checkpresence(repo, ctxhashs, askserver=True):
    """
    checks if the ctx in ctxhashs are in the local database or requests for it
    """
    if askserver:
        missing = _processmissing(repo, ctxhashs, mutable=False)
        # Requests the missing data to the server
        if missing and not repo.copytraceremote:
            _requestdata(repo, missing)

    missing = _processmissing(repo, ctxhashs)
    # Manually adds the still missing data
    if missing:
        _addmissingmoves(repo, missing)

    return missing

def _processmissing(repo, ctxhashs, mutable=True):
    """
    returns the list of missing hashes
    """
    dbname, conn, cursor = _connect(repo)
    # Returns hash
    cursor.execute(_sqlcmds('retrievehashes', repo.copytraceremote)
                   % (','.join('?' * len(ctxhashs)), '?'),
                   ctxhashs + [repo.root])
    processed = cursor.fetchall()
    _close(conn, cursor)
    processed = [ctx[0].encode('utf8') for ctx in processed]
    if mutable:
        missing = [f for f in ctxhashs if f not in processed and f != '0']
    else:
        missing = [f for f in ctxhashs if f not in processed and f != '0' and \
                  not repo[f].mutable()]
    return missing


def _requestdata(repo, nodelist):
    """
    Requests missing ctx data to a server
    """
    bundle2.pullmoves(repo, nodelist)


def _addmissingmoves(repo, ctxlist):
    """
    Manually add missing moves
    """
    # This should only concern client draft commits made before the extension
    for ctxhash in ctxlist:
        ctx = repo[ctxhash]
        m = ctx.manifest()
        cp = copies._forwardcopies(ctx.p1(), ctx)
        mvdict = {}
        cpdict = {}
        for dst, src in cp.iteritems():
            if src in m:
                cpdict[dst] = src
            else:
                mvdict[dst] = src
        insertdata(repo, ctx, mvdict, cpdict)
