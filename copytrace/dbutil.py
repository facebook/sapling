# dbutil.py
#
# Util functions to interact with the moves/copy database
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import scmutil, util, commands
import bundle2
import time

import sqlite3
# user servers don't need to have the mysql module
try:
    import mysql.connector
except:
    pass

localdb = 'moves.db'
remote = False
remoteargs = {}


def _sqlcmds(name):
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

    elif name == 'retrievehashes':
        if not remote:
            return 'SELECT DISTINCT hash ' + \
                   'FROM Moves ' + \
                   'WHERE hash IN (%s);'

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
    if not remote:
        dbname = localdb
        conn = sqlite3.connect(repo.vfs.join(dbname))
        cursor = conn.cursor()

    # Remote SQL db
    else:
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

    _exists(cursor)
    return dbname, conn, cursor


def _initremote(repo):
    """
    detects if its the server or the client
    """
    global remote
    ui = repo.ui
    remote = ui.configbool("copytrace", "remote", False)
    if remote and not remoteargs:
        remoteargs['host'] = ui.config("copytrace", "xdbhost")
        remoteargs['database'] = ui.config("copytrace", "xdb")
        remoteargs['user'] = ui.config("copytrace", "xdbuser")
        remoteargs['port'] = ui.configint("copytrace", "xdbport")
        remoteargs['password'] = ui.config("copytrace", "xdbpassword")


def _close(conn, cursor):
    cursor.close()
    conn.close()


def _exists(cursor):
    """
    checks the existence of the Moves table and creates it if it doesn't
    """
    cursor.execute(_sqlcmds('tableexists'))
    table = cursor.fetchall()
    if not table:
        cursor.execute(_sqlcmds('createtable'))


def insertitem(cursor, ctxhash, dic, move, repo):
    """
    inserts {dst:src} in the database using the cursor
    """
    mv = '1' if move else '0'
    insertcmd = _sqlcmds('insertctx')

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


def insertrawdata(repo, dic):
    """
    inserts dict = {ctxhash: [src, dst, mv]} for moves and copies into the
    database
    """
    dbname, conn, cursor = _connect(repo)

    for ctxhash, mvlist in dic.iteritems():
        for src, dst, mv in mvlist:
            if src == 'None' and dst == 'None':
                src = None
                dst = None
            insertdata = (repo.root, ctxhash, src, dst, mv)
            cursor.execute(_sqlcmds('insertctx'), insertdata)
    conn.commit()

    _close(conn, cursor)


def retrievedatapkg(repo, ctxlist, move=False, askserver=True):
    """
    retrieves {ctxhash: {dst: src}} for ctxhash in ctxlist for moves or copies
    """
    # Do we want moves or copies
    mv = '1' if move else '0'
    token = '%s' if remote else '?'

    dbname, conn, cursor = _connect(repo)

    # Returns : hash, src, dst
    cursor.execute(_sqlcmds('retrievemoves') %
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

    processed = ret.keys()
    missing = [f for f in ctxlist if f not in processed]

    # The local database doesn't have the data for this ctx and hasn't tried
    # to retrieve it yet (firstcheck)
    if askserver and not remote and missing:
        _requestdata(repo, missing)
        add = retrievedatapkg(repo, missing, move=move, askserver=False)
        ret.update(add)

    return ret


def retrieverawdata(repo, ctxlist, askserver=True):
    """
    retrieves {ctxhash: [src, dst, mv]} for ctxhash in ctxlist for moves or
    copies
    """
    dbname, conn, cursor = _connect(repo)
    token = '%s' if remote else '?'

    # Returns: hash, src, dst, mv
    cursor.execute(_sqlcmds('retrieveraw') %
                   (','.join([token] * len(ctxlist)), token),
                   ctxlist + [repo.root])

    all_rows = cursor.fetchall()
    _close(conn, cursor)

    ret = {}
    # Building the mvdict and cpdict for each ctxhash:
    for ctxhash, src, dst, mv in all_rows:
        # No move or No copy
        if not src and not dst:
            src = 'None'
            dst = 'None'
        ret.setdefault(ctxhash.encode('utf8'), []).append((src.encode('utf8'),
             dst.encode('utf8'), mv.encode('utf8')))

    processed = ret.keys()
    missing = [f for f in ctxlist if f not in processed]

    # The local database doesn't have the data for this ctx and hasn't tried
    # to retrieve it yet (askserver)
    if askserver and not remote and missing:
        _requestdata(repo, missing)
        add = retrieverawdata(repo, missing, move=move,
                              askserver=False)
        ret.update(add)

    return ret


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
    cursor.execute(_sqlcmds('deletectx'), deletedata)
    conn.commit()
    _close(conn, cursor)


def checkpresence(repo, ctxlist):
    """
    checks if the ctx in ctxlist are in the local database or requests for it
    """
    ctxhashs = [ctx.hex() for ctx in ctxlist]
    dbname, conn, cursor = _connect(repo)
    # Returns hash
    cursor.execute(_sqlcmds('retrievehashes')
                   % (','.join('?' * len(ctxhashs))),
                   ctxhashs)
    processed = cursor.fetchall()
    _close(conn, cursor)
    processed = [ctx[0].encode('utf8') for ctx in processed]
    missing = [repo[f].node() for f in ctxlist if f not in processed]
    if missing:
        _requestdata(repo, missing)


def _requestdata(repo, nodelist):
    """
    Requests missing ctx data to a server
    """
    bundle2.pullmoves(repo, nodelist)
