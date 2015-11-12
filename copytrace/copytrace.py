# copytrace.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import copies, scmutil, util
import sqlite3

import dbutil


def _createctxstack(repo, c, ca):
    """
    c is the more recent context, ca the ancestor to reach
    returns the ctx stack between them
    """

    ctxstack = []
    curctx = c
    while curctx != ca:
        ctxstack.append(curctx)
        # merge case
        if curctx.p2():
            raise util.Abort('cannot handle merge')
        curctx = curctx.p1()
        if not curctx:
            raise util.Abort('could not find the ancestor')

    return ctxstack


def _forwardrenamesandpaths(repo, ctxstack, m):
    """
    m the most recent manifest
    returns {dst@c2, [src@ca, ...]} the full path of renames from src to dst
            {src@anc, [dst]}
    e.g.

        bbb
         :
       aa bb
         :
        a b

    should returns {bbb: [b, bb]}, {b, [bbb]}
    """

    paths = {}

    while ctxstack:
        ctx = ctxstack.pop()
        data = dbutil.retrievedata(repo, ctx, move=True)
        pk = paths.keys()
        delsrc = []
        for dst, src in data.iteritems():
            # This file was renamed before
            if src in pk:
                opath = paths[src][:]
                opath.append(src)
                paths[dst] = opath
                if not src in delsrc:
                    delsrc.append(src)
            else:
                paths[dst] = [src]

        # we only consider moves so the src disappered
        for src in delsrc:
            del paths[src]

    renames = {}
    deleted = []
    for dst, path in paths.iteritems():
        # The file was renamed and then deleted
        if not dst in m:
            deleted.append(dst)
        else:
            renames.setdefault(path[0], []).append(dst)
    for dele in deleted:
        del paths[dele]

    return paths, renames


def _checkfile(f, pathf, renames2, m2, copy, renamedelete):
    """
    f the file to check
    pathf its path from the ancestor to c1
    renames2 the {src, [dst]} moves between ancestor and c2
    m2 the manifest in c2
    copy and renamedelete the structures to complete
    > check what happened to the file f from c1 in the c2 branch
    > returns the 'used' files in renames2 so that they are not considered as
    divergent
    """
    used = []
    of = pathf[0]
    # the original file was renamed in the other branch
    if of in renames2.keys():
        intersect = [val for val in pathf if val in renames2[of]]

        # Case:
        #     d -> e
        #       :   ------>
        #     b -> d      b -> d
        #       :           :
        #       b       ....
        #   f is 'e'       copy{e:d} has to be added

        if intersect:
            src = intersect.pop()
            copy[f] = src
            used.append(src)
            used.append(f)
    # the original file was not renamed but doesn't appear in c2 : deleted
    elif of not in m2:
        renamedelete.setdefault(of, []).append(f)
        used.append(f)
    # the original file is still in c2
    else:
        copy[f] = of
        used.append(f)

    return used


def _branch(repo, c1, anc):
    """
    returns {dst@c2, [src@ca, ...]} the full path of renames from src to dst
            {src@anc, [dst]}
    """
    ctxstack = _createctxstack(repo, c1, anc)
    return _forwardrenamesandpaths(repo, ctxstack, c1.manifest())


def mergecopieswithdb(orig, repo, c1, c2, ca):
    """
    c2 on the draft branch which is getting rebased
    c1 where it is getting rebased to
    ca ancestor to consider to evaluate copies
    returns:
        copy         file renamed in one, modified in the other {dst: src@ca}
        diverge      file renamed in both                       {src@ca: [dst]}
        renamedelete file renamed in one, deleted in the other  {src@ca: [dst]}
    """
    if not c1 or not c2 or c1 == c2:
        return {}, {}, {}, {}

    if c2.node() is None and c1.node() == repo.dirstate.p1():
        return repo.dirstate.copies(), {}, {}, {}

    if c1.rev() == None:
        c1 = c1.p1()
    if c2.rev() == None:
        c2 = c2.p1()

    # in case of a rebase, ca isn't always a common ancestor
    anc = c1.ancestor(c2)
    paths1, renames1 = _branch(repo, c1, anc)
    paths2, renames2 = _branch(repo, c2, anc)

    copy = {}
    renamedelete = {}
    diverge = {}

    m1 = c1.manifest()
    m2 = c2.manifest()
    ma = ca.manifest()

    addedinm1 = m1.filesnotin(ma)
    addedinm2 = m2.filesnotin(ma)
    u1 = sorted(addedinm1 - addedinm2)
    u2 = sorted(addedinm2 - addedinm1)

    used = []
    for f in u1:
        if not f in paths1:
            continue
        used1 = _checkfile(f, paths1[f], renames2, m2, copy, renamedelete)
        used.extend(used1)
    for f in u2:
        if not f in paths2:
            continue
        used.extend(_checkfile(f, paths2[f], renames1, m1, copy, renamedelete))

    for src, dstl in renames1.iteritems():
        for dst in dstl:
            if not dst in used:
                diverge.setdefault(src, []).append(dst)
    for src, dstl in renames2.iteritems():
        for dst in dstl:
            if not dst in used:
                diverge.setdefault(src, []).append(dst)

    # puts the copy data into a temporary row of the db to be able to retrieve
    # it at the commit time of the rebase (concludenode)
    dbutil.removectx(repo, '0')
    dbutil.insertdata(repo, '0', {}, copy)
    return copy, {}, diverge, renamedelete


def _chain(src, dst, a, b):
    """
    chain two sets of copies a->b, renames are okay if move = True
    """
    t = a.copy()
    for bdst, bsrc in b.iteritems():
        if bsrc in t:
            # found a chain
            if t[bsrc] != bdst:
                # file wasn't renamed back to itself
                t[bdst] = t[bsrc]
        if bsrc in src:
            # file is a copy of an existing file
            t[bdst] = bsrc

    # remove criss-crossed copies
    for k, v in t.items():
        if k in src and v in dst:
            del t[k]
    return t


def _dirstatecopies(ctx):
    dirstate = ctx._repo.dirstate
    copies = dirstate.copies().copy()
    for dst in copies.keys():
        if dirstate[dst] not in 'anm':
            del copies[dst]
    return copies


def _dirstaterenames(ctx):
    dirstate = ctx._repo.dirstate
    copies = dirstate.copies().copy()
    for dst in copies.keys():
        if dirstate[dst] not in 'anm' or dirstate[copies[dst]] not in 'r':
            del copies[dst]
    return copies


def _processrenames(repo, ctx, renamed, move=False):
    """
    Adds the renames {dst: src} to the 'renamed' dictionary if the source is
     in files
    """
    data = dbutil.retrievedata(repo, ctx, move=True)
    movedsrc = []
    # moves and copies
    if not move:
        data.update(dbutil.retrievedata(repo, ctx, move=False))

    for dst, src in data.iteritems():
        # checks if the source file is to be considered
        if src in renamed.keys():
            renamed[dst] = renamed[src]
            movedsrc.append(src)
        else:
            renamed[dst] = src

    m = ctx.manifest()
    for src in movedsrc:
        # the file was only moved and not copied
        if not src in m:
            del renamed[src]


def _forwardrenameswithdb(a, b, match=None, move=False):
    """
    find {dst@b: src@a} renames mapping where a is an ancestor of b
    if move = True, copies are not considered
    """
    if move:
        dirstatefunc = _dirstaterenames
    else:
        dirstatefunc = _dirstatecopies
    # check for working copy
    w = None
    if b.rev() is None:
        w = b
        b = w.p1()
        if a == b:
        # short-circuit to avoid issues with merge states
            return dirstatefunc(w)

    ctxstack = _createctxstack(b._repo, b, a)
    renamed = {}

    while ctxstack:
        ctx = ctxstack.pop()
        _processrenames(b._repo, ctx, renamed, move)

    # combine renames from dirstate if necessary
    if w is not None:
        renamed = _chain(a, w, renamed, dirstatefunc(w))

    return renamed


def _backwardrenameswithdb(a, b):
    """
    find {src@b: dst@a} moves mapping where b is an ancestor of a
    """
    # Even though we're not taking copies into account, 1:n rename situations
    # can still exist (e.g. hg cp a b; hg mv a c). In those cases we
    # arbitrarily pick one of the renames.
    # Maybe in the future we can take the most similar one (automv.py)
    forward = _forwardrenameswithdb(b, a, move=True)
    backward = {}
    for dst, src in forward.iteritems():
        # copy not rename
        if dst in a:
            continue
        backward[src] = dst
    return backward


def pathcopieswithdb(orig, x, y, match=None):
    """
    find {dst@y: src@x} copy mapping for directed compare
    """
    if x == y or not x or not y:
        return {}
    a = y.ancestor(x)
    if a == x:
        return _forwardrenameswithdb(x, y, match=match)
    if a == y:
        return _backwardrenameswithdb(x, y)
    return _chain(x, y, _backwardrenameswithdb(x, a),
                   _forwardrenameswithdb(a, y, match=match))


def buildstate(orig, repo, dest, rebaseset, collapsef, obsoletenotrebased):
    """
    wraps the command to get the set of revs that will be involved in the
    rebase and checks if they are in the database
    """
    if rebaseset:
        rev = rebaseset.first()
        rebased = repo[rev]
        ca = rebased.ancestor(dest)
        ctxlist = list(repo.set("only(%r, %r)" % (dest.rev(), ca.rev())))
        if ctxlist:
            dbutil.checkpresence(repo, ctxlist)
    return orig(repo, dest, rebaseset, collapsef, obsoletenotrebased)
