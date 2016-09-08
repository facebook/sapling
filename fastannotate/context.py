# Copyright 2016-present Facebook. All Rights Reserved.
#
# context: context needed to annotate a file
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import os

from fastannotate import (
    revmap as revmapmod,
    error as faerror,
)

from mercurial import (
    lock as lockmod,
    mdiff,
    node,
    scmutil,
    util,
)
from mercurial.i18n import _

import linelog as linelogmod

# extracted from mercurial.context.basefilectx.annotate
def _getbase(fctx):
    introrev = fctx.introrev()
    if fctx.rev() == introrev:
        return fctx
    else:
        return fctx.filectx(fctx.filenode(), changeid=introrev)

# extracted from mercurial.context.basefilectx.annotate
@util.lrucachefunc
def _getlog(f, x):
    return f._repo.file(x)

# extracted from mercurial.context.basefilectx.annotate
def _parents(f, follow=True):
    # Cut _descendantrev here to mitigate the penalty of lazy linkrev
    # adjustment. Otherwise, p._adjustlinkrev() would walk changelog
    # from the topmost introrev (= srcrev) down to p.linkrev() if it
    # isn't an ancestor of the srcrev.
    f._changeid
    pl = f.parents()

    # Don't return renamed parents if we aren't following.
    if not follow:
        pl = [p for p in pl if p.path() == f.path()]

    # renamed filectx won't have a filelog yet, so set it
    # from the cache to save time
    for p in pl:
        if not '_filelog' in p.__dict__:
            p._filelog = _getlog(f, p.path())

    return pl

# extracted from mercurial.context.basefilectx.annotate. slightly modified
# so it takes a fctx instead of a pair of text and fctx.
def _decorate(fctx):
    text = fctx.data()
    linecount = text.count('\n')
    if not text.endswith('\n'):
        linecount += 1
    return ([(fctx, i) for i in xrange(linecount)], text)

# extracted from mercurial.context.basefilectx.annotate. slightly modified
# so it takes an extra "blocks" parameter calculated elsewhere, instead of
# calculating diff here.
def _pair(parent, child, blocks):
    for (a1, a2, b1, b2), t in blocks:
        # Changed blocks ('!') or blocks made only of blank lines ('~')
        # belong to the child.
        if t == '=':
            child[0][b1:b2] = parent[0][a1:a2]
    return child

class annotateopts(object):
    """like mercurial.mdiff.diffopts, but is for annotate

    followrename: follow renames, like "hg annotate -f"
    followmerge: follow p2 of a merge changeset, otherwise p2 is ignored
    """

    defaults = {
        'followrename': True,
        'followmerge': True,
    }

    def __init__(self, **opts):
        for k, v in self.defaults.iteritems():
            setattr(self, k, opts.get(k, v))

    @property
    def shortstr(self):
        """represent opts in a short string, suitable for a directory name"""
        result = ''
        if not self.followrename:
            result += 'r0'
        if not self.followmerge:
            result += 'm0'
        return result or 'default'

defaultopts = annotateopts()

class _annotatecontext(object):
    """do not use this class directly as it does not use lock to protect
    writes. use "with annotatecontext(...)" instead.
    """

    def __init__(self, repo, path, linelog, revmap, opts):
        self.repo = repo
        self.ui = repo.ui
        self.path = path
        self.linelog = linelog
        self.revmap = revmap
        self.opts = opts

    def annotate(self, rev, master=None, showpath=False, showlines=False):
        """incrementally update the cache so it includes revisions in the main
        branch till 'master'. and run annotate on 'rev', which may or may not be
        included in the main branch.

        if master is None, do not update linelog. if master is a callable, call
        it to get the actual master, which can save some time if we don't need
        to resolve the master.

        the first value returned is the annotate result, it is [(node, linenum)]
        by default. [(node, linenum, path)] if showpath is True.

        if showlines is True, a second value will be returned, it is a list of
        corresponding line contents.
        """

        # fast path: if rev is in the main branch already
        directly, revfctx = self.canannotatedirectly(rev)
        if directly:
            if self.ui.debugflag:
                self.ui.debug('fastannotate: %s: no need to update linelog\n'
                              % self.path)
            return self.annotatedirectly(revfctx, showpath, showlines)

        # resolve master
        masterfctx = None
        if master:
            if callable(master):
                master = master()
            masterfctx = _getbase(scmutil.revsingle(self.repo,
                                                    master)[self.path])
            if masterfctx in self.revmap:
                masterfctx = None

        #                  ... - @ <- rev (can be an arbitrary changeset,
        #                 /                not necessarily a descendant
        #      master -> o                 of master)
        #                |
        #     a merge -> o         'o': new changesets in the main branch
        #                |\        '#': revisions in the main branch that
        #                o *            exist in linelog / revmap
        #                | .       '*': changesets in side branches, or
        # last master -> # .            descendants of master
        #                | .
        #                # *       joint: '#', and is a parent of a '*'
        #                |/
        #     a joint -> # ^^^^ --- side branches
        #                |
        #                ^ --- main branch (in linelog)

        # these DFSes are similar to the traditional annotate algorithm.
        # we cannot really reuse the code for perf reason.

        # 1st DFS calculates merges, joint points, and needed.
        # "needed" is a simple reference counting dict to free items in
        # "hist", reducing its memory usage otherwise could be huge.
        initvisit = [revfctx]
        if masterfctx:
            initvisit.append(masterfctx)
        visit = initvisit[:]
        pcache = {}
        needed = {revfctx: 1}
        hist = {} # {fctx: ([(llrev or fctx, linenum)], text)}
        while visit:
            f = visit.pop()
            if f in pcache or f in hist:
                continue
            if f in self.revmap: # in the old main branch, it's a joint
                llrev = self.revmap.hsh2rev(f.node())
                self.linelog.annotate(llrev)
                result = self.linelog.annotateresult
                hist[f] = (result, f.data())
                continue
            pl = self._parentfunc(f)
            pcache[f] = pl
            for p in pl:
                needed[p] = needed.get(p, 0) + 1
                if p not in pcache:
                    visit.append(p)

        # 2nd (simple) DFS calculates new changesets in the main branch
        # ('o' nodes in # the above graph), so we know when to update linelog.
        newmainbranch = set()
        f = masterfctx
        while f and f not in self.revmap:
            newmainbranch.add(f)
            pl = pcache[f]
            if pl:
                f = pl[0]
            else:
                f = None
                break

        # f, if present, is the position where the last build stopped at, and
        # should be the "master" last time. check to see if we can continue
        # building the linelog incrementally. (we cannot if diverged)
        if masterfctx is not None:
            self._checklastmasterhead(f)

        if self.ui.debugflag:
            self.ui.debug('fastannotate: %s: %d new changesets in the main '
                          'branch\n' % (self.path, len(newmainbranch)))

        # prepare annotateresult so we can update linelog incrementally
        self.linelog.annotate(self.linelog.maxrev)

        # 3rd DFS does the actual annotate
        visit = initvisit[:]
        progress = 0
        while visit:
            f = visit[-1]
            if f in hist or f in self.revmap:
                visit.pop()
                continue

            ready = True
            pl = pcache[f]
            for p in pl:
                if p not in hist:
                    ready = False
                    visit.append(p)
            if not ready:
                continue

            visit.pop()
            blocks = None # mdiff blocks, used for appending linelog
            ismainbranch = (f in newmainbranch)
            # curr is the same as the traditional annotate algorithm,
            # if we only care about linear history (do not follow merge),
            # then curr is not actually used.
            assert f not in hist
            curr = _decorate(f)
            for i, p in enumerate(pl):
                bs = list(mdiff.allblocks(hist[p][1], curr[1]))
                if i == 0 and ismainbranch:
                    blocks = bs
                curr = _pair(hist[p], curr, bs)
                if needed[p] == 1:
                    del hist[p]
                    del needed[p]
                else:
                    needed[p] -= 1

            hist[f] = curr
            del pcache[f]

            if ismainbranch: # need to write to linelog
                if not self.ui.quiet:
                    progress += 1
                    self.ui.progress(_('building cache'), progress,
                                     total=len(newmainbranch))
                bannotated = None
                if len(pl) == 2 and self.opts.followmerge: # merge
                    bannotated = curr[0]
                if blocks is None: # no parents, add an empty one
                    blocks = list(mdiff.allblocks('', curr[1]))
                self._appendrev(f, blocks, bannotated)

        if progress: # clean progress bar
            self.ui.write()

        result = [
            ((self.revmap.rev2hsh(f) if isinstance(f, int) else f.node()), l)
            for f, l in hist[revfctx][0]]
        return self._refineannotateresult(result, revfctx, showpath, showlines)

    def canannotatedirectly(self, rev):
        """(str) -> bool, fctx or node.
        return (True, f) if we can annotate without updating the linelog, pass
        f to annotatedirectly.
        return (False, f) if we need extra calculation. f is the fctx resolved
        from rev.
        """
        result = True
        f = None
        if len(rev) == 20 and rev in self.revmap:
            f = rev
        elif len(rev) == 40 and node.bin(rev) in self.revmap:
            f = node.bin(rev)
        else:
            f = _getbase(scmutil.revsingle(self.repo, rev)[self.path])
            result = f in self.revmap
        return result, f

    def annotatedirectly(self, f, showpath, showlines):
        """like annotate, but when we know that f is in linelog.
        f can be either a 20-char str (node) or a fctx. this is for perf - in
        the best case, the user provides a node and we don't need to read the
        filelog or construct any filecontext.
        """
        if isinstance(f, str):
            hsh = f
        else:
            hsh = f.node()
        llrev = self.revmap.hsh2rev(hsh)
        assert llrev
        assert (self.revmap.rev2flag(llrev) & revmapmod.sidebranchflag) == 0
        self.linelog.annotate(llrev)
        result = [(self.revmap.rev2hsh(r), l)
                  for r, l in self.linelog.annotateresult]
        return self._refineannotateresult(result, f, showpath, showlines)

    def _refineannotateresult(self, result, f, showpath, showlines):
        """add the missing path or line contents, they can be expensive.
        f could be either node or fctx.
        """
        if showpath:
            result = self._addpathtoresult(result)
        if showlines:
            if isinstance(f, str): # f: node or fctx
                llrev = self.revmap.hsh2rev(f)
                fctx = self.repo[f][self.revmap.rev2path(llrev)]
            else:
                fctx = f
            lines = mdiff.splitnewlines(fctx.data())
            assert len(lines) == len(result)
            result = (result, lines)
        return result

    def _appendrev(self, fctx, blocks, bannotated=None):
        self._doappendrev(self.linelog, self.revmap, fctx, blocks, bannotated)

    @staticmethod
    def _doappendrev(linelog, revmap, fctx, blocks, bannotated=None):
        """append a revision to linelog and revmap"""

        def getllrev(f):
            """(fctx) -> int"""
            # f should not be a linelog revision
            assert not isinstance(f, int)
            # f is a fctx, allocate linelog rev on demand
            hsh = f.node()
            rev = revmap.hsh2rev(hsh)
            if rev is None:
                rev = revmap.append(hsh, sidebranch=True, path=f.path())
            return rev

        # append sidebranch revisions to revmap
        siderevs = []
        siderevmap = {} # node: int
        if bannotated is not None:
            for (a1, a2, b1, b2), op in blocks:
                if op != '=':
                    # f could be either linelong rev, or fctx.
                    siderevs += [f for f, l in bannotated[b1:b2]
                                 if not isinstance(f, int)]
        siderevs = set(siderevs)
        if fctx in siderevs: # mainnode must be appended seperately
            siderevs.remove(fctx)
        for f in siderevs:
            siderevmap[f] = getllrev(f)

        # the changeset in the main branch, could be a merge
        llrev = revmap.append(fctx.node(), path=fctx.path())
        siderevmap[fctx] = llrev

        for (a1, a2, b1, b2), op in reversed(blocks):
            if op == '=':
                continue
            if bannotated is None:
                linelog.replacelines(llrev, a1, a2, b1, b2)
            else:
                blines = [((r if isinstance(r, int) else siderevmap[r]), l)
                          for r, l in bannotated[b1:b2]]
                linelog.replacelines_vec(llrev, a1, a2, blines)

    def _addpathtoresult(self, annotateresult, revmap=None):
        """(revmap, [(node, linenum)]) -> [(node, linenum, path)]"""
        if revmap is None:
            revmap = self.revmap
        nodes = set([n for n, l in annotateresult])
        paths = dict((n, revmap.rev2path(revmap.hsh2rev(n))) for n in nodes)
        return [(n, l, paths[n]) for n, l in annotateresult]

    def _checklastmasterhead(self, fctx):
        """check if fctx is the master's head last time, raise if not"""
        if fctx is None:
            llrev = 0
        else:
            llrev = self.revmap.hsh2rev(fctx.node())
            assert llrev
        if self.linelog.maxrev != llrev:
            raise faerror.CannotReuseError()

    @util.propertycache
    def _parentfunc(self):
        """-> (fctx) -> [fctx]"""
        followrename = self.opts.followrename
        followmerge = self.opts.followmerge
        def parents(f):
            pl = _parents(f, follow=followrename)
            if not followmerge:
                pl = pl[:1]
            return pl
        return parents

def _unlinkpaths(paths):
    """silent, best-effort unlink"""
    for path in paths:
        try:
            util.unlink(path)
        except OSError:
            pass

@contextlib.contextmanager
def annotatecontext(repo, path, opts=defaultopts, rebuild=False):
    """context needed to perform (fast) annotate on a file

    an annotatecontext of a single file consists of two structures: the
    linelog and the revmap. this function takes care of locking. only 1
    process is allowed to write that file's linelog and revmap at a time.

    when something goes wrong, this function will assume the linelog and the
    revmap are in a bad state, and remove them from disk.

    use this function in the following way:

        with annotatecontext(...) as actx:
            actx. ....
    """
    # different options use different directories
    subpath = os.path.join('fastannotate', opts.shortstr, path)
    util.makedirs(repo.vfs.join(os.path.dirname(subpath)))
    lockpath = subpath + '.lock'
    lock = lockmod.lock(repo.vfs, lockpath)
    fullpath = repo.vfs.join(subpath)
    revmappath = fullpath + '.m'
    linelogpath = fullpath + '.l'
    linelog = revmap = None
    try:
        with lock:
            if rebuild:
                _unlinkpaths([revmappath, linelogpath])
            revmap = revmapmod.revmap(revmappath)
            linelog = linelogmod.linelog(linelogpath)
            yield _annotatecontext(repo, path, linelog, revmap, opts)
    except Exception:
        revmap = linelog = None
        _unlinkpaths([revmappath, linelogpath])
        repo.ui.debug('fastannotate: %s: cache broken and deleted\n' % path)
        raise
    finally:
        if revmap:
            revmap.flush()
        if linelog:
            linelog.close()
