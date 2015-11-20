# filldb.py
#
# Wrapping the commit, amend and rebase commands to add the copytracing data
# into the renames database
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import scmutil
from mercurial import copies as copiesmod
import sqlite3

import dbutil


def _sortmvcp(repo, ctx, cp, moves, copies):
    """
    sorts the cp renames between moves and copies
    """
    for dst, src in cp.iteritems():
        # check if it is a move or a copy
        m = scmutil.match(ctx, {}, {})
        if not src in repo[ctx].manifest():
            moves[dst] = src
        else:
            copies[dst] = src


def _adddata(repo):
    """
    adds the data for the last commit node in the database
    """
    ctx = repo['.']
    pctx = ctx.p1()
    cp = copiesmod._forwardcopies(pctx, ctx, None)

    moves = {}
    copies = {}

    _sortmvcp(repo, ctx, cp, moves, copies)

    dbutil.insertdata(repo, ctx, moves, copies)


def commit(orig, ui, repo, commitfunc, pats, opts):
    """
    wraps cmdutil.commit to add the renames data in the db
    """
    ret = orig(ui, repo, commitfunc, pats, opts)
    _adddata(repo)
    return ret


def amend(orig, ui, repo, commitfunc, old, extra, pats, opts):
    """
    wraps cmdutil.amend to add the renames data in the db
    """
    ret = orig(ui, repo, commitfunc, old, extra, pats, opts)
    _adddata(repo)
    return ret


def _markchanges(repo, renames):
    """
    Marks the files in renames as copied
    """
    wctx = repo[None]
    wlock = repo.wlock()
    try:
        for dst, src in renames.iteritems():
            wctx.copy(src, dst)
    finally:
        wlock.release()


def concludenode(orig, repo, rev, p1, p2, **kwargs):
    """
    wraps the committing function from rebase, retrieves the stored temp data
    during mergecopies and writes it in the dirstate, adds the renames data in
     the db
    """
    # this allows to trace rename information from the rebase which mercurial
    # doesn't do today
    cp = dbutil.retrievedatapkg(repo, ['0'], move=False, askserver=False)['0']
    dbutil.removectx(repo, '0')
    _markchanges(repo, cp)
    ret = orig(repo, rev, p1, p2, **kwargs)

    _adddata(repo)
    return ret
