# context.py - changeset and file context objects for mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import nullid, nullrev, short, hex
from i18n import _
import ancestor, bdiff, error, util, subrepo, patch, encoding
import os, errno, stat

propertycache = util.propertycache

class changectx(object):
    """A changecontext object makes access to data related to a particular
    changeset convenient."""
    def __init__(self, repo, changeid=''):
        """changeid is a revision number, node, or tag"""
        if changeid == '':
            changeid = '.'
        self._repo = repo
        if isinstance(changeid, (long, int)):
            self._rev = changeid
            self._node = self._repo.changelog.node(changeid)
        else:
            self._node = self._repo.lookup(changeid)
            self._rev = self._repo.changelog.rev(self._node)

    def __str__(self):
        return short(self.node())

    def __int__(self):
        return self.rev()

    def __repr__(self):
        return "<changectx %s>" % str(self)

    def __hash__(self):
        try:
            return hash(self._rev)
        except AttributeError:
            return id(self)

    def __eq__(self, other):
        try:
            return self._rev == other._rev
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def __nonzero__(self):
        return self._rev != nullrev

    @propertycache
    def _changeset(self):
        return self._repo.changelog.read(self.node())

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

    @propertycache
    def substate(self):
        return subrepo.state(self, self._repo.ui)

    def __contains__(self, key):
        return key in self._manifest

    def __getitem__(self, key):
        return self.filectx(key)

    def __iter__(self):
        for f in sorted(self._manifest):
            yield f

    def changeset(self):
        return self._changeset
    def manifest(self):
        return self._manifest
    def manifestnode(self):
        return self._changeset[0]

    def rev(self):
        return self._rev
    def node(self):
        return self._node
    def hex(self):
        return hex(self._node)
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
    def extra(self):
        return self._changeset[5]
    def tags(self):
        return self._repo.nodetags(self._node)
    def bookmarks(self):
        return self._repo.nodebookmarks(self._node)

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def p1(self):
        return self._parents[0]

    def p2(self):
        if len(self._parents) == 2:
            return self._parents[1]
        return changectx(self._repo, -1)

    def children(self):
        """return contexts for each child changeset"""
        c = self._repo.changelog.children(self._node)
        return [changectx(self._repo, x) for x in c]

    def ancestors(self):
        for a in self._repo.changelog.ancestors(self._rev):
            yield changectx(self._repo, a)

    def descendants(self):
        for d in self._repo.changelog.descendants(self._rev):
            yield changectx(self._repo, d)

    def _fileinfo(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest[path], self._manifest.flags(path)
            except KeyError:
                raise error.LookupError(self._node, path,
                                        _('not found in manifest'))
        if '_manifestdelta' in self.__dict__ or path in self.files():
            if path in self._manifestdelta:
                return self._manifestdelta[path], self._manifestdelta.flags(path)
        node, flag = self._repo.manifest.find(self._changeset[0], path)
        if not node:
            raise error.LookupError(self._node, path,
                                    _('not found in manifest'))

        return node, flag

    def filenode(self, path):
        return self._fileinfo(path)[0]

    def flags(self, path):
        try:
            return self._fileinfo(path)[1]
        except error.LookupError:
            return ''

    def filectx(self, path, fileid=None, filelog=None):
        """get a file context from this changeset"""
        if fileid is None:
            fileid = self.filenode(path)
        return filectx(self._repo, path, fileid=fileid,
                       changectx=self, filelog=filelog)

    def ancestor(self, c2):
        """
        return the ancestor context of self and c2
        """
        # deal with workingctxs
        n2 = c2._node
        if n2 is None:
            n2 = c2._parents[0]._node
        n = self._repo.changelog.ancestor(self._node, n2)
        return changectx(self._repo, n)

    def walk(self, match):
        fset = set(match.files())
        # for dirstate.walk, files=['.'] means "walk the whole tree".
        # follow that here, too
        fset.discard('.')
        for fn in self:
            for ffn in fset:
                # match if the file is the exact name or a directory
                if ffn == fn or fn.startswith("%s/" % ffn):
                    fset.remove(ffn)
                    break
            if match(fn):
                yield fn
        for fn in sorted(fset):
            if match.bad(fn, _('no such file in rev %s') % self) and match(fn):
                yield fn

    def sub(self, path):
        return subrepo.subrepo(self, path)

    def diff(self, ctx2=None, match=None, **opts):
        """Returns a diff generator for the given contexts and matcher"""
        if ctx2 is None:
            ctx2 = self.p1()
        if ctx2 is not None and not isinstance(ctx2, changectx):
            ctx2 = self._repo[ctx2]
        diffopts = patch.diffopts(self._repo.ui, opts)
        return patch.diff(self._repo, ctx2.node(), self.node(),
                          match=match, opts=diffopts)

class filectx(object):
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

        if filelog:
            self._filelog = filelog

        if changeid is not None:
            self._changeid = changeid
        if changectx is not None:
            self._changectx = changectx
        if fileid is not None:
            self._fileid = fileid

    @propertycache
    def _changectx(self):
        return changectx(self._repo, self._changeid)

    @propertycache
    def _filelog(self):
        return self._repo.file(self._path)

    @propertycache
    def _changeid(self):
        if '_changectx' in self.__dict__:
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
        return "%s@%s" % (self.path(), short(self.node()))

    def __repr__(self):
        return "<filectx %s>" % str(self)

    def __hash__(self):
        try:
            return hash((self._path, self._filenode))
        except AttributeError:
            return id(self)

    def __eq__(self, other):
        try:
            return (self._path == other._path
                    and self._filenode == other._filenode)
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def filectx(self, fileid):
        '''opens an arbitrary revision of the file without
        opening a new filelog'''
        return filectx(self._repo, self._path, fileid=fileid,
                       filelog=self._filelog)

    def filerev(self):
        return self._filerev
    def filenode(self):
        return self._filenode
    def flags(self):
        return self._changectx.flags(self._path)
    def filelog(self):
        return self._filelog

    def rev(self):
        if '_changectx' in self.__dict__:
            return self._changectx.rev()
        if '_changeid' in self.__dict__:
            return self._changectx.rev()
        return self._filelog.linkrev(self._filerev)

    def linkrev(self):
        return self._filelog.linkrev(self._filerev)
    def node(self):
        return self._changectx.node()
    def hex(self):
        return hex(self.node())
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
    def manifest(self):
        return self._changectx.manifest()
    def changectx(self):
        return self._changectx

    def data(self):
        return self._filelog.read(self._filenode)
    def path(self):
        return self._path
    def size(self):
        return self._filelog.size(self._filerev)

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        if (fctx._filerev is None and self._repo._encodefilterpats
            or self.size() == fctx.size()):
            return self._filelog.cmp(self._filenode, fctx.data())

        return True

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

    def parents(self):
        p = self._path
        fl = self._filelog
        pl = [(p, n, fl) for n in self._filelog.parents(self._filenode)]

        r = self._filelog.renamed(self._filenode)
        if r:
            pl[0] = (r[0], r[1], None)

        return [filectx(self._repo, p, fileid=n, filelog=l)
                for p, n, l in pl if n != nullid]

    def children(self):
        # hard for renames
        c = self._filelog.children(self._filenode)
        return [filectx(self._repo, self._path, fileid=x,
                        filelog=self._filelog) for x in c]

    def annotate(self, follow=False, linenumber=None):
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
            for a1, a2, b1, b2 in bdiff.blocks(parent[1], child[1]):
                child[0][b1:b2] = parent[0][a1:a2]
            return child

        getlog = util.lrucachefunc(lambda x: self._repo.file(x))
        def getctx(path, fileid):
            log = path == self._path and self._filelog or getlog(path)
            return filectx(self._repo, path, fileid=fileid, filelog=log)
        getctx = util.lrucachefunc(getctx)

        def parents(f):
            # we want to reuse filectx objects as much as possible
            p = f._path
            if f._filerev is None: # working dir
                pl = [(n.path(), n.filerev()) for n in f.parents()]
            else:
                pl = [(p, n) for n in f._filelog.parentrevs(f._filerev)]

            if follow:
                r = f.renamed()
                if r:
                    pl[0] = (r[0], getlog(r[0]).rev(r[1]))

            return [getctx(p, n) for p, n in pl if n != nullrev]

        # use linkrev to find the first changeset where self appeared
        if self.rev() != self.linkrev():
            base = self.filectx(self.filerev())
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
            if f not in pcache:
                pcache[f] = parents(f)

            ready = True
            pl = pcache[f]
            for p in pl:
                if p not in hist:
                    ready = False
                    visit.append(p)
                    needed[p] = needed.get(p, 0) + 1
            if ready:
                visit.pop()
                curr = decorate(f.data(), f)
                for p in pl:
                    curr = pair(hist[p], curr)
                    if needed[p] == 1:
                        del hist[p]
                    else:
                        needed[p] -= 1

                hist[f] = curr
                pcache[f] = []

        return zip(hist[base][0], hist[base][1].splitlines(True))

    def ancestor(self, fc2, actx=None):
        """
        find the common ancestor file context, if any, of self, and fc2

        If actx is given, it must be the changectx of the common ancestor
        of self's and fc2's respective changesets.
        """

        if actx is None:
            actx = self.changectx().ancestor(fc2.changectx())

        # the trivial case: changesets are unrelated, files must be too
        if not actx:
            return None

        # the easy case: no (relevant) renames
        if fc2.path() == self.path() and self.path() in actx:
            return actx[self.path()]
        acache = {}

        # prime the ancestor cache for the working directory
        for c in (self, fc2):
            if c._filerev is None:
                pl = [(n.path(), n.filenode()) for n in c.parents()]
                acache[(c._path, None)] = pl

        flcache = {self._repopath:self._filelog, fc2._repopath:fc2._filelog}
        def parents(vertex):
            if vertex in acache:
                return acache[vertex]
            f, n = vertex
            if f not in flcache:
                flcache[f] = self._repo.file(f)
            fl = flcache[f]
            pl = [(f, p) for p in fl.parents(n) if p != nullid]
            re = fl.renamed(n)
            if re:
                pl.append(re)
            acache[vertex] = pl
            return pl

        a, b = (self._path, self._filenode), (fc2._path, fc2._filenode)
        v = ancestor.ancestor(a, b, parents)
        if v:
            f, n = v
            return filectx(self._repo, f, fileid=n, filelog=flcache[f])

        return None

    def ancestors(self):
        visit = {}
        c = self
        while True:
            for parent in c.parents():
                visit[(parent.rev(), parent.node())] = parent
            if not visit:
                break
            c = visit.pop(max(visit))
            yield c

class workingctx(changectx):
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
        self._repo = repo
        self._rev = None
        self._node = None
        self._text = text
        if date:
            self._date = util.parsedate(date)
        if user:
            self._user = user
        if changes:
            self._status = list(changes[:4])
            self._unknown = changes[4]
            self._ignored = changes[5]
            self._clean = changes[6]
        else:
            self._unknown = None
            self._ignored = None
            self._clean = None

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

    def __repr__(self):
        return "<workingctx %s>" % str(self)

    def __nonzero__(self):
        return True

    def __contains__(self, key):
        return self._repo.dirstate[key] not in "?r"

    @propertycache
    def _manifest(self):
        """generate a manifest corresponding to the working directory"""

        if self._unknown is None:
            self.status(unknown=True)

        man = self._parents[0].manifest().copy()
        copied = self._repo.dirstate.copies()
        if len(self._parents) > 1:
            man2 = self.p2().manifest()
            def getman(f):
                if f in man:
                    return man
                return man2
        else:
            getman = lambda f: man
        def cf(f):
            f = copied.get(f, f)
            return getman(f).flags(f)
        ff = self._repo.dirstate.flagfunc(cf)
        modified, added, removed, deleted = self._status
        unknown = self._unknown
        for i, l in (("a", added), ("m", modified), ("u", unknown)):
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
        return self._repo.status()[:4]

    @propertycache
    def _user(self):
        return self._repo.ui.username()

    @propertycache
    def _date(self):
        return util.makedate()

    @propertycache
    def _parents(self):
        p = self._repo.dirstate.parents()
        if p[1] == nullid:
            p = p[:-1]
        self._parents = [changectx(self._repo, x) for x in p]
        return self._parents

    def status(self, ignored=False, clean=False, unknown=False):
        """Explicit status query
        Unless this method is used to query the working copy status, the
        _status property will implicitly read the status using its default
        arguments."""
        stat = self._repo.status(ignored=ignored, clean=clean, unknown=unknown)
        self._unknown = self._ignored = self._clean = None
        if unknown:
            self._unknown = stat[4]
        if ignored:
            self._ignored = stat[5]
        if clean:
            self._clean = stat[6]
        self._status = stat[:4]
        return stat

    def manifest(self):
        return self._manifest
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
        assert self._unknown is not None  # must call status first
        return self._unknown
    def ignored(self):
        assert self._ignored is not None  # must call status first
        return self._ignored
    def clean(self):
        assert self._clean is not None  # must call status first
        return self._clean
    def branch(self):
        return encoding.tolocal(self._extra['branch'])
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

    def children(self):
        return []

    def flags(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest.flags(path)
            except KeyError:
                return ''

        orig = self._repo.dirstate.copies().get(path, path)

        def findflag(ctx):
            mnode = ctx.changeset()[0]
            node, flag = self._repo.manifest.find(mnode, orig)
            ff = self._repo.dirstate.flagfunc(lambda x: flag or '')
            try:
                return ff(path)
            except OSError:
                pass

        flag = findflag(self._parents[0])
        if flag is None and len(self.parents()) > 1:
            flag = findflag(self._parents[1])
        if flag is None or self._repo.dirstate[path] == 'r':
            return ''
        return flag

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return workingfilectx(self._repo, path, workingctx=self,
                              filelog=filelog)

    def ancestor(self, c2):
        """return the ancestor context of self and c2"""
        return self._parents[0].ancestor(c2) # punt on two parents for now

    def walk(self, match):
        return sorted(self._repo.dirstate.walk(match, self.substate.keys(),
                                               True, False))

    def dirty(self, missing=False):
        "check whether a working directory is modified"
        # check subrepos first
        for s in self.substate:
            if self.sub(s).dirty():
                return True
        # check current working dir
        return (self.p2() or self.branch() != self.p1().branch() or
                self.modified() or self.added() or self.removed() or
                (missing and self.deleted()))

    def add(self, list, prefix=""):
        join = lambda f: os.path.join(prefix, f)
        wlock = self._repo.wlock()
        ui, ds = self._repo.ui, self._repo.dirstate
        try:
            rejected = []
            for f in list:
                p = self._repo.wjoin(f)
                try:
                    st = os.lstat(p)
                except:
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
                    rejected.append(p)
                elif ds[f] in 'amn':
                    ui.warn(_("%s already tracked!\n") % join(f))
                elif ds[f] == 'r':
                    ds.normallookup(f)
                else:
                    ds.add(f)
            return rejected
        finally:
            wlock.release()

    def forget(self, list):
        wlock = self._repo.wlock()
        try:
            for f in list:
                if self._repo.dirstate[f] != 'a':
                    self._repo.ui.warn(_("%s not added!\n") % f)
                else:
                    self._repo.dirstate.forget(f)
        finally:
            wlock.release()

    def ancestors(self):
        for a in self._repo.changelog.ancestors(
            *[p.rev() for p in self._parents]):
            yield changectx(self._repo, a)

    def remove(self, list, unlink=False):
        if unlink:
            for f in list:
                try:
                    util.unlinkpath(self._repo.wjoin(f))
                except OSError, inst:
                    if inst.errno != errno.ENOENT:
                        raise
        wlock = self._repo.wlock()
        try:
            for f in list:
                if unlink and os.path.lexists(self._repo.wjoin(f)):
                    self._repo.ui.warn(_("%s still exists!\n") % f)
                elif self._repo.dirstate[f] == 'a':
                    self._repo.dirstate.forget(f)
                elif f not in self._repo.dirstate:
                    self._repo.ui.warn(_("%s not tracked!\n") % f)
                else:
                    self._repo.dirstate.remove(f)
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
        p = self._repo.wjoin(dest)
        if not os.path.lexists(p):
            self._repo.ui.warn(_("%s does not exist!\n") % dest)
        elif not (os.path.isfile(p) or os.path.islink(p)):
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

class workingfilectx(filectx):
    """A workingfilectx object makes access to data related to a particular
       file in the working directory convenient."""
    def __init__(self, repo, path, filelog=None, workingctx=None):
        """changeid can be a changeset revision, node, or tag.
           fileid can be a file revision or node."""
        self._repo = repo
        self._path = path
        self._changeid = None
        self._filerev = self._filenode = None

        if filelog:
            self._filelog = filelog
        if workingctx:
            self._changectx = workingctx

    @propertycache
    def _changectx(self):
        return workingctx(self._repo)

    def __nonzero__(self):
        return True

    def __str__(self):
        return "%s@%s" % (self.path(), self._changectx)

    def __repr__(self):
        return "<workingfilectx %s>" % str(self)

    def data(self):
        return self._repo.wread(self._path)
    def renamed(self):
        rp = self._repo.dirstate.copied(self._path)
        if not rp:
            return None
        return rp, self._changectx._parents[0]._manifest.get(rp, nullid)

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

    def size(self):
        return os.lstat(self._repo.wjoin(self._path)).st_size
    def date(self):
        t, tz = self._changectx.date()
        try:
            return (int(os.lstat(self._repo.wjoin(self._path)).st_mtime), tz)
        except OSError, err:
            if err.errno != errno.ENOENT:
                raise
            return (t, tz)

    def cmp(self, fctx):
        """compare with other file context

        returns True if different than fctx.
        """
        # fctx should be a filectx (not a wfctx)
        # invert comparison to reuse the same code path
        return fctx.cmp(self)

class memctx(object):
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
                 date=None, extra=None):
        self._repo = repo
        self._rev = None
        self._node = None
        self._text = text
        self._date = date and util.parsedate(date) or util.makedate()
        self._user = user
        parents = [(p or nullid) for p in parents]
        p1, p2 = parents
        self._parents = [changectx(self._repo, p) for p in (p1, p2)]
        files = sorted(set(files))
        self._status = [files, [], [], [], []]
        self._filectxfn = filectxfn

        self._extra = extra and extra.copy() or {}
        if 'branch' not in self._extra:
            self._extra['branch'] = 'default'
        elif self._extra.get('branch') == '':
            self._extra['branch'] = 'default'

    def __str__(self):
        return str(self._parents[0]) + "+"

    def __int__(self):
        return self._rev

    def __nonzero__(self):
        return True

    def __getitem__(self, key):
        return self.filectx(key)

    def p1(self):
        return self._parents[0]
    def p2(self):
        return self._parents[1]

    def user(self):
        return self._user or self._repo.ui.username()
    def date(self):
        return self._date
    def description(self):
        return self._text
    def files(self):
        return self.modified()
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
    def extra(self):
        return self._extra
    def flags(self, f):
        return self[f].flags()

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return self._filectxfn(self._repo, self, path)

    def commit(self):
        """commit context to the repo"""
        return self._repo.commitctx(self)

class memfilectx(object):
    """memfilectx represents an in-memory file to commit.

    See memctx for more details.
    """
    def __init__(self, path, data, islink=False, isexec=False, copied=None):
        """
        path is the normalized file path relative to repository root.
        data is the file content as a string.
        islink is True if the file is a symbolic link.
        isexec is True if the file is executable.
        copied is the source file path if current file was copied in the
        revision being committed, or None."""
        self._path = path
        self._data = data
        self._flags = (islink and 'l' or '') + (isexec and 'x' or '')
        self._copied = None
        if copied:
            self._copied = (copied, nullid)

    def __nonzero__(self):
        return True
    def __str__(self):
        return "%s@%s" % (self.path(), self._changectx)
    def path(self):
        return self._path
    def data(self):
        return self._data
    def flags(self):
        return self._flags
    def isexec(self):
        return 'x' in self._flags
    def islink(self):
        return 'l' in self._flags
    def renamed(self):
        return self._copied
