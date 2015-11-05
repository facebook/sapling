# dbutil.py
#
# Util functions to interact with the moves/copy database
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import scmutil, util
import sqlite3
import os


localdb = 'moves.db'
remotedb = 'moves.db' # Will be modified to the XDB database

def _connect(repo, dbname):
    _exists(repo, dbname)
    try:
        conn = sqlite3.connect(repo.vfs.join(dbname))
        c = conn.cursor()
    except:
        raise util.Abort('could not open the %s local database' % dbname)
    return conn, c


def _close(conn):
    conn.close()


def _exists(repo, dbname):
    """
    checks the existence of the database or creates it
    """
    if not repo.vfs.exists(dbname):
        try:
            conn = sqlite3.connect(repo.vfs.join(dbname))
            c = conn.cursor()
            c.execute('CREATE TABLE Moves(hash CHAR(40), source TEXT, ' +
                'destination TEXT, mv CHAR(1));')
            _close(conn)
        except:
            raise util.Abort('could not create the %s local database' % dbname)


def insertdata(repo, ctx, mvdict, cpdict, remote=False):
    """
    inserts the mvdict/cpdict = {dst: src} data in the database with '1' if it
    is a move, '0' if it is a copy
    """
    if remote:
        dbname = remotedb
    else:
        dbname = localdb
    if mvdict == {} and cpdict == {}:
        return
    sqlcmd = 'INSERT INTO Moves VALUES(?, ?, ?, ?);'
    conn, c = _connect(repo, dbname)
    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex())
    try:
        for dst, src in mvdict.iteritems():
            c.execute(sqlcmd, (ctxhash, src, dst, '1'))
        for dst, src in cpdict.iteritems():
            c.execute(sqlcmd, (ctxhash, src, dst, '0'))
        conn.commit()
    except:
        raise util.Abort('could not insert data into the %s database' % dbname)

    _close(conn)


def retrievedata(repo, ctx, move=False, remote=False):
    """
    returns the {dst:src} dictonary for moves if move = True or of copies if
    move = False for ctx
    """
    if remote:
        dbname = remotedb
    else:
        dbname = localdb
    conn, c = _connect(repo, dbname)
    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex())
    if move:
        mv = '1'
    else:
        mv = '0'
    try:
        c.execute('SELECT DISTINCT source, destination FROM Moves ' +
                'WHERE hash = ? AND mv = ?', [ctxhash, mv])
    except:
        raise util.Abort('could not access data from the %s database' % dbname)

    all_rows = c.fetchall()
    _close(conn)
    ret = {}
    for src, dst in all_rows:
        ret[dst.encode('utf8')] = src.encode('utf8')
    return ret


def insertdatapkg(repo, renames, remote=False):
    """
    inserts renames = {ctxhash: [(src, dst, mv)]} into the database
    """
    if remote:
        dbname = remotedb
    else:
        dbname = localdb
    conn, c = _connect(repo, dbname)
    sqlcmd = 'INSERT INTO Moves VALUES(?, ?, ?, ?);'
    try:
        for ctxhash, mvlist in renames.iteritems():
            for src, dst, mv in mvlist:
                c.execute(sqlcmd, (ctxhash, src, dst, mv))
        conn.commit()
    except:
        raise util.Abort('could not insert data into the %s database' % dbname)

    _close(conn)


def retrievedatapkg(repo, ctxlist, remote=False):
    """
    retrieves {ctxhash: [(src, dst, mv)]} for ctxhash in ctxlist
    """
    if remote:
        dbname = remotedb
    else:
        dbname = localdb
    conn, c = _connect(repo, dbname)
    try:
        c.execute('SELECT DISTINCT * FROM Moves WHERE hash IN' +
                ' (%s)' % (','.join('?'*len(ctxlist))), ctxlist)
    except:
        raise util.Abort('could not access data from the %s database' % dbname)

    all_rows = c.fetchall()
    _close(conn)
    ret = {}
    for ctxhash, src, dst, mv in all_rows:
        ret.setdefault(ctxhash.encode('utf8'), []).append((src.encode('utf8'),
             dst.encode('utf8'), mv.encode('utf8')))

    return ret



def removectx(repo, ctx, remote=False):
    """
    removes the data concerning the ctx in the database
    """
    if remote:
        dbname = remotedb
    else:
        dbname = localdb
    conn, c = _connect(repo, dbname)
    # '0'is used as temp data storage
    if ctx == '0':
        ctxhash = '0'
    else:
        ctxhash = str(ctx.hex()) + '%'
    try:
        c.execute('DELETE FROM Moves WHERE hash LIKE ?', [ctxhash])
        conn.commit()
    except:
        raise util.Abort('could not delete ctx from the %s database' % dbname)

    _close(conn)
