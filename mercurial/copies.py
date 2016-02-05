# copies.py - copy detection for Mercurial
#
# Copyright 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import heapq

from . import (
    node,
    pathutil,
    scmutil,
    util,
)

def _findlimit(repo, a, b):
    """
    Find the last revision that needs to be checked to ensure that a full
    transitive closure for file copies can be properly calculated.
    Generally, this means finding the earliest revision number that's an
    ancestor of a or b but not both, except when a or b is a direct descendent
    of the other, in which case we can return the minimum revnum of a and b.
    None if no such revision exists.
    """

    # basic idea:
    # - mark a and b with different sides
    # - if a parent's children are all on the same side, the parent is
    #   on that side, otherwise it is on no side
    # - walk the graph in topological order with the help of a heap;
    #   - add unseen parents to side map
    #   - clear side of any parent that has children on different sides
    #   - track number of interesting revs that might still be on a side
    #   - track the lowest interesting rev seen
    #   - quit when interesting revs is zero

    cl = repo.changelog
    working = len(cl) # pseudo rev for the working directory
    if a is None:
        a = working
    if b is None:
        b = working

    side = {a: -1, b: 1}
    visit = [-a, -b]
    heapq.heapify(visit)
    interesting = len(visit)
    hascommonancestor = False
    limit = working

    while interesting:
        r = -heapq.heappop(visit)
        if r == working:
            parents = [cl.rev(p) for p in repo.dirstate.parents()]
        else:
            parents = cl.parentrevs(r)
        for p in parents:
            if p < 0:
                continue
            if p not in side:
                # first time we see p; add it to visit
                side[p] = side[r]
                if side[p]:
                    interesting += 1
                heapq.heappush(visit, -p)
            elif side[p] and side[p] != side[r]:
                # p was interesting but now we know better
                side[p] = 0
                interesting -= 1
                hascommonancestor = True
        if side[r]:
            limit = r # lowest rev visited
            interesting -= 1

    if not hascommonancestor:
        return None

    # Consider the following flow (see test-commit-amend.t under issue4405):
    # 1/ File 'a0' committed
    # 2/ File renamed from 'a0' to 'a1' in a new commit (call it 'a1')
    # 3/ Move back to first commit
    # 4/ Create a new commit via revert to contents of 'a1' (call it 'a1-amend')
    # 5/ Rename file from 'a1' to 'a2' and commit --amend 'a1-msg'
    #
    # During the amend in step five, we will be in this state:
    #
    # @  3 temporary amend commit for a1-amend
    # |
    # o  2 a1-amend
    # |
    # | o  1 a1
    # |/
    # o  0 a0
    #
    # When _findlimit is called, a and b are revs 3 and 0, so limit will be 2,
    # yet the filelog has the copy information in rev 1 and we will not look
    # back far enough unless we also look at the a and b as candidates.
    # This only occurs when a is a descendent of b or visa-versa.
    return min(limit, a, b)

def _chain(src, dst, a, b):
    '''chain two sets of copies a->b'''
    t = a.copy()
    for k, v in b.iteritems():
        if v in t:
            # found a chain
            if t[v] != k:
                # file wasn't renamed back to itself
                t[k] = t[v]
            if v not in dst:
                # chain was a rename, not a copy
                del t[v]
        if v in src:
            # file is a copy of an existing file
            t[k] = v

    # remove criss-crossed copies
    for k, v in t.items():
        if k in src and v in dst:
            del t[k]

    return t

def _tracefile(fctx, am, limit=-1):
    '''return file context that is the ancestor of fctx present in ancestor
    manifest am, stopping after the first ancestor lower than limit'''

    for f in fctx.ancestors():
        if am.get(f.path(), None) == f.filenode():
            return f
        if limit >= 0 and f.linkrev() < limit and f.rev() < limit:
            return None

def _dirstatecopies(d):
    ds = d._repo.dirstate
    c = ds.copies().copy()
    for k in c.keys():
        if ds[k] not in 'anm':
            del c[k]
    return c

def _computeforwardmissing(a, b, match=None):
    """Computes which files are in b but not a.
    This is its own function so extensions can easily wrap this call to see what
    files _forwardcopies is about to process.
    """
    ma = a.manifest()
    mb = b.manifest()
    if match:
        ma = ma.matches(match)
        mb = mb.matches(match)
    return mb.filesnotin(ma)

def _forwardcopies(a, b, match=None):
    '''find {dst@b: src@a} copy mapping where a is an ancestor of b'''

    # check for working copy
    w = None
    if b.rev() is None:
        w = b
        b = w.p1()
        if a == b:
            # short-circuit to avoid issues with merge states
            return _dirstatecopies(w)

    # files might have to be traced back to the fctx parent of the last
    # one-side-only changeset, but not further back than that
    limit = _findlimit(a._repo, a.rev(), b.rev())
    if limit is None:
        limit = -1
    am = a.manifest()

    # find where new files came from
    # we currently don't try to find where old files went, too expensive
    # this means we can miss a case like 'hg rm b; hg cp a b'
    cm = {}

    # Computing the forward missing is quite expensive on large manifests, since
    # it compares the entire manifests. We can optimize it in the common use
    # case of computing what copies are in a commit versus its parent (like
    # during a rebase or histedit). Note, we exclude merge commits from this
    # optimization, since the ctx.files() for a merge commit is not correct for
    # this comparison.
    forwardmissingmatch = match
    if not match and b.p1() == a and b.p2().node() == node.nullid:
        forwardmissingmatch = scmutil.matchfiles(a._repo, b.files())
    missing = _computeforwardmissing(a, b, match=forwardmissingmatch)

    ancestrycontext = a._repo.changelog.ancestors([b.rev()], inclusive=True)
    for f in missing:
        fctx = b[f]
        fctx._ancestrycontext = ancestrycontext
        ofctx = _tracefile(fctx, am, limit)
        if ofctx:
            cm[f] = ofctx.path()

    # combine copies from dirstate if necessary
    if w is not None:
        cm = _chain(a, w, cm, _dirstatecopies(w))

    return cm

def _backwardrenames(a, b):
    if a._repo.ui.configbool('experimental', 'disablecopytrace'):
        return {}

    # Even though we're not taking copies into account, 1:n rename situations
    # can still exist (e.g. hg cp a b; hg mv a c). In those cases we
    # arbitrarily pick one of the renames.
    f = _forwardcopies(b, a)
    r = {}
    for k, v in sorted(f.iteritems()):
        # remove copies
        if v in a:
            continue
        r[v] = k
    return r

def pathcopies(x, y, match=None):
    '''find {dst@y: src@x} copy mapping for directed compare'''
    if x == y or not x or not y:
        return {}
    a = y.ancestor(x)
    if a == x:
        return _forwardcopies(x, y, match=match)
    if a == y:
        return _backwardrenames(x, y)
    return _chain(x, y, _backwardrenames(x, a),
                  _forwardcopies(a, y, match=match))

def _computenonoverlap(repo, c1, c2, addedinm1, addedinm2):
    """Computes, based on addedinm1 and addedinm2, the files exclusive to c1
    and c2. This is its own function so extensions can easily wrap this call
    to see what files mergecopies is about to process.

    Even though c1 and c2 are not used in this function, they are useful in
    other extensions for being able to read the file nodes of the changed files.
    """
    u1 = sorted(addedinm1 - addedinm2)
    u2 = sorted(addedinm2 - addedinm1)

    if u1:
        repo.ui.debug("  unmatched files in local:\n   %s\n"
                      % "\n   ".join(u1))
    if u2:
        repo.ui.debug("  unmatched files in other:\n   %s\n"
                      % "\n   ".join(u2))
    return u1, u2

def _makegetfctx(ctx):
    """return a 'getfctx' function suitable for checkcopies usage

    We have to re-setup the function building 'filectx' for each
    'checkcopies' to ensure the linkrev adjustment is properly setup for
    each. Linkrev adjustment is important to avoid bug in rename
    detection. Moreover, having a proper '_ancestrycontext' setup ensures
    the performance impact of this adjustment is kept limited. Without it,
    each file could do a full dag traversal making the time complexity of
    the operation explode (see issue4537).

    This function exists here mostly to limit the impact on stable. Feel
    free to refactor on default.
    """
    rev = ctx.rev()
    repo = ctx._repo
    ac = getattr(ctx, '_ancestrycontext', None)
    if ac is None:
        revs = [rev]
        if rev is None:
            revs = [p.rev() for p in ctx.parents()]
        ac = repo.changelog.ancestors(revs, inclusive=True)
        ctx._ancestrycontext = ac
    def makectx(f, n):
        if len(n) != 20:  # in a working context?
            if ctx.rev() is None:
                return ctx.filectx(f)
            return repo[None][f]
        fctx = repo.filectx(f, fileid=n)
        # setup only needed for filectx not create from a changectx
        fctx._ancestrycontext = ac
        fctx._descendantrev = rev
        return fctx
    return util.lrucachefunc(makectx)

def mergecopies(repo, c1, c2, ca):
    """
    Find moves and copies between context c1 and c2 that are relevant
    for merging.

    Returns four dicts: "copy", "movewithdir", "diverge", and
    "renamedelete".

    "copy" is a mapping from destination name -> source name,
    where source is in c1 and destination is in c2 or vice-versa.

    "movewithdir" is a mapping from source name -> destination name,
    where the file at source present in one context but not the other
    needs to be moved to destination by the merge process, because the
    other context moved the directory it is in.

    "diverge" is a mapping of source name -> list of destination names
    for divergent renames.

    "renamedelete" is a mapping of source name -> list of destination
    names for files deleted in c1 that were renamed in c2 or vice-versa.
    """
    # avoid silly behavior for update from empty dir
    if not c1 or not c2 or c1 == c2:
        return {}, {}, {}, {}

    # avoid silly behavior for parent -> working dir
    if c2.node() is None and c1.node() == repo.dirstate.p1():
        return repo.dirstate.copies(), {}, {}, {}

    # Copy trace disabling is explicitly below the node == p1 logic above
    # because the logic above is required for a simple copy to be kept across a
    # rebase.
    if repo.ui.configbool('experimental', 'disablecopytrace'):
        return {}, {}, {}, {}

    limit = _findlimit(repo, c1.rev(), c2.rev())
    if limit is None:
        # no common ancestor, no copies
        return {}, {}, {}, {}
    repo.ui.debug("  searching for copies back to rev %d\n" % limit)

    m1 = c1.manifest()
    m2 = c2.manifest()
    ma = ca.manifest()

    copy1, copy2, = {}, {}
    movewithdir1, movewithdir2 = {}, {}
    fullcopy1, fullcopy2 = {}, {}
    diverge = {}

    # find interesting file sets from manifests
    addedinm1 = m1.filesnotin(ma)
    addedinm2 = m2.filesnotin(ma)
    u1, u2 = _computenonoverlap(repo, c1, c2, addedinm1, addedinm2)
    bothnew = sorted(addedinm1 & addedinm2)

    for f in u1:
        checkcopies(c1, f, m1, m2, ca, limit, diverge, copy1, fullcopy1)

    for f in u2:
        checkcopies(c2, f, m2, m1, ca, limit, diverge, copy2, fullcopy2)

    copy = dict(copy1.items() + copy2.items())
    movewithdir = dict(movewithdir1.items() + movewithdir2.items())
    fullcopy = dict(fullcopy1.items() + fullcopy2.items())

    renamedelete = {}
    renamedeleteset = set()
    divergeset = set()
    for of, fl in diverge.items():
        if len(fl) == 1 or of in c1 or of in c2:
            del diverge[of] # not actually divergent, or not a rename
            if of not in c1 and of not in c2:
                # renamed on one side, deleted on the other side, but filter
                # out files that have been renamed and then deleted
                renamedelete[of] = [f for f in fl if f in c1 or f in c2]
                renamedeleteset.update(fl) # reverse map for below
        else:
            divergeset.update(fl) # reverse map for below

    if bothnew:
        repo.ui.debug("  unmatched files new in both:\n   %s\n"
                      % "\n   ".join(bothnew))
    bothdiverge, _copy, _fullcopy = {}, {}, {}
    for f in bothnew:
        checkcopies(c1, f, m1, m2, ca, limit, bothdiverge, _copy, _fullcopy)
        checkcopies(c2, f, m2, m1, ca, limit, bothdiverge, _copy, _fullcopy)
    for of, fl in bothdiverge.items():
        if len(fl) == 2 and fl[0] == fl[1]:
            copy[fl[0]] = of # not actually divergent, just matching renames

    if fullcopy and repo.ui.debugflag:
        repo.ui.debug("  all copies found (* = to merge, ! = divergent, "
                      "% = renamed and deleted):\n")
        for f in sorted(fullcopy):
            note = ""
            if f in copy:
                note += "*"
            if f in divergeset:
                note += "!"
            if f in renamedeleteset:
                note += "%"
            repo.ui.debug("   src: '%s' -> dst: '%s' %s\n" % (fullcopy[f], f,
                                                              note))
    del divergeset

    if not fullcopy:
        return copy, movewithdir, diverge, renamedelete

    repo.ui.debug("  checking for directory renames\n")

    # generate a directory move map
    d1, d2 = c1.dirs(), c2.dirs()
    # Hack for adding '', which is not otherwise added, to d1 and d2
    d1.addpath('/')
    d2.addpath('/')
    invalid = set()
    dirmove = {}

    # examine each file copy for a potential directory move, which is
    # when all the files in a directory are moved to a new directory
    for dst, src in fullcopy.iteritems():
        dsrc, ddst = pathutil.dirname(src), pathutil.dirname(dst)
        if dsrc in invalid:
            # already seen to be uninteresting
            continue
        elif dsrc in d1 and ddst in d1:
            # directory wasn't entirely moved locally
            invalid.add(dsrc + "/")
        elif dsrc in d2 and ddst in d2:
            # directory wasn't entirely moved remotely
            invalid.add(dsrc + "/")
        elif dsrc + "/" in dirmove and dirmove[dsrc + "/"] != ddst + "/":
            # files from the same directory moved to two different places
            invalid.add(dsrc + "/")
        else:
            # looks good so far
            dirmove[dsrc + "/"] = ddst + "/"

    for i in invalid:
        if i in dirmove:
            del dirmove[i]
    del d1, d2, invalid

    if not dirmove:
        return copy, movewithdir, diverge, renamedelete

    for d in dirmove:
        repo.ui.debug("   discovered dir src: '%s' -> dst: '%s'\n" %
                      (d, dirmove[d]))

    # check unaccounted nonoverlapping files against directory moves
    for f in u1 + u2:
        if f not in fullcopy:
            for d in dirmove:
                if f.startswith(d):
                    # new file added in a directory that was moved, move it
                    df = dirmove[d] + f[len(d):]
                    if df not in copy:
                        movewithdir[f] = df
                        repo.ui.debug(("   pending file src: '%s' -> "
                                       "dst: '%s'\n") % (f, df))
                    break

    return copy, movewithdir, diverge, renamedelete

def checkcopies(ctx, f, m1, m2, ca, limit, diverge, copy, fullcopy):
    """
    check possible copies of f from m1 to m2

    ctx = starting context for f in m1
    f = the filename to check
    m1 = the source manifest
    m2 = the destination manifest
    ca = the changectx of the common ancestor
    limit = the rev number to not search beyond
    diverge = record all diverges in this dict
    copy = record all non-divergent copies in this dict
    fullcopy = record all copies in this dict
    """

    ma = ca.manifest()
    getfctx = _makegetfctx(ctx)

    def _related(f1, f2, limit):
        # Walk back to common ancestor to see if the two files originate
        # from the same file. Since workingfilectx's rev() is None it messes
        # up the integer comparison logic, hence the pre-step check for
        # None (f1 and f2 can only be workingfilectx's initially).

        if f1 == f2:
            return f1 # a match

        g1, g2 = f1.ancestors(), f2.ancestors()
        try:
            f1r, f2r = f1.linkrev(), f2.linkrev()

            if f1r is None:
                f1 = g1.next()
            if f2r is None:
                f2 = g2.next()

            while True:
                f1r, f2r = f1.linkrev(), f2.linkrev()
                if f1r > f2r:
                    f1 = g1.next()
                elif f2r > f1r:
                    f2 = g2.next()
                elif f1 == f2:
                    return f1 # a match
                elif f1r == f2r or f1r < limit or f2r < limit:
                    return False # copy no longer relevant
        except StopIteration:
            return False

    of = None
    seen = set([f])
    for oc in getfctx(f, m1[f]).ancestors():
        ocr = oc.linkrev()
        of = oc.path()
        if of in seen:
            # check limit late - grab last rename before
            if ocr < limit:
                break
            continue
        seen.add(of)

        fullcopy[f] = of # remember for dir rename detection
        if of not in m2:
            continue # no match, keep looking
        if m2[of] == ma.get(of):
            break # no merge needed, quit early
        c2 = getfctx(of, m2[of])
        cr = _related(oc, c2, ca.rev())
        if cr and (of == f or of == c2.path()): # non-divergent
            copy[f] = of
            of = None
            break

    if of in ma:
        diverge.setdefault(of, []).append(f)

def duplicatecopies(repo, rev, fromrev, skiprev=None):
    '''reproduce copies from fromrev to rev in the dirstate

    If skiprev is specified, it's a revision that should be used to
    filter copy records. Any copies that occur between fromrev and
    skiprev will not be duplicated, even if they appear in the set of
    copies between fromrev and rev.
    '''
    exclude = {}
    if (skiprev is not None and
        not repo.ui.configbool('experimental', 'disablecopytrace')):
        # disablecopytrace skips this line, but not the entire function because
        # the line below is O(size of the repo) during a rebase, while the rest
        # of the function is much faster (and is required for carrying copy
        # metadata across the rebase anyway).
        exclude = pathcopies(repo[fromrev], repo[skiprev])
    for dst, src in pathcopies(repo[fromrev], repo[rev]).iteritems():
        # copies.pathcopies returns backward renames, so dst might not
        # actually be in the dirstate
        if dst in exclude:
            continue
        if repo.dirstate[dst] in "nma":
            repo.dirstate.copy(src, dst)
