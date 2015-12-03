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

import dbutil, error


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
    try:
        ctx = repo['.']
        pctx = ctx.p1()
        cp = copiesmod._forwardcopies(pctx, ctx, None)

        moves = {}
        copies = {}

        _sortmvcp(repo, ctx, cp, moves, copies)

        dbutil.insertdata(repo, ctx, moves, copies)
    except Exception as e:
        error.logfailure(repo, e, "_adddata")


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
    flag = 0
    try:
        cp = dbutil.retrievedatapkg(repo, ['0'], move=False, askserver=False)
        if '0' in cp.keys():
            _markchanges(repo, cp['0'])
            dbutil.removectx(repo, '0')
    except Exception as e:
        flag = 1
        error.logfailure(repo, e, "concludenode")
    finally:
        ret = orig(repo, rev, p1, p2, **kwargs)

        # don't try if the former calls failed
        if flag == 0:
            _adddata(repo)
        return ret


def fillmvdb(ui, repo, *pats, **opts):
    """
    moves backwards on the tree to adds copytrace data
    """
    start = opts.get('start').split(',')
    stop = int(opts.get('stop'))
    try:
        ctxlist = [repo[startctx].hex() for startctx in start]
        while ctxlist:
            plist = _fillctx(repo, ctxlist)
            ctxlist = []
            for p in plist:
                if p and p.rev() != stop:
                    ctxlist.append(p.hex())
    except:
        ui.warn(ctxlist)


def _fillctx(repo, ctxlist):
    """
    check the presence of the ctx move data or adds it returning its parents
    """
    try:
        added = dbutil.checkpresence(repo, ctxlist, askserver=False)
        # The ctx was already processed, we don't check its parents
        if not added:
            return []
        else:
            repo.ui.warn("Loop:\n")
            parents = []
            for ctxhash in added:
                ctx = repo[ctxhash]
                repo.ui.warn("%s added\n" % ctxhash)
                parents.append(ctx.p1())
                parents.append(ctx.p2())
            return parents
    except:
        repo.ui.warn("%s failed\n" % curctx.hex())
