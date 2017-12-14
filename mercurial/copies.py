# copies.py - copy detection for Mercurial
#
# Copyright 2008 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import heapq
import os

from .i18n import _

from . import (
    match as matchmod,
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
    """chain two sets of copies a->b"""
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
    """return file context that is the ancestor of fctx present in ancestor
    manifest am, stopping after the first ancestor lower than limit"""

    for f in fctx.ancestors():
        if am.get(f.path(), None) == f.filenode():
            return f
        if limit >= 0 and f.linkrev() < limit and f.rev() < limit:
            return None

def _dirstatecopies(d, match=None):
    ds = d._repo.dirstate
    c = ds.copies().copy()
    for k in list(c):
        if ds[k] not in 'anm' or (match and not match(k)):
            del c[k]
    return c

def _computeforwardmissing(a, b, match=None):
    """Computes which files are in b but not a.
    This is its own function so extensions can easily wrap this call to see what
    files _forwardcopies is about to process.
    """
    ma = a.manifest()
    mb = b.manifest()
    return mb.filesnotin(ma, match=match)

def _committedforwardcopies(a, b, match):
    """Like _forwardcopies(), but b.rev() cannot be None (working copy)"""
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
    if b.p1() == a and b.p2().node() == node.nullid:
        filesmatcher = scmutil.matchfiles(a._repo, b.files())
        forwardmissingmatch = matchmod.intersectmatchers(match, filesmatcher)
    missing = _computeforwardmissing(a, b, match=forwardmissingmatch)

    ancestrycontext = a._repo.changelog.ancestors([b.rev()], inclusive=True)
    for f in missing:
        fctx = b[f]
        fctx._ancestrycontext = ancestrycontext
        ofctx = _tracefile(fctx, am, limit)
        if ofctx:
            cm[f] = ofctx.path()
    return cm

def _forwardcopies(a, b, match=None):
    """find {dst@b: src@a} copy mapping where a is an ancestor of b"""

    # check for working copy
    if b.rev() is None:
        if a == b.p1():
            # short-circuit to avoid issues with merge states
            return _dirstatecopies(b, match)

        cm = _committedforwardcopies(a, b.p1(), match)
        # combine copies from dirstate if necessary
        return _chain(a, b, cm, _dirstatecopies(b, match))
    return _committedforwardcopies(a, b, match)

def _backwardrenames(a, b):
    if a._repo.ui.config('experimental', 'copytrace') == 'off':
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
    """find {dst@y: src@x} copy mapping for directed compare"""
    if x == y or not x or not y:
        return {}
    a = y.ancestor(x)
    if a == x:
        return _forwardcopies(x, y, match=match)
    if a == y:
        return _backwardrenames(x, y)
    return _chain(x, y, _backwardrenames(x, a),
                  _forwardcopies(a, y, match=match))

def _computenonoverlap(repo, c1, c2, addedinm1, addedinm2, baselabel=''):
    """Computes, based on addedinm1 and addedinm2, the files exclusive to c1
    and c2. This is its own function so extensions can easily wrap this call
    to see what files mergecopies is about to process.

    Even though c1 and c2 are not used in this function, they are useful in
    other extensions for being able to read the file nodes of the changed files.

    "baselabel" can be passed to help distinguish the multiple computations
    done in the graft case.
    """
    u1 = sorted(addedinm1 - addedinm2)
    u2 = sorted(addedinm2 - addedinm1)

    header = "  unmatched files in %s"
    if baselabel:
        header += ' (from %s)' % baselabel
    if u1:
        repo.ui.debug("%s:\n   %s\n" % (header % 'local', "\n   ".join(u1)))
    if u2:
        repo.ui.debug("%s:\n   %s\n" % (header % 'other', "\n   ".join(u2)))
    return u1, u2

def _makegetfctx(ctx):
    """return a 'getfctx' function suitable for _checkcopies usage

    We have to re-setup the function building 'filectx' for each
    '_checkcopies' to ensure the linkrev adjustment is properly setup for
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
        if n in node.wdirnodes:  # in a working context?
            if ctx.rev() is None:
                return ctx.filectx(f)
            return repo[None][f]
        fctx = repo.filectx(f, fileid=n)
        # setup only needed for filectx not create from a changectx
        fctx._ancestrycontext = ac
        fctx._descendantrev = rev
        return fctx
    return util.lrucachefunc(makectx)

def _combinecopies(copyfrom, copyto, finalcopy, diverge, incompletediverge):
    """combine partial copy paths"""
    remainder = {}
    for f in copyfrom:
        if f in copyto:
            finalcopy[copyto[f]] = copyfrom[f]
            del copyto[f]
    for f in incompletediverge:
        assert f not in diverge
        ic = incompletediverge[f]
        if ic[0] in copyto:
            diverge[f] = [copyto[ic[0]], ic[1]]
        else:
            remainder[f] = ic
    return remainder

def mergecopies(repo, c1, c2, base):
    """
    The function calling different copytracing algorithms on the basis of config
    which find moves and copies between context c1 and c2 that are relevant for
    merging. 'base' will be used as the merge base.

    Copytracing is used in commands like rebase, merge, unshelve, etc to merge
    files that were moved/ copied in one merge parent and modified in another.
    For example:

    o          ---> 4 another commit
    |
    |   o      ---> 3 commit that modifies a.txt
    |  /
    o /        ---> 2 commit that moves a.txt to b.txt
    |/
    o          ---> 1 merge base

    If we try to rebase revision 3 on revision 4, since there is no a.txt in
    revision 4, and if user have copytrace disabled, we prints the following
    message:

    ```other changed <file> which local deleted```

    Returns five dicts: "copy", "movewithdir", "diverge", "renamedelete" and
    "dirmove".

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

    "dirmove" is a mapping of detected source dir -> destination dir renames.
    This is needed for handling changes to new files previously grafted into
    renamed directories.
    """
    # avoid silly behavior for update from empty dir
    if not c1 or not c2 or c1 == c2:
        return {}, {}, {}, {}, {}

    # avoid silly behavior for parent -> working dir
    if c2.node() is None and c1.node() == repo.dirstate.p1():
        return repo.dirstate.copies(), {}, {}, {}, {}

    copytracing = repo.ui.config('experimental', 'copytrace')

    # Copy trace disabling is explicitly below the node == p1 logic above
    # because the logic above is required for a simple copy to be kept across a
    # rebase.
    if copytracing == 'off':
        return {}, {}, {}, {}, {}
    elif copytracing == 'heuristics':
        # Do full copytracing if only non-public revisions are involved as
        # that will be fast enough and will also cover the copies which could
        # be missed by heuristics
        if _isfullcopytraceable(repo, c1, base):
            return _fullcopytracing(repo, c1, c2, base)
        return _heuristicscopytracing(repo, c1, c2, base)
    else:
        return _fullcopytracing(repo, c1, c2, base)

def _isfullcopytraceable(repo, c1, base):
    """ Checks that if base, source and destination are all no-public branches,
    if yes let's use the full copytrace algorithm for increased capabilities
    since it will be fast enough.

    `experimental.copytrace.sourcecommitlimit` can be used to set a limit for
    number of changesets from c1 to base such that if number of changesets are
    more than the limit, full copytracing algorithm won't be used.
    """
    if c1.rev() is None:
        c1 = c1.p1()
    if c1.mutable() and base.mutable():
        sourcecommitlimit = repo.ui.configint('experimental',
                                              'copytrace.sourcecommitlimit')
        commits = len(repo.revs('%d::%d', base.rev(), c1.rev()))
        return commits < sourcecommitlimit
    return False

def _fullcopytracing(repo, c1, c2, base):
    """ The full copytracing algorithm which finds all the new files that were
    added from merge base up to the top commit and for each file it checks if
    this file was copied from another file.

    This is pretty slow when a lot of changesets are involved but will track all
    the copies.
    """
    # In certain scenarios (e.g. graft, update or rebase), base can be
    # overridden We still need to know a real common ancestor in this case We
    # can't just compute _c1.ancestor(_c2) and compare it to ca, because there
    # can be multiple common ancestors, e.g. in case of bidmerge.  Because our
    # caller may not know if the revision passed in lieu of the CA is a genuine
    # common ancestor or not without explicitly checking it, it's better to
    # determine that here.
    #
    # base.descendant(wc) and base.descendant(base) are False, work around that
    _c1 = c1.p1() if c1.rev() is None else c1
    _c2 = c2.p1() if c2.rev() is None else c2
    # an endpoint is "dirty" if it isn't a descendant of the merge base
    # if we have a dirty endpoint, we need to trigger graft logic, and also
    # keep track of which endpoint is dirty
    dirtyc1 = not (base == _c1 or base.descendant(_c1))
    dirtyc2 = not (base == _c2 or base.descendant(_c2))
    graft = dirtyc1 or dirtyc2
    tca = base
    if graft:
        tca = _c1.ancestor(_c2)

    limit = _findlimit(repo, c1.rev(), c2.rev())
    if limit is None:
        # no common ancestor, no copies
        return {}, {}, {}, {}, {}
    repo.ui.debug("  searching for copies back to rev %d\n" % limit)

    m1 = c1.manifest()
    m2 = c2.manifest()
    mb = base.manifest()

    # gather data from _checkcopies:
    # - diverge = record all diverges in this dict
    # - copy = record all non-divergent copies in this dict
    # - fullcopy = record all copies in this dict
    # - incomplete = record non-divergent partial copies here
    # - incompletediverge = record divergent partial copies here
    diverge = {} # divergence data is shared
    incompletediverge  = {}
    data1 = {'copy': {},
             'fullcopy': {},
             'incomplete': {},
             'diverge': diverge,
             'incompletediverge': incompletediverge,
            }
    data2 = {'copy': {},
             'fullcopy': {},
             'incomplete': {},
             'diverge': diverge,
             'incompletediverge': incompletediverge,
            }

    # find interesting file sets from manifests
    addedinm1 = m1.filesnotin(mb)
    addedinm2 = m2.filesnotin(mb)
    bothnew = sorted(addedinm1 & addedinm2)
    if tca == base:
        # unmatched file from base
        u1r, u2r = _computenonoverlap(repo, c1, c2, addedinm1, addedinm2)
        u1u, u2u = u1r, u2r
    else:
        # unmatched file from base (DAG rotation in the graft case)
        u1r, u2r = _computenonoverlap(repo, c1, c2, addedinm1, addedinm2,
                                      baselabel='base')
        # unmatched file from topological common ancestors (no DAG rotation)
        # need to recompute this for directory move handling when grafting
        mta = tca.manifest()
        u1u, u2u = _computenonoverlap(repo, c1, c2, m1.filesnotin(mta),
                                                    m2.filesnotin(mta),
                                      baselabel='topological common ancestor')

    for f in u1u:
        _checkcopies(c1, c2, f, base, tca, dirtyc1, limit, data1)

    for f in u2u:
        _checkcopies(c2, c1, f, base, tca, dirtyc2, limit, data2)

    copy = dict(data1['copy'])
    copy.update(data2['copy'])
    fullcopy = dict(data1['fullcopy'])
    fullcopy.update(data2['fullcopy'])

    if dirtyc1:
        _combinecopies(data2['incomplete'], data1['incomplete'], copy, diverge,
                       incompletediverge)
    else:
        _combinecopies(data1['incomplete'], data2['incomplete'], copy, diverge,
                       incompletediverge)

    renamedelete = {}
    renamedeleteset = set()
    divergeset = set()
    for of, fl in list(diverge.items()):
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
    bothdiverge = {}
    bothincompletediverge = {}
    remainder = {}
    both1 = {'copy': {},
             'fullcopy': {},
             'incomplete': {},
             'diverge': bothdiverge,
             'incompletediverge': bothincompletediverge
            }
    both2 = {'copy': {},
             'fullcopy': {},
             'incomplete': {},
             'diverge': bothdiverge,
             'incompletediverge': bothincompletediverge
            }
    for f in bothnew:
        _checkcopies(c1, c2, f, base, tca, dirtyc1, limit, both1)
        _checkcopies(c2, c1, f, base, tca, dirtyc2, limit, both2)
    if dirtyc1:
        # incomplete copies may only be found on the "dirty" side for bothnew
        assert not both2['incomplete']
        remainder = _combinecopies({}, both1['incomplete'], copy, bothdiverge,
                                   bothincompletediverge)
    elif dirtyc2:
        assert not both1['incomplete']
        remainder = _combinecopies({}, both2['incomplete'], copy, bothdiverge,
                                   bothincompletediverge)
    else:
        # incomplete copies and divergences can't happen outside grafts
        assert not both1['incomplete']
        assert not both2['incomplete']
        assert not bothincompletediverge
    for f in remainder:
        assert f not in bothdiverge
        ic = remainder[f]
        if ic[0] in (m1 if dirtyc1 else m2):
            # backed-out rename on one side, but watch out for deleted files
            bothdiverge[f] = ic
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
        return copy, {}, diverge, renamedelete, {}

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
        return copy, {}, diverge, renamedelete, {}

    for d in dirmove:
        repo.ui.debug("   discovered dir src: '%s' -> dst: '%s'\n" %
                      (d, dirmove[d]))

    movewithdir = {}
    # check unaccounted nonoverlapping files against directory moves
    for f in u1r + u2r:
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

    return copy, movewithdir, diverge, renamedelete, dirmove

def _heuristicscopytracing(repo, c1, c2, base):
    """ Fast copytracing using filename heuristics

    Assumes that moves or renames are of following two types:

    1) Inside a directory only (same directory name but different filenames)
    2) Move from one directory to another
                    (same filenames but different directory names)

    Works only when there are no merge commits in the "source branch".
    Source branch is commits from base up to c2 not including base.

    If merge is involved it fallbacks to _fullcopytracing().

    Can be used by setting the following config:

        [experimental]
        copytrace = heuristics

    In some cases the copy/move candidates found by heuristics can be very large
    in number and that will make the algorithm slow. The number of possible
    candidates to check can be limited by using the config
    `experimental.copytrace.movecandidateslimit` which defaults to 100.
    """

    if c1.rev() is None:
        c1 = c1.p1()
    if c2.rev() is None:
        c2 = c2.p1()

    copies = {}

    changedfiles = set()
    m1 = c1.manifest()
    if not repo.revs('%d::%d', base.rev(), c2.rev()):
        # If base is not in c2 branch, we switch to fullcopytracing
        repo.ui.debug("switching to full copytracing as base is not "
                      "an ancestor of c2\n")
        return _fullcopytracing(repo, c1, c2, base)

    ctx = c2
    while ctx != base:
        if len(ctx.parents()) == 2:
            # To keep things simple let's not handle merges
            repo.ui.debug("switching to full copytracing because of merges\n")
            return _fullcopytracing(repo, c1, c2, base)
        changedfiles.update(ctx.files())
        ctx = ctx.p1()

    cp = _forwardcopies(base, c2)
    for dst, src in cp.iteritems():
        if src in m1:
            copies[dst] = src

    # file is missing if it isn't present in the destination, but is present in
    # the base and present in the source.
    # Presence in the base is important to exclude added files, presence in the
    # source is important to exclude removed files.
    missingfiles = filter(lambda f: f not in m1 and f in base and f in c2,
                          changedfiles)

    if missingfiles:
        basenametofilename = collections.defaultdict(list)
        dirnametofilename = collections.defaultdict(list)

        for f in m1.filesnotin(base.manifest()):
            basename = os.path.basename(f)
            dirname = os.path.dirname(f)
            basenametofilename[basename].append(f)
            dirnametofilename[dirname].append(f)

        # in case of a rebase/graft, base may not be a common ancestor
        anc = c1.ancestor(c2)

        for f in missingfiles:
            basename = os.path.basename(f)
            dirname = os.path.dirname(f)
            samebasename = basenametofilename[basename]
            samedirname = dirnametofilename[dirname]
            movecandidates = samebasename + samedirname
            # f is guaranteed to be present in c2, that's why
            # c2.filectx(f) won't fail
            f2 = c2.filectx(f)
            # we can have a lot of candidates which can slow down the heuristics
            # config value to limit the number of candidates moves to check
            maxcandidates = repo.ui.configint('experimental',
                                              'copytrace.movecandidateslimit')

            if len(movecandidates) > maxcandidates:
                repo.ui.status(_("skipping copytracing for '%s', more "
                                 "candidates than the limit: %d\n")
                               % (f, len(movecandidates)))
                continue

            for candidate in movecandidates:
                f1 = c1.filectx(candidate)
                if _related(f1, f2, anc.rev()):
                    # if there are a few related copies then we'll merge
                    # changes into all of them. This matches the behaviour
                    # of upstream copytracing
                    copies[candidate] = f

    return copies, {}, {}, {}, {}

def _related(f1, f2, limit):
    """return True if f1 and f2 filectx have a common ancestor

    Walk back to common ancestor to see if the two files originate
    from the same file. Since workingfilectx's rev() is None it messes
    up the integer comparison logic, hence the pre-step check for
    None (f1 and f2 can only be workingfilectx's initially).
    """

    if f1 == f2:
        return f1 # a match

    g1, g2 = f1.ancestors(), f2.ancestors()
    try:
        f1r, f2r = f1.linkrev(), f2.linkrev()

        if f1r is None:
            f1 = next(g1)
        if f2r is None:
            f2 = next(g2)

        while True:
            f1r, f2r = f1.linkrev(), f2.linkrev()
            if f1r > f2r:
                f1 = next(g1)
            elif f2r > f1r:
                f2 = next(g2)
            elif f1 == f2:
                return f1 # a match
            elif f1r == f2r or f1r < limit or f2r < limit:
                return False # copy no longer relevant
    except StopIteration:
        return False

def _checkcopies(srcctx, dstctx, f, base, tca, remotebase, limit, data):
    """
    check possible copies of f from msrc to mdst

    srcctx = starting context for f in msrc
    dstctx = destination context for f in mdst
    f = the filename to check (as in msrc)
    base = the changectx used as a merge base
    tca = topological common ancestor for graft-like scenarios
    remotebase = True if base is outside tca::srcctx, False otherwise
    limit = the rev number to not search beyond
    data = dictionary of dictionary to store copy data. (see mergecopies)

    note: limit is only an optimization, and provides no guarantee that
    irrelevant revisions will not be visited
    there is no easy way to make this algorithm stop in a guaranteed way
    once it "goes behind a certain revision".
    """

    msrc = srcctx.manifest()
    mdst = dstctx.manifest()
    mb = base.manifest()
    mta = tca.manifest()
    # Might be true if this call is about finding backward renames,
    # This happens in the case of grafts because the DAG is then rotated.
    # If the file exists in both the base and the source, we are not looking
    # for a rename on the source side, but on the part of the DAG that is
    # traversed backwards.
    #
    # In the case there is both backward and forward renames (before and after
    # the base) this is more complicated as we must detect a divergence.
    # We use 'backwards = False' in that case.
    backwards = not remotebase and base != tca and f in mb
    getsrcfctx = _makegetfctx(srcctx)
    getdstfctx = _makegetfctx(dstctx)

    if msrc[f] == mb.get(f) and not remotebase:
        # Nothing to merge
        return

    of = None
    seen = {f}
    for oc in getsrcfctx(f, msrc[f]).ancestors():
        ocr = oc.linkrev()
        of = oc.path()
        if of in seen:
            # check limit late - grab last rename before
            if ocr < limit:
                break
            continue
        seen.add(of)

        # remember for dir rename detection
        if backwards:
            data['fullcopy'][of] = f # grafting backwards through renames
        else:
            data['fullcopy'][f] = of
        if of not in mdst:
            continue # no match, keep looking
        if mdst[of] == mb.get(of):
            return # no merge needed, quit early
        c2 = getdstfctx(of, mdst[of])
        # c2 might be a plain new file on added on destination side that is
        # unrelated to the droids we are looking for.
        cr = _related(oc, c2, tca.rev())
        if cr and (of == f or of == c2.path()): # non-divergent
            if backwards:
                data['copy'][of] = f
            elif of in mb:
                data['copy'][f] = of
            elif remotebase: # special case: a <- b <- a -> b "ping-pong" rename
                data['copy'][of] = f
                del data['fullcopy'][f]
                data['fullcopy'][of] = f
            else: # divergence w.r.t. graft CA on one side of topological CA
                for sf in seen:
                    if sf in mb:
                        assert sf not in data['diverge']
                        data['diverge'][sf] = [f, of]
                        break
            return

    if of in mta:
        if backwards or remotebase:
            data['incomplete'][of] = f
        else:
            for sf in seen:
                if sf in mb:
                    if tca == base:
                        data['diverge'].setdefault(sf, []).append(f)
                    else:
                        data['incompletediverge'][sf] = [of, f]
                    return

def duplicatecopies(repo, wctx, rev, fromrev, skiprev=None):
    """reproduce copies from fromrev to rev in the dirstate

    If skiprev is specified, it's a revision that should be used to
    filter copy records. Any copies that occur between fromrev and
    skiprev will not be duplicated, even if they appear in the set of
    copies between fromrev and rev.
    """
    exclude = {}
    if (skiprev is not None and
        repo.ui.config('experimental', 'copytrace') != 'off'):
        # copytrace='off' skips this line, but not the entire function because
        # the line below is O(size of the repo) during a rebase, while the rest
        # of the function is much faster (and is required for carrying copy
        # metadata across the rebase anyway).
        exclude = pathcopies(repo[fromrev], repo[skiprev])
    for dst, src in pathcopies(repo[fromrev], repo[rev]).iteritems():
        # copies.pathcopies returns backward renames, so dst might not
        # actually be in the dirstate
        if dst in exclude:
            continue
        wctx[dst].markcopied(src)
