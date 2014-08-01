# context.py - changeset and file context objects for mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import nullid, nullrev, short, hex, bin
from i18n import _
import mdiff, error, util, scmutil, subrepo, patch, encoding, phases
import match as matchmod
import os, errno, stat
import obsolete as obsmod
import repoview
import fileset
import revlog

propertycache = util.propertycache

class basectx(object):
    """A basectx object represents the common logic for its children:
    changectx: read-only context that is already present in the repo,
    workingctx: a context that represents the working directory and can
                be committed,
    memctx: a context that represents changes in-memory and can also
            be committed."""
    def __new__(cls, repo, changeid='', *args, **kwargs):
        if isinstance(changeid, basectx):
            return changeid

        o = super(basectx, cls).__new__(cls)

        o._repo = repo
        o._rev = nullrev
        o._node = nullid

        return o

    def __str__(self):
        return short(self.node())

    def __int__(self):
        return self.rev()

    def __repr__(self):
        return "<%s %s>" % (type(self).__name__, str(self))

    def __eq__(self, other):
        try:
            return type(self) == type(other) and self._rev == other._rev
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def __contains__(self, key):
        return key in self._manifest

    def __getitem__(self, key):
        return self.filectx(key)

    def __iter__(self):
        for f in sorted(self._manifest):
            yield f

    def _manifestmatches(self, match, s):
        """generate a new manifest filtered by the match argument

        This method is for internal use only and mainly exists to provide an
        object oriented way for other contexts to customize the manifest
        generation.
        """
        if match.always():
            return self.manifest().copy()

        files = match.files()
        if (match.matchfn == match.exact or
            (not match.anypats() and util.all(fn in self for fn in files))):
            return self.manifest().intersectfiles(files)

        mf = self.manifest().copy()
        for fn in mf.keys():
            if not match(fn):
                del mf[fn]
        return mf

    def _matchstatus(self, other, s, match, listignored, listclean,
                     listunknown):
        """return match.always if match is none

        This internal method provides a way for child objects to override the
        match operator.
        """
        return match or matchmod.always(self._repo.root, self._repo.getcwd())

    def _prestatus(self, other, s, match, listignored, listclean, listunknown):
        """provide a hook to allow child objects to preprocess status results

        For example, this allows other contexts, such as workingctx, to query
        the dirstate before comparing the manifests.
        """
        # load earliest manifest first for caching reasons
        if self.rev() < other.rev():
            self.manifest()
        return s

    def _poststatus(self, other, s, match, listignored, listclean, listunknown):
        """provide a hook to allow child objects to postprocess status results

        For example, this allows other contexts, such as workingctx, to filter
        suspect symlinks in the case of FAT32 and NTFS filesytems.
        """
        return s

    def _buildstatus(self, other, s, match, listignored, listclean,
                     listunknown):
        """build a status with respect to another context"""
        mf1 = other._manifestmatches(match, s)
        mf2 = self._manifestmatches(match, s)

        modified, added, clean = [], [], []
        deleted, unknown, ignored = s[3], [], []
        withflags = mf1.withflags() | mf2.withflags()
        for fn, mf2node in mf2.iteritems():
            if fn in mf1:
                if (fn not in deleted and
                    ((fn in withflags and mf1.flags(fn) != mf2.flags(fn)) or
                     (mf1[fn] != mf2node and
                      (mf2node or self[fn].cmp(other[fn]))))):
                    modified.append(fn)
                elif listclean:
                    clean.append(fn)
                del mf1[fn]
            elif fn not in deleted:
                added.append(fn)
        removed = mf1.keys()
        if removed:
            # need to filter files if they are already reported as removed
            unknown = [fn for fn in unknown if fn not in mf1]
            ignored = [fn for fn in ignored if fn not in mf1]

        return [modified, added, removed, deleted, unknown, ignored, clean]

    @propertycache
    def substate(self):
        return subrepo.state(self, self._repo.ui)

    def subrev(self, subpath):
        return self.substate[subpath][1]

    def rev(self):
        return self._rev
    def node(self):
        return self._node
    def hex(self):
        return hex(self.node())
    def manifest(self):
        return self._manifest
    def phasestr(self):
        return phases.phasenames[self.phase()]
    def mutable(self):
        return self.phase() > phases.public

    def getfileset(self, expr):
        return fileset.getfileset(self, expr)

    def obsolete(self):
        """True if the changeset is obsolete"""
        return self.rev() in obsmod.getrevs(self._repo, 'obsolete')

    def extinct(self):
        """True if the changeset is extinct"""
        return self.rev() in obsmod.getrevs(self._repo, 'extinct')

    def unstable(self):
        """True if the changeset is not obsolete but it's ancestor are"""
        return self.rev() in obsmod.getrevs(self._repo, 'unstable')

    def bumped(self):
        """True if the changeset try to be a successor of a public changeset

        Only non-public and non-obsolete changesets may be bumped.
        """
        return self.rev() in obsmod.getrevs(self._repo, 'bumped')

    def divergent(self):
        """Is a successors of a changeset with multiple possible successors set

        Only non-public and non-obsolete changesets may be divergent.
        """
        return self.rev() in obsmod.getrevs(self._repo, 'divergent')

    def troubled(self):
        """True if the changeset is either unstable, bumped or divergent"""
        return self.unstable() or self.bumped() or self.divergent()

    def troubles(self):
        """return the list of troubles affecting this changesets.

        Troubles are returned as strings. possible values are:
        - unstable,
        - bumped,
        - divergent.
        """
        troubles = []
        if self.unstable():
            troubles.append('unstable')
        if self.bumped():
            troubles.append('bumped')
        if self.divergent():
            troubles.append('divergent')
        return troubles

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def p1(self):
        return self._parents[0]

    def p2(self):
        if len(self._parents) == 2:
            return self._parents[1]
        return changectx(self._repo, -1)

    def _fileinfo(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest[path], self._manifest.flags(path)
            except KeyError:
                raise error.ManifestLookupError(self._node, path,
                                                _('not found in manifest'))
        if '_manifestdelta' in self.__dict__ or path in self.files():
            if path in self._manifestdelta:
                return (self._manifestdelta[path],
                        self._manifestdelta.flags(path))
        node, flag = self._repo.manifest.find(self._changeset[0], path)
        if not node:
            raise error.ManifestLookupError(self._node, path,
                                            _('not found in manifest'))

        return node, flag

    def filenode(self, path):
        return self._fileinfo(path)[0]

    def flags(self, path):
        try:
            return self._fileinfo(path)[1]
        except error.LookupError:
            return ''

    def sub(self, path):
        return subrepo.subrepo(self, path)

    def match(self, pats=[], include=None, exclude=None, default='glob'):
        r = self._repo
        return matchmod.match(r.root, r.getcwd(), pats,
                              include, exclude, default,
                              auditor=r.auditor, ctx=self)

    def diff(self, ctx2=None, match=None, **opts):
        """Returns a diff generator for the given contexts and matcher"""
        if ctx2 is None:
            ctx2 = self.p1()
        if ctx2 is not None:
            ctx2 = self._repo[ctx2]
        diffopts = patch.diffopts(self._repo.ui, opts)
        return patch.diff(self._repo, ctx2, self, match=match, opts=diffopts)

    @propertycache
    def _dirs(self):
        return scmutil.dirs(self._manifest)

    def dirs(self):
        return self._dirs

    def dirty(self):
        return False

    def status(self, other=None, match=None, listignored=False,
               listclean=False, listunknown=False, listsubrepos=False):
        """return status of files between two nodes or node and working
        directory.

        If other is None, compare this node with working directory.

        returns (modified, added, removed, deleted, unknown, ignored, clean)
        """

        ctx1 = self
        ctx2 = self._repo[other]

        # This next code block is, admittedly, fragile logic that tests for
        # reversing the contexts and wouldn't need to exist if it weren't for
        # the fast (and common) code path of comparing the working directory
        # with its first parent.
        #
        # What we're aiming for here is the ability to call:
        #
        # workingctx.status(parentctx)
        #
        # If we always built the manifest for each context and compared those,
        # then we'd be done. But the special case of the above call means we
        # just copy the manifest of the parent.
        reversed = False
        if (not isinstance(ctx1, changectx)
            and isinstance(ctx2, changectx)):
            reversed = True
            ctx1, ctx2 = ctx2, ctx1

        r = [[], [], [], [], [], [], []]
        match = ctx2._matchstatus(ctx1, r, match, listignored, listclean,
                                  listunknown)
        r = ctx2._prestatus(ctx1, r, match, listignored, listclean, listunknown)
        r = ctx2._buildstatus(ctx1, r, match, listignored, listclean,
                              listunknown)
        r = ctx2._poststatus(ctx1, r, match, listignored, listclean,
                             listunknown)

        if reversed:
            r[1], r[2], r[3], r[4] = r[2], r[1], r[4], r[3]

        if listsubrepos:
            for subpath, sub in scmutil.itersubrepos(ctx1, ctx2):
                rev2 = ctx2.subrev(subpath)
                try:
                    submatch = matchmod.narrowmatcher(subpath, match)
                    s = sub.status(rev2, match=submatch, ignored=listignored,
                                   clean=listclean, unknown=listunknown,
                                   listsubrepos=True)
                    for rfiles, sfiles in zip(r, s):
                        rfiles.extend("%s/%s" % (subpath, f) for f in sfiles)
                except error.LookupError:
                    self._repo.ui.status(_("skipping missing "
                                           "subrepository: %s\n") % subpath)

        for l in r:
            l.sort()

        # we return a tuple to signify that this list isn't changing
        return tuple(r)


def makememctx(repo, parents, text, user, date, branch, files, store,
               editor=None):
    def getfilectx(repo, memctx, path):
        data, (islink, isexec), copied = store.getfile(path)
        return memfilectx(repo, path, data, islink=islink, isexec=isexec,
                                  copied=copied, memctx=memctx)
    extra = {}
    if branch:
        extra['branch'] = encoding.fromlocal(branch)
    ctx =  memctx(repo, parents, text, files, getfilectx, user,
                          date, extra, editor)
    return ctx

class changectx(basectx):
    """A changecontext object makes access to data related to a particular
    changeset convenient. It represents a read-only context already present in
    the repo."""
    def __init__(self, repo, changeid=''):
        """changeid is a revision number, node, or tag"""

        # since basectx.__new__ already took care of copying the object, we
        # don't need to do anything in __init__, so we just exit here
        if isinstance(changeid, basectx):
            return

        if changeid == '':
            changeid = '.'
        self._repo = repo

        if isinstance(changeid, int):
            try:
                self._node = repo.changelog.node(changeid)
            except IndexError:
                raise error.RepoLookupError(
                    _("unknown revision '%s'") % changeid)
            self._rev = changeid
            return
        if isinstance(changeid, long):
            changeid = str(changeid)
        if changeid == '.':
            self._node = repo.dirstate.p1()
            self._rev = repo.changelog.rev(self._node)
            return
        if changeid == 'null':
            self._node = nullid
            self._rev = nullrev
            return
        if changeid == 'tip':
            self._node = repo.changelog.tip()
            self._rev = repo.changelog.rev(self._node)
            return
        if len(changeid) == 20:
            try:
                self._node = changeid
                self._rev = repo.changelog.rev(changeid)
                return
            except LookupError:
                pass

        try:
            r = int(changeid)
            if str(r) != changeid:
                raise ValueError
            l = len(repo.changelog)
            if r < 0:
                r += l
            if r < 0 or r >= l:
                raise ValueError
            self._rev = r
            self._node = repo.changelog.node(r)
            return
        except (ValueError, OverflowError, IndexError):
            pass

        if len(changeid) == 40:
            try:
                self._node = bin(changeid)
                self._rev = repo.changelog.rev(self._node)
                return
            except (TypeError, LookupError):
                pass

        if changeid in repo._bookmarks:
            self._node = repo._bookmarks[changeid]
            self._rev = repo.changelog.rev(self._node)
            return
        if changeid in repo._tagscache.tags:
            self._node = repo._tagscache.tags[changeid]
            self._rev = repo.changelog.rev(self._node)
            return
        try:
            self._node = repo.branchtip(changeid)
            self._rev = repo.changelog.rev(self._node)
            return
        except error.RepoLookupError:
            pass

        self._node = repo.changelog._partialmatch(changeid)
        if self._node is not None:
            self._rev = repo.changelog.rev(self._node)
            return

        # lookup failed
        # check if it might have come from damaged dirstate
        #
        # XXX we could avoid the unfiltered if we had a recognizable exception
        # for filtered changeset access
        if changeid in repo.unfiltered().dirstate.parents():
            raise error.Abort(_("working directory has unknown parent '%s'!")
                              % short(changeid))
        try:
            if len(changeid) == 20:
                changeid = hex(changeid)
        except TypeError:
            pass
        raise error.RepoLookupError(
            _("unknown revision '%s'") % changeid)

    def __hash__(self):
        try:
            return hash(self._rev)
        except AttributeError:
            return id(self)

    def __nonzero__(self):
        return self._rev != nullrev

    @propertycache
    def _changeset(self):
        return self._repo.changelog.read(self.rev())

    @propertycache
    def _manifest(self):
        return self._repo.manifest.read(self._changeset[0])

    @propertycache
    def _manifestdelta(self):
        return self._repo.manifest.readdelta(self._changeset[0])

    @propertycache
    def _parents(self):
        p = self._repo.changelog.parentrevs(self._rev)
        if p[1] == nullrev:
            p = p[:-1]
        return [changectx(self._repo, x) for x in p]

    def changeset(self):
        return self._changeset
    def manifestnode(self):
        return self._changeset[0]

    def user(self):
        return self._changeset[1]
    def date(self):
        return self._changeset[2]
    def files(self):
        return self._changeset[3]
    def description(self):
        return self._changeset[4]
    def branch(self):
        return encoding.tolocal(self._changeset[5].get("branch"))
    def closesbranch(self):
        return 'close' in self._changeset[5]
    def extra(self):
        return self._changeset[5]
    def tags(self):
        return self._repo.nodetags(self._node)
    def bookmarks(self):
        return self._repo.nodebookmarks(self._node)
    def phase(self):
        return self._repo._phasecache.phase(self._repo, self._rev)
    def hidden(self):
        return self._rev in repoview.filterrevs(self._repo, 'visible')

    def children(self):
        """return contexts for each child changeset"""
        c = self._repo.changelog.children(self._node)
        return [changectx(self._repo, x) for x in c]

    def ancestors(self):
        for a in self._repo.changelog.ancestors([self._rev]):
            yield changectx(self._repo, a)

    def descendants(self):
        for d in self._repo.changelog.descendants([self._rev]):
            yield changectx(self._repo, d)

    def filectx(self, path, fileid=None, filelog=None):
        """get a file context from this changeset"""
        if fileid is None:
            fileid = self.filenode(path)
        return filectx(self._repo, path, fileid=fileid,
                       changectx=self, filelog=filelog)

    def ancestor(self, c2, warn=False):
        """
        return the "best" ancestor context of self and c2
        """
        # deal with workingctxs
        n2 = c2._node
        if n2 is None:
            n2 = c2._parents[0]._node
        cahs = self._repo.changelog.commonancestorsheads(self._node, n2)
        if not cahs:
            anc = nullid
        elif len(cahs) == 1:
            anc = cahs[0]
        else:
            for r in self._repo.ui.configlist('merge', 'preferancestor'):
                ctx = changectx(self._repo, r)
                anc = ctx.node()
                if anc in cahs:
                    break
            else:
                anc = self._repo.changelog.ancestor(self._node, n2)
            if warn:
                self._repo.ui.status(
                    (_("note: using %s as ancestor of %s and %s\n") %
                     (short(anc), short(self._node), short(n2))) +
                    ''.join(_("      alternatively, use --config "
                              "merge.preferancestor=%s\n") %
                            short(n) for n in sorted(cahs) if n != anc))
        return changectx(self._repo, anc)

    def descendant(self, other):
        """True if other is descendant of this changeset"""
        return self._repo.changelog.descendant(self._rev, other._rev)

    def walk(self, match):
        fset = set(match.files())
        # for dirstate.walk, files=['.'] means "walk the whole tree".
        # follow that here, too
        fset.discard('.')

        # avoid the entire walk if we're only looking for specific files
        if fset and not match.anypats():
            if util.all([fn in self for fn in fset]):
                for fn in sorted(fset):
                    if match(fn):
                        yield fn
                raise StopIteration

        for fn in self:
            if fn in fset:
                # specified pattern is the exact name
                fset.remove(fn)
            if match(fn):
                yield fn
        for fn in sorted(fset):
            if fn in self._dirs:
                # specified pattern is a directory
                continue
            match.bad(fn, _('no such file in rev %s') % self)

class basefilectx(object):
    """A filecontext object represents the common logic for its children:
    filectx: read-only access to a filerevision that is already present
             in the repo,
    workingfilectx: a filecontext that represents files from the working
                    directory,
    memfilectx: a filecontext that represents files in-memory."""
    def __new__(cls, repo, path, *args, **kwargs):
        return super(basefilectx, cls).__new__(cls)

    @propertycache
    def _filelog(self):
        return self._repo.file(self._path)

    @propertycache
    def _changeid(self):
        if '_changeid' in self.__dict__:
            return self._changeid
        elif '_changectx' in self.__dict__:
            return self._changectx.rev()
        else:
            return self._filelog.linkrev(self._filerev)

    @propertycache
    def _filenode(self):
        if '_fileid' in self.__dict__:
            return self._filelog.lookup(self._fileid)
        else:
            return self._changectx.filenode(self._path)

    @propertycache
    def _filerev(self):
        return self._filelog.rev(self._filenode)

    @propertycache
    def _repopath(self):
        return self._path

    def __nonzero__(self):
        try:
            self._filenode
            return True
        except error.LookupError:
            # file is missing
            return False

    def __str__(self):
        return "%s@%s" % (self.path(), self._changectx)

    def __repr__(self):
        return "<%s %s>" % (type(self).__name__, str(self))

    def __hash__(self):
        try:
            return hash((self._path, self._filenode))
        except AttributeError:
            return id(self)

    def __eq__(self, other):
        try:
            return (type(self) == type(other) and self._path == other._path
                    and self._filenode == other._filenode)
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def filerev(self):
        return self._filerev
    def filenode(self):
        return self._filenode
    def flags(self):
        return self._changectx.flags(self._path)
    def filelog(self):
        return self._filelog
    def rev(self):
        return self._changeid
    def linkrev(self):
        return self._filelog.linkrev(self._filerev)
    def node(self):
        return self._changectx.node()
    def hex(self):
        return self._changectx.hex()
    def user(self):
        return self._changectx.user()
    def date(self):
        return self._changectx.date()
    def files(self):
        return self._changectx.files()
    def description(self):
        return self._changectx.description()
    def branch(self):
        return self._changectx.branch()
    def extra(self):
        return self._changectx.extra()
    def phase(self):
        return self._changectx.phase()
    def phasestr(self):
        return self._changectx.phasestr()
    def manifest(self):
        return self._changectx.manifest()
    def changectx(self):
        return self._changectx

    def path(self):
        return self._path

    def isbinary(self):
        try:
            return util.binary(self.data())
        except IOError:
            return False

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        if (fctx._filerev is None
            and (self._repo._encodefilterpats
                 # if file data starts with '\1\n', empty metadata block is
                 # prepended, which adds 4 bytes to filelog.size().
                 or self.size() - 4 == fctx.size())
            or self.size() == fctx.size()):
            return self._filelog.cmp(self._filenode, fctx.data())

        return True

    def parents(self):
        p = self._path
        fl = self._filelog
        pl = [(p, n, fl) for n in self._filelog.parents(self._filenode)]

        r = self._filelog.renamed(self._filenode)
        if r:
            pl[0] = (r[0], r[1], None)

        return [filectx(self._repo, p, fileid=n, filelog=l)
                for p, n, l in pl if n != nullid]

    def p1(self):
        return self.parents()[0]

    def p2(self):
        p = self.parents()
        if len(p) == 2:
            return p[1]
        return filectx(self._repo, self._path, fileid=-1, filelog=self._filelog)

    def annotate(self, follow=False, linenumber=None, diffopts=None):
        '''returns a list of tuples of (ctx, line) for each line
        in the file, where ctx is the filectx of the node where
        that line was last changed.
        This returns tuples of ((ctx, linenumber), line) for each line,
        if "linenumber" parameter is NOT "None".
        In such tuples, linenumber means one at the first appearance
        in the managed file.
        To reduce annotation cost,
        this returns fixed value(False is used) as linenumber,
        if "linenumber" parameter is "False".'''

        def decorate_compat(text, rev):
            return ([rev] * len(text.splitlines()), text)

        def without_linenumber(text, rev):
            return ([(rev, False)] * len(text.splitlines()), text)

        def with_linenumber(text, rev):
            size = len(text.splitlines())
            return ([(rev, i) for i in xrange(1, size + 1)], text)

        decorate = (((linenumber is None) and decorate_compat) or
                    (linenumber and with_linenumber) or
                    without_linenumber)

        def pair(parent, child):
            blocks = mdiff.allblocks(parent[1], child[1], opts=diffopts,
                                     refine=True)
            for (a1, a2, b1, b2), t in blocks:
                # Changed blocks ('!') or blocks made only of blank lines ('~')
                # belong to the child.
                if t == '=':
                    child[0][b1:b2] = parent[0][a1:a2]
            return child

        getlog = util.lrucachefunc(lambda x: self._repo.file(x))

        def parents(f):
            pl = f.parents()

            # Don't return renamed parents if we aren't following.
            if not follow:
                pl = [p for p in pl if p.path() == f.path()]

            # renamed filectx won't have a filelog yet, so set it
            # from the cache to save time
            for p in pl:
                if not '_filelog' in p.__dict__:
                    p._filelog = getlog(p.path())

            return pl

        # use linkrev to find the first changeset where self appeared
        if self.rev() != self.linkrev():
            base = self.filectx(self.filenode())
        else:
            base = self

        # This algorithm would prefer to be recursive, but Python is a
        # bit recursion-hostile. Instead we do an iterative
        # depth-first search.

        visit = [base]
        hist = {}
        pcache = {}
        needed = {base: 1}
        while visit:
            f = visit[-1]
            pcached = f in pcache
            if not pcached:
                pcache[f] = parents(f)

            ready = True
            pl = pcache[f]
            for p in pl:
                if p not in hist:
                    ready = False
                    visit.append(p)
                if not pcached:
                    needed[p] = needed.get(p, 0) + 1
            if ready:
                visit.pop()
                reusable = f in hist
                if reusable:
                    curr = hist[f]
                else:
                    curr = decorate(f.data(), f)
                for p in pl:
                    if not reusable:
                        curr = pair(hist[p], curr)
                    if needed[p] == 1:
                        del hist[p]
                        del needed[p]
                    else:
                        needed[p] -= 1

                hist[f] = curr
                pcache[f] = []

        return zip(hist[base][0], hist[base][1].splitlines(True))

    def ancestors(self, followfirst=False):
        visit = {}
        c = self
        cut = followfirst and 1 or None
        while True:
            for parent in c.parents()[:cut]:
                visit[(parent.rev(), parent.node())] = parent
            if not visit:
                break
            c = visit.pop(max(visit))
            yield c

class filectx(basefilectx):
    """A filecontext object makes access to data related to a particular
       filerevision convenient."""
    def __init__(self, repo, path, changeid=None, fileid=None,
                 filelog=None, changectx=None):
        """changeid can be a changeset revision, node, or tag.
           fileid can be a file revision or node."""
        self._repo = repo
        self._path = path

        assert (changeid is not None
                or fileid is not None
                or changectx is not None), \
                ("bad args: changeid=%r, fileid=%r, changectx=%r"
                 % (changeid, fileid, changectx))

        if filelog is not None:
            self._filelog = filelog

        if changeid is not None:
            self._changeid = changeid
        if changectx is not None:
            self._changectx = changectx
        if fileid is not None:
            self._fileid = fileid

    @propertycache
    def _changectx(self):
        try:
            return changectx(self._repo, self._changeid)
        except error.RepoLookupError:
            # Linkrev may point to any revision in the repository.  When the
            # repository is filtered this may lead to `filectx` trying to build
            # `changectx` for filtered revision. In such case we fallback to
            # creating `changectx` on the unfiltered version of the reposition.
            # This fallback should not be an issue because `changectx` from
            # `filectx` are not used in complex operations that care about
            # filtering.
            #
            # This fallback is a cheap and dirty fix that prevent several
            # crashes. It does not ensure the behavior is correct. However the
            # behavior was not correct before filtering either and "incorrect
            # behavior" is seen as better as "crash"
            #
            # Linkrevs have several serious troubles with filtering that are
            # complicated to solve. Proper handling of the issue here should be
            # considered when solving linkrev issue are on the table.
            return changectx(self._repo.unfiltered(), self._changeid)

    def filectx(self, fileid):
        '''opens an arbitrary revision of the file without
        opening a new filelog'''
        return filectx(self._repo, self._path, fileid=fileid,
                       filelog=self._filelog)

    def data(self):
        return self._filelog.read(self._filenode)
    def size(self):
        return self._filelog.size(self._filerev)

    def renamed(self):
        """check if file was actually renamed in this changeset revision

        If rename logged in file revision, we report copy for changeset only
        if file revisions linkrev points back to the changeset in question
        or both changeset parents contain different file revisions.
        """

        renamed = self._filelog.renamed(self._filenode)
        if not renamed:
            return renamed

        if self.rev() == self.linkrev():
            return renamed

        name = self.path()
        fnode = self._filenode
        for p in self._changectx.parents():
            try:
                if fnode == p.filenode(name):
                    return None
            except error.LookupError:
                pass
        return renamed

    def children(self):
        # hard for renames
        c = self._filelog.children(self._filenode)
        return [filectx(self._repo, self._path, fileid=x,
                        filelog=self._filelog) for x in c]

class committablectx(basectx):
    """A committablectx object provides common functionality for a context that
    wants the ability to commit, e.g. workingctx or memctx."""
    def __init__(self, repo, text="", user=None, date=None, extra=None,
                 changes=None):
        self._repo = repo
        self._rev = None
        self._node = None
        self._text = text
        if date:
            self._date = util.parsedate(date)
        if user:
            self._user = user
        if changes:
            self._status = changes

        self._extra = {}
        if extra:
            self._extra = extra.copy()
        if 'branch' not in self._extra:
            try:
                branch = encoding.fromlocal(self._repo.dirstate.branch())
            except UnicodeDecodeError:
                raise util.Abort(_('branch name not in UTF-8!'))
            self._extra['branch'] = branch
        if self._extra['branch'] == '':
            self._extra['branch'] = 'default'

    def __str__(self):
        return str(self._parents[0]) + "+"

    def __nonzero__(self):
        return True

    def _buildflagfunc(self):
        # Create a fallback function for getting file flags when the
        # filesystem doesn't support them

        copiesget = self._repo.dirstate.copies().get

        if len(self._parents) < 2:
            # when we have one parent, it's easy: copy from parent
            man = self._parents[0].manifest()
            def func(f):
                f = copiesget(f, f)
                return man.flags(f)
        else:
            # merges are tricky: we try to reconstruct the unstored
            # result from the merge (issue1802)
            p1, p2 = self._parents
            pa = p1.ancestor(p2)
            m1, m2, ma = p1.manifest(), p2.manifest(), pa.manifest()

            def func(f):
                f = copiesget(f, f) # may be wrong for merges with copies
                fl1, fl2, fla = m1.flags(f), m2.flags(f), ma.flags(f)
                if fl1 == fl2:
                    return fl1
                if fl1 == fla:
                    return fl2
                if fl2 == fla:
                    return fl1
                return '' # punt for conflicts

        return func

    @propertycache
    def _flagfunc(self):
        return self._repo.dirstate.flagfunc(self._buildflagfunc)

    @propertycache
    def _manifest(self):
        """generate a manifest corresponding to the values in self._status"""

        man = self._parents[0].manifest().copy()
        if len(self._parents) > 1:
            man2 = self.p2().manifest()
            def getman(f):
                if f in man:
                    return man
                return man2
        else:
            getman = lambda f: man

        copied = self._repo.dirstate.copies()
        ff = self._flagfunc
        modified, added, removed, deleted = self._status[:4]
        for i, l in (("a", added), ("m", modified)):
            for f in l:
                orig = copied.get(f, f)
                man[f] = getman(orig).get(orig, nullid) + i
                try:
                    man.set(f, ff(f))
                except OSError:
                    pass

        for f in deleted + removed:
            if f in man:
                del man[f]

        return man

    @propertycache
    def _status(self):
        return self._repo.status()

    @propertycache
    def _user(self):
        return self._repo.ui.username()

    @propertycache
    def _date(self):
        return util.makedate()

    def subrev(self, subpath):
        return None

    def user(self):
        return self._user or self._repo.ui.username()
    def date(self):
        return self._date
    def description(self):
        return self._text
    def files(self):
        return sorted(self._status[0] + self._status[1] + self._status[2])

    def modified(self):
        return self._status[0]
    def added(self):
        return self._status[1]
    def removed(self):
        return self._status[2]
    def deleted(self):
        return self._status[3]
    def unknown(self):
        return self._status[4]
    def ignored(self):
        return self._status[5]
    def clean(self):
        return self._status[6]
    def branch(self):
        return encoding.tolocal(self._extra['branch'])
    def closesbranch(self):
        return 'close' in self._extra
    def extra(self):
        return self._extra

    def tags(self):
        t = []
        for p in self.parents():
            t.extend(p.tags())
        return t

    def bookmarks(self):
        b = []
        for p in self.parents():
            b.extend(p.bookmarks())
        return b

    def phase(self):
        phase = phases.draft # default phase to draft
        for p in self.parents():
            phase = max(phase, p.phase())
        return phase

    def hidden(self):
        return False

    def children(self):
        return []

    def flags(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest.flags(path)
            except KeyError:
                return ''

        try:
            return self._flagfunc(path)
        except OSError:
            return ''

    def ancestor(self, c2):
        """return the ancestor context of self and c2"""
        return self._parents[0].ancestor(c2) # punt on two parents for now

    def walk(self, match):
        return sorted(self._repo.dirstate.walk(match, sorted(self.substate),
                                               True, False))

    def ancestors(self):
        for a in self._repo.changelog.ancestors(
            [p.rev() for p in self._parents]):
            yield changectx(self._repo, a)

    def markcommitted(self, node):
        """Perform post-commit cleanup necessary after committing this ctx

        Specifically, this updates backing stores this working context
        wraps to reflect the fact that the changes reflected by this
        workingctx have been committed.  For example, it marks
        modified and added files as normal in the dirstate.

        """

        for f in self.modified() + self.added():
            self._repo.dirstate.normal(f)
        for f in self.removed():
            self._repo.dirstate.drop(f)
        self._repo.dirstate.setparents(node)

    def dirs(self):
        return self._repo.dirstate.dirs()

class workingctx(committablectx):
    """A workingctx object makes access to data related to
    the current working directory convenient.
    date - any valid date string or (unixtime, offset), or None.
    user - username string, or None.
    extra - a dictionary of extra values, or None.
    changes - a list of file lists as returned by localrepo.status()
               or None to use the repository status.
    """
    def __init__(self, repo, text="", user=None, date=None, extra=None,
                 changes=None):
        super(workingctx, self).__init__(repo, text, user, date, extra, changes)

    def __iter__(self):
        d = self._repo.dirstate
        for f in d:
            if d[f] != 'r':
                yield f

    def __contains__(self, key):
        return self._repo.dirstate[key] not in "?r"

    @propertycache
    def _parents(self):
        p = self._repo.dirstate.parents()
        if p[1] == nullid:
            p = p[:-1]
        return [changectx(self._repo, x) for x in p]

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return workingfilectx(self._repo, path, workingctx=self,
                              filelog=filelog)

    def dirty(self, missing=False, merge=True, branch=True):
        "check whether a working directory is modified"
        # check subrepos first
        for s in sorted(self.substate):
            if self.sub(s).dirty():
                return True
        # check current working dir
        return ((merge and self.p2()) or
                (branch and self.branch() != self.p1().branch()) or
                self.modified() or self.added() or self.removed() or
                (missing and self.deleted()))

    def add(self, list, prefix=""):
        join = lambda f: os.path.join(prefix, f)
        wlock = self._repo.wlock()
        ui, ds = self._repo.ui, self._repo.dirstate
        try:
            rejected = []
            lstat = self._repo.wvfs.lstat
            for f in list:
                scmutil.checkportable(ui, join(f))
                try:
                    st = lstat(f)
                except OSError:
                    ui.warn(_("%s does not exist!\n") % join(f))
                    rejected.append(f)
                    continue
                if st.st_size > 10000000:
                    ui.warn(_("%s: up to %d MB of RAM may be required "
                              "to manage this file\n"
                              "(use 'hg revert %s' to cancel the "
                              "pending addition)\n")
                              % (f, 3 * st.st_size // 1000000, join(f)))
                if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
                    ui.warn(_("%s not added: only files and symlinks "
                              "supported currently\n") % join(f))
                    rejected.append(f)
                elif ds[f] in 'amn':
                    ui.warn(_("%s already tracked!\n") % join(f))
                elif ds[f] == 'r':
                    ds.normallookup(f)
                else:
                    ds.add(f)
            return rejected
        finally:
            wlock.release()

    def forget(self, files, prefix=""):
        join = lambda f: os.path.join(prefix, f)
        wlock = self._repo.wlock()
        try:
            rejected = []
            for f in files:
                if f not in self._repo.dirstate:
                    self._repo.ui.warn(_("%s not tracked!\n") % join(f))
                    rejected.append(f)
                elif self._repo.dirstate[f] != 'a':
                    self._repo.dirstate.remove(f)
                else:
                    self._repo.dirstate.drop(f)
            return rejected
        finally:
            wlock.release()

    def undelete(self, list):
        pctxs = self.parents()
        wlock = self._repo.wlock()
        try:
            for f in list:
                if self._repo.dirstate[f] != 'r':
                    self._repo.ui.warn(_("%s not removed!\n") % f)
                else:
                    fctx = f in pctxs[0] and pctxs[0][f] or pctxs[1][f]
                    t = fctx.data()
                    self._repo.wwrite(f, t, fctx.flags())
                    self._repo.dirstate.normal(f)
        finally:
            wlock.release()

    def copy(self, source, dest):
        try:
            st = self._repo.wvfs.lstat(dest)
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise
            self._repo.ui.warn(_("%s does not exist!\n") % dest)
            return
        if not (stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode)):
            self._repo.ui.warn(_("copy failed: %s is not a file or a "
                                 "symbolic link\n") % dest)
        else:
            wlock = self._repo.wlock()
            try:
                if self._repo.dirstate[dest] in '?r':
                    self._repo.dirstate.add(dest)
                self._repo.dirstate.copy(source, dest)
            finally:
                wlock.release()

    def _filtersuspectsymlink(self, files):
        if not files or self._repo.dirstate._checklink:
            return files

        # Symlink placeholders may get non-symlink-like contents
        # via user error or dereferencing by NFS or Samba servers,
        # so we filter out any placeholders that don't look like a
        # symlink
        sane = []
        for f in files:
            if self.flags(f) == 'l':
                d = self[f].data()
                if d == '' or len(d) >= 1024 or '\n' in d or util.binary(d):
                    self._repo.ui.debug('ignoring suspect symlink placeholder'
                                        ' "%s"\n' % f)
                    continue
            sane.append(f)
        return sane

    def _checklookup(self, files):
        # check for any possibly clean files
        if not files:
            return [], []

        modified = []
        fixup = []
        pctx = self._parents[0]
        # do a full compare of any files that might have changed
        for f in sorted(files):
            if (f not in pctx or self.flags(f) != pctx.flags(f)
                or pctx[f].cmp(self[f])):
                modified.append(f)
            else:
                fixup.append(f)

        # update dirstate for files that are actually clean
        if fixup:
            try:
                # updating the dirstate is optional
                # so we don't wait on the lock
                normal = self._repo.dirstate.normal
                wlock = self._repo.wlock(False)
                try:
                    for f in fixup:
                        normal(f)
                finally:
                    wlock.release()
            except error.LockError:
                pass
        return modified, fixup

    def _manifestmatches(self, match, s):
        """Slow path for workingctx

        The fast path is when we compare the working directory to its parent
        which means this function is comparing with a non-parent; therefore we
        need to build a manifest and return what matches.
        """
        mf = self._repo['.']._manifestmatches(match, s)
        modified, added, removed = s[0:3]
        for f in modified + added:
            mf[f] = None
            mf.set(f, self.flags(f))
        for f in removed:
            if f in mf:
                del mf[f]
        return mf

    def _prestatus(self, other, s, match, listignored, listclean, listunknown):
        """override the parent hook with a dirstate query

        We use this prestatus hook to populate the status with information from
        the dirstate.
        """
        # doesn't need to call super; if that changes, be aware that super
        # calls self.manifest which would slow down the common case of calling
        # status against a workingctx's parent
        return self._dirstatestatus(match, listignored, listclean, listunknown)

    def _poststatus(self, other, s, match, listignored, listclean, listunknown):
        """override the parent hook with a filter for suspect symlinks

        We use this poststatus hook to filter out symlinks that might have
        accidentally ended up with the entire contents of the file they are
        susposed to be linking to.
        """
        s[0] = self._filtersuspectsymlink(s[0])
        self._status = s[:]
        return s

    def _dirstatestatus(self, match=None, ignored=False, clean=False,
                        unknown=False):
        '''Gets the status from the dirstate -- internal use only.'''
        listignored, listclean, listunknown = ignored, clean, unknown
        match = match or matchmod.always(self._repo.root, self._repo.getcwd())
        subrepos = []
        if '.hgsub' in self:
            subrepos = sorted(self.substate)
        s = self._repo.dirstate.status(match, subrepos, listignored,
                                       listclean, listunknown)
        cmp, modified, added, removed, deleted, unknown, ignored, clean = s

        # check for any possibly clean files
        if cmp:
            modified2, fixup = self._checklookup(cmp)
            modified += modified2

            # update dirstate for files that are actually clean
            if fixup and listclean:
                clean += fixup

        return [modified, added, removed, deleted, unknown, ignored, clean]

    def _buildstatus(self, other, s, match, listignored, listclean,
                     listunknown):
        """build a status with respect to another context

        This includes logic for maintaining the fast path of status when
        comparing the working directory against its parent, which is to skip
        building a new manifest if self (working directory) is not comparing
        against its parent (repo['.']).
        """
        if other != self._repo['.']:
            s = super(workingctx, self)._buildstatus(other, s, match,
                                                     listignored, listclean,
                                                     listunknown)
        return s

    def _matchstatus(self, other, s, match, listignored, listclean,
                     listunknown):
        """override the match method with a filter for directory patterns

        We use inheritance to customize the match.bad method only in cases of
        workingctx since it belongs only to the working directory when
        comparing against the parent changeset.

        If we aren't comparing against the working directory's parent, then we
        just use the default match object sent to us.
        """
        superself = super(workingctx, self)
        match = superself._matchstatus(other, s, match, listignored, listclean,
                                       listunknown)
        if other != self._repo['.']:
            def bad(f, msg):
                # 'f' may be a directory pattern from 'match.files()',
                # so 'f not in ctx1' is not enough
                if f not in other and f not in other.dirs():
                    self._repo.ui.warn('%s: %s\n' %
                                       (self._repo.dirstate.pathto(f), msg))
            match.bad = bad
        return match

    def status(self, other='.', match=None, listignored=False,
               listclean=False, listunknown=False, listsubrepos=False):
        # yet to be determined: what to do if 'other' is a 'workingctx' or a
        # 'memctx'?
        s = super(workingctx, self).status(other, match, listignored, listclean,
                                           listunknown, listsubrepos)
        # calling 'super' subtly reveresed the contexts, so we flip the results
        # (s[1] is 'added' and s[2] is 'removed')
        s = list(s)
        s[1], s[2] = s[2], s[1]
        return tuple(s)

class committablefilectx(basefilectx):
    """A committablefilectx provides common functionality for a file context
    that wants the ability to commit, e.g. workingfilectx or memfilectx."""
    def __init__(self, repo, path, filelog=None, ctx=None):
        self._repo = repo
        self._path = path
        self._changeid = None
        self._filerev = self._filenode = None

        if filelog is not None:
            self._filelog = filelog
        if ctx:
            self._changectx = ctx

    def __nonzero__(self):
        return True

    def parents(self):
        '''return parent filectxs, following copies if necessary'''
        def filenode(ctx, path):
            return ctx._manifest.get(path, nullid)

        path = self._path
        fl = self._filelog
        pcl = self._changectx._parents
        renamed = self.renamed()

        if renamed:
            pl = [renamed + (None,)]
        else:
            pl = [(path, filenode(pcl[0], path), fl)]

        for pc in pcl[1:]:
            pl.append((path, filenode(pc, path), fl))

        return [filectx(self._repo, p, fileid=n, filelog=l)
                for p, n, l in pl if n != nullid]

    def children(self):
        return []

class workingfilectx(committablefilectx):
    """A workingfilectx object makes access to data related to a particular
       file in the working directory convenient."""
    def __init__(self, repo, path, filelog=None, workingctx=None):
        super(workingfilectx, self).__init__(repo, path, filelog, workingctx)

    @propertycache
    def _changectx(self):
        return workingctx(self._repo)

    def data(self):
        return self._repo.wread(self._path)
    def renamed(self):
        rp = self._repo.dirstate.copied(self._path)
        if not rp:
            return None
        return rp, self._changectx._parents[0]._manifest.get(rp, nullid)

    def size(self):
        return self._repo.wvfs.lstat(self._path).st_size
    def date(self):
        t, tz = self._changectx.date()
        try:
            return (int(self._repo.wvfs.lstat(self._path).st_mtime), tz)
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise
            return (t, tz)

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        # fctx should be a filectx (not a workingfilectx)
        # invert comparison to reuse the same code path
        return fctx.cmp(self)

class memctx(committablectx):
    """Use memctx to perform in-memory commits via localrepo.commitctx().

    Revision information is supplied at initialization time while
    related files data and is made available through a callback
    mechanism.  'repo' is the current localrepo, 'parents' is a
    sequence of two parent revisions identifiers (pass None for every
    missing parent), 'text' is the commit message and 'files' lists
    names of files touched by the revision (normalized and relative to
    repository root).

    filectxfn(repo, memctx, path) is a callable receiving the
    repository, the current memctx object and the normalized path of
    requested file, relative to repository root. It is fired by the
    commit function for every file in 'files', but calls order is
    undefined. If the file is available in the revision being
    committed (updated or added), filectxfn returns a memfilectx
    object. If the file was removed, filectxfn raises an
    IOError. Moved files are represented by marking the source file
    removed and the new file added with copy information (see
    memfilectx).

    user receives the committer name and defaults to current
    repository username, date is the commit date in any format
    supported by util.parsedate() and defaults to current date, extra
    is a dictionary of metadata or is left empty.
    """
    def __init__(self, repo, parents, text, files, filectxfn, user=None,
                 date=None, extra=None, editor=False):
        super(memctx, self).__init__(repo, text, user, date, extra)
        self._rev = None
        self._node = None
        parents = [(p or nullid) for p in parents]
        p1, p2 = parents
        self._parents = [changectx(self._repo, p) for p in (p1, p2)]
        files = sorted(set(files))
        self._status = [files, [], [], [], []]
        self._filectxfn = filectxfn
        self.substate = {}

        self._extra = extra and extra.copy() or {}
        if self._extra.get('branch', '') == '':
            self._extra['branch'] = 'default'

        if editor:
            self._text = editor(self._repo, self, [])
            self._repo.savecommitmessage(self._text)

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return self._filectxfn(self._repo, self, path)

    def commit(self):
        """commit context to the repo"""
        return self._repo.commitctx(self)

    @propertycache
    def _manifest(self):
        """generate a manifest based on the return values of filectxfn"""

        # keep this simple for now; just worry about p1
        pctx = self._parents[0]
        man = pctx.manifest().copy()

        for f, fnode in man.iteritems():
            p1node = nullid
            p2node = nullid
            p = pctx[f].parents()
            if len(p) > 0:
                p1node = p[0].node()
                if len(p) > 1:
                    p2node = p[1].node()
            man[f] = revlog.hash(self[f].data(), p1node, p2node)

        return man


class memfilectx(committablefilectx):
    """memfilectx represents an in-memory file to commit.

    See memctx and commitablefilectx for more details.
    """
    def __init__(self, repo, path, data, islink=False,
                 isexec=False, copied=None, memctx=None):
        """
        path is the normalized file path relative to repository root.
        data is the file content as a string.
        islink is True if the file is a symbolic link.
        isexec is True if the file is executable.
        copied is the source file path if current file was copied in the
        revision being committed, or None."""
        super(memfilectx, self).__init__(repo, path, None, memctx)
        self._data = data
        self._flags = (islink and 'l' or '') + (isexec and 'x' or '')
        self._copied = None
        if copied:
            self._copied = (copied, nullid)

    def data(self):
        return self._data
    def size(self):
        return len(self.data())
    def flags(self):
        return self._flags
    def isexec(self):
        return 'x' in self._flags
    def islink(self):
        return 'l' in self._flags
    def renamed(self):
        return self._copied
