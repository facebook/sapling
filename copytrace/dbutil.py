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
import sqlite3


localdb = 'moves.db'
remotedb = 'moves.db'  # Will be modified to the XDB database


def _connect(repo, remote):
    if remote:
        dbname = remotedb
    else:
        dbname = localdb

    _exists(repo, dbname)
    try:
        conn = sqlite3.connect(repo.vfs.join(dbname))
        c = conn.cursor()
    except:
        raise util.Abort('could not open the %s local database' % dbname)
    return dbname, conn, c


def _close(conn):
    conn.close()


def _exists(repo, dbname):
    """
    checks the existence of the database or creates it
    """
    try:
        conn = sqlite3.connect(repo.vfs.join(dbname))
        c = conn.cursor()
        c.execute("SELECT name FROM sqlite_master WHERE type='table'" +
                  " AND name='Moves';")
        table = c.fetchall()
        if not table:
            c.execute('CREATE TABLE Moves(hash CHAR(40), source TEXT, ' +
                      'destination TEXT, mv CHAR(1));')
        _close(conn)
    except:
        raise util.Abort('could not create the %s local database' % dbname)


def insertitem(cursor, ctxhash, dic, move):
    """
    inserts {dst:src} in the database using the cursor
    """
    mv = '1' if move else '0'
    insertcmd = 'INSERT INTO Moves VALUES(?, ?, ?, ?);'
    # No rename in this ctx
    if dic == {}:
        cursor.execute(insertcmd, (ctxhash, None, None, mv))
    else:
        for dst, src in dic.iteritems():
            cursor.execute(insertcmd, (ctxhash, src, dst, mv))


def insertdata(repo, ctx, mvdict, cpdict, remote=False):
    """
    inserts the mvdict/cpdict = {dst: src} data in the database with '1' if it
    is a move, '0' if it is a copy
    """
    dbname, conn, c = _connect(repo, remote)

    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex())
    try:
        insertitem(c, ctxhash, mvdict, True)
        insertitem(c, ctxhash, cpdict, False)
        conn.commit()
    except:
        raise util.Abort('could not insert data into the %s database' % dbname)

    _close(conn)


def insertrawdata(repo, dic, remote=False):
    """
    inserts dict = {ctxhash: [src, dst, mv]} for moves and copies into the
    database
    """
    dbname, conn, c = _connect(repo, remote)
    insertcmd = 'INSERT INTO Moves VALUES(?, ?, ?, ?);'
    try:
        for ctxhash, mvlist in dic.iteritems():
            for src, dst, mv in mvlist:
                if src == 'None' and dst == 'None':
                    src = None
                    dst = None
                c.execute(insertcmd, (ctxhash, src, dst, mv))
        conn.commit()
    except:
        raise util.Abort('could not insert data into the %s database' % dbname)

    _close(conn)


def retrievedatapkg(repo, ctxlist, move=False, remote=False, askserver=True):
    """
    retrieves {ctxhash: {dst: src}} for ctxhash in ctxlist for moves or copies
    """
    # Do we want moves or copies
    mv = '1' if move else '0'

    dbname, conn, c = _connect(repo, remote)
    try:
        c.execute('SELECT DISTINCT hash, source, destination FROM Moves' +
                  ' WHERE hash IN (%s) AND mv = ?'
                  % (','.join('?' * len(ctxlist))), ctxlist + [mv])
    except:
        raise util.Abort('could not access data from the %s database' % dbname)

    all_rows = c.fetchall()
    _close(conn)

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
        add = retrievedatapkg(repo, missing, move=move, remote=remote,
                              askserver=False)
        ret.update(add)

    return ret


def retrieverawdata(repo, ctxlist, remote=False, askserver=True):
    """
    retrieves {ctxhash: [src, dst, mv]} for ctxhash in ctxlist for moves or copies
    """
    dbname, conn, c = _connect(repo, remote)
    try:
        c.execute('SELECT DISTINCT hash, source, destination, mv FROM Moves' +
                  ' WHERE hash IN (%s)'
                  % (','.join('?' * len(ctxlist))), ctxlist)
    except:
        raise util.Abort('could not access data from the %s database' % dbname)

    all_rows = c.fetchall()
    _close(conn)

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
        add = retrieverawdata(repo, missing, move=move, remote=remote,
                              askserver=False)
        ret.update(add)

    return ret


def removectx(repo, ctx, remote=False):
    """
    removes the data concerning the ctx in the database
    """
    dbname, conn, c = _connect(repo, remote)
    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex())
    try:
        c.execute('DELETE FROM Moves WHERE hash = ?', [ctxhash])
        conn.commit()
    except:
        raise util.Abort('could not delete ctx from the %s database' % dbname)

    _close(conn)


def checkpresence(repo, ctxlist):
    """
    checks if the ctx in ctxlist are present in the database or requests for it
    """
    ctxhashs = [ctx.hex() for ctx in ctxlist]
    dbname, conn, c = _connect(repo, False)
    try:
        c.execute('SELECT DISTINCT hash FROM Moves WHERE hash IN (%s)'
                  % (','.join('?' * len(ctxhashs))), ctxhashs)
    except:
        raise util.Abort('could not check ctx presence in the %s database'
                         % dbname)
    processed = c.fetchall()
    _close(conn)
    processed = [ctx[0].encode('utf8') for ctx in processed]
    missing = [repo[f].node() for f in ctxlist if f not in processed]
    if missing:
        _requestdata(repo, missing)


def _requestdata(repo, nodelist):
    """
    Requests missing ctx data to a server
    """
    try:
        bundle2.pullmoves(repo, nodelist)
    except:
        pass
