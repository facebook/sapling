# context.py - changeset and file context objects for mercurial
#
# Copyright 2006, 2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import nullid, nullrev, short
from i18n import _
import ancestor, bdiff, revlog, util, os, errno

class changectx(object):
    """A changecontext object makes access to data related to a particular
    changeset convenient."""
    def __init__(self, repo, changeid=''):
        """changeid is a revision number, node, or tag"""
        if changeid == '':
            changeid = '.'
        self._repo = repo
        self._node = self._repo.lookup(changeid)
        self._rev = self._repo.changelog.rev(self._node)

    def __str__(self):
        return short(self.node())

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

    def __getattr__(self, name):
        if name == '_changeset':
            self._changeset = self._repo.changelog.read(self.node())
            return self._changeset
        elif name == '_manifest':
            self._manifest = self._repo.manifest.read(self._changeset[0])
            return self._manifest
        elif name == '_manifestdelta':
            md = self._repo.manifest.readdelta(self._changeset[0])
            self._manifestdelta = md
            return self._manifestdelta
        elif name == '_parents':
            p = self._repo.changelog.parents(self._node)
            if p[1] == nullid:
                p = p[:-1]
            self._parents = [changectx(self._repo, x) for x in p]
            return self._parents
        else:
            raise AttributeError, name

    def __contains__(self, key):
        return key in self._manifest

    def __getitem__(self, key):
        return self.filectx(key)

    def __iter__(self):
        a = self._manifest.keys()
        a.sort()
        for f in a:
            yield f

    def changeset(self): return self._changeset
    def manifest(self): return self._manifest

    def rev(self): return self._rev
    def node(self): return self._node
    def user(self): return self._changeset[1]
    def date(self): return self._changeset[2]
    def files(self): return self._changeset[3]
    def description(self): return self._changeset[4]
    def branch(self): return self._changeset[5].get("branch")
    def extra(self): return self._changeset[5]
    def tags(self): return self._repo.nodetags(self._node)

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def children(self):
        """return contexts for each child changeset"""
        c = self._repo.changelog.children(self._node)
        return [changectx(self._repo, x) for x in c]

    def _fileinfo(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest[path], self._manifest.flags(path)
            except KeyError:
                raise revlog.LookupError(self._node, path,
                                         _('not found in manifest'))
        if '_manifestdelta' in self.__dict__ or path in self.files():
            if path in self._manifestdelta:
                return self._manifestdelta[path], self._manifestdelta.flags(path)
        node, flag = self._repo.manifest.find(self._changeset[0], path)
        if not node:
            raise revlog.LookupError(self._node, path,
                                     _('not found in manifest'))

        return node, flag

    def filenode(self, path):
        return self._fileinfo(path)[0]

    def flags(self, path):
        try:
            return self._fileinfo(path)[1]
        except revlog.LookupError:
            return ''

    def filectx(self, path, fileid=None, filelog=None):
        """get a file context from this changeset"""
        if fileid is None:
            fileid = self.filenode(path)
        return filectx(self._repo, path, fileid=fileid,
                       changectx=self, filelog=filelog)

    def filectxs(self):
        """generate a file context for each file in this changeset's
           manifest"""
        mf = self.manifest()
        m = mf.keys()
        m.sort()
        for f in m:
            yield self.filectx(f, fileid=mf[f])

    def ancestor(self, c2):
        """
        return the ancestor context of self and c2
        """
        n = self._repo.changelog.ancestor(self._node, c2._node)
        return changectx(self._repo, n)

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
                or changectx is not None)

        if filelog:
            self._filelog = filelog

        if changeid is not None:
            self._changeid = changeid
        if changectx is not None:
            self._changectx = changectx
        if fileid is not None:
            self._fileid = fileid

    def __getattr__(self, name):
        if name == '_changectx':
            self._changectx = changectx(self._repo, self._changeid)
            return self._changectx
        elif name == '_filelog':
            self._filelog = self._repo.file(self._path)
            return self._filelog
        elif name == '_changeid':
            if '_changectx' in self.__dict__:
                self._changeid = self._changectx.rev()
            else:
                self._changeid = self._filelog.linkrev(self._filenode)
            return self._changeid
        elif name == '_filenode':
            if '_fileid' in self.__dict__:
                self._filenode = self._filelog.lookup(self._fileid)
            else:
                self._filenode = self._changectx.filenode(self._path)
            return self._filenode
        elif name == '_filerev':
            self._filerev = self._filelog.rev(self._filenode)
            return self._filerev
        elif name == '_repopath':
            self._repopath = self._path
            return self._repopath
        else:
            raise AttributeError, name

    def __nonzero__(self):
        try:
            n = self._filenode
            return True
        except revlog.LookupError:
            # file is missing
            return False

    def __str__(self):
        return "%s@%s" % (self.path(), short(self.node()))

    def __repr__(self):
        return "<filectx %s>" % str(self)

    def __hash__(self):
        try:
            return hash((self._path, self._fileid))
        except AttributeError:
            return id(self)

    def __eq__(self, other):
        try:
            return (self._path == other._path
                    and self._fileid == other._fileid)
        except AttributeError:
            return False

    def __ne__(self, other):
        return not (self == other)

    def filectx(self, fileid):
        '''opens an arbitrary revision of the file without
        opening a new filelog'''
        return filectx(self._repo, self._path, fileid=fileid,
                       filelog=self._filelog)

    def filerev(self): return self._filerev
    def filenode(self): return self._filenode
    def flags(self): return self._changectx.flags(self._path)
    def isexec(self): return 'x' in self.flags()
    def islink(self): return 'l' in self.flags()
    def filelog(self): return self._filelog

    def rev(self):
        if '_changectx' in self.__dict__:
            return self._changectx.rev()
        if '_changeid' in self.__dict__:
            return self._changectx.rev()
        return self._filelog.linkrev(self._filenode)

    def linkrev(self): return self._filelog.linkrev(self._filenode)
    def node(self): return self._changectx.node()
    def user(self): return self._changectx.user()
    def date(self): return self._changectx.date()
    def files(self): return self._changectx.files()
    def description(self): return self._changectx.description()
    def branch(self): return self._changectx.branch()
    def manifest(self): return self._changectx.manifest()
    def changectx(self): return self._changectx

    def data(self): return self._filelog.read(self._filenode)
    def path(self): return self._path
    def size(self): return self._filelog.size(self._filerev)

    def cmp(self, text): return self._filelog.cmp(self._filenode, text)

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
            except revlog.LookupError:
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
                for p,n,l in pl if n != nullid]

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

        getlog = util.cachefunc(lambda x: self._repo.file(x))
        def getctx(path, fileid):
            log = path == self._path and self._filelog or getlog(path)
            return filectx(self._repo, path, fileid=fileid, filelog=log)
        getctx = util.cachefunc(getctx)

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

        # find all ancestors
        needed = {base: 1}
        visit = [base]
        files = [base._path]
        while visit:
            f = visit.pop(0)
            for p in parents(f):
                if p not in needed:
                    needed[p] = 1
                    visit.append(p)
                    if p._path not in files:
                        files.append(p._path)
                else:
                    # count how many times we'll use this
                    needed[p] += 1

        # sort by revision (per file) which is a topological order
        visit = []
        for f in files:
            fn = [(n.rev(), n) for n in needed.keys() if n._path == f]
            visit.extend(fn)
        visit.sort()
        hist = {}

        for r, f in visit:
            curr = decorate(f.data(), f)
            for p in parents(f):
                if p != nullid:
                    curr = pair(hist[p], curr)
                    # trim the history of unneeded revs
                    needed[p] -= 1
                    if not needed[p]:
                        del hist[p]
            hist[f] = curr

        return zip(hist[f][0], hist[f][1].splitlines(1))

    def ancestor(self, fc2):
        """
        find the common ancestor file context, if any, of self, and fc2
        """

        acache = {}

        # prime the ancestor cache for the working directory
        for c in (self, fc2):
            if c._filerev == None:
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

class workingctx(changectx):
    """A workingctx object makes access to data related to
    the current working directory convenient.
    parents - a pair of parent nodeids, or None to use the dirstate.
    date - any valid date string or (unixtime, offset), or None.
    user - username string, or None.
    extra - a dictionary of extra values, or None.
    changes - a list of file lists as returned by localrepo.status()
               or None to use the repository status.
    """
    def __init__(self, repo, parents=None, text="", user=None, date=None,
                 extra=None, changes=None):
        self._repo = repo
        self._rev = None
        self._node = None
        self._text = text
        if date:
            self._date = util.parsedate(date)
        else:
            self._date = util.makedate()
        if user:
            self._user = user
        else:
            self._user = self._repo.ui.username()
        if parents:
            p1, p2 = parents
            self._parents = [self._repo.changectx(p) for p in (p1, p2)]
        if changes:
            self._status = list(changes)

        self._extra = {}
        if extra:
            self._extra = extra.copy()
        if 'branch' not in self._extra:
            branch = self._repo.dirstate.branch()
            try:
                branch = branch.decode('UTF-8').encode('UTF-8')
            except UnicodeDecodeError:
                raise util.Abort(_('branch name not in UTF-8!'))
            self._extra['branch'] = branch
        if self._extra['branch'] == '':
            self._extra['branch'] = 'default'

    def __str__(self):
        return str(self._parents[0]) + "+"

    def __nonzero__(self):
        return True

    def __getattr__(self, name):
        if name == '_status':
            self._status = self._repo.status()
            return self._status
        if name == '_manifest':
            self._buildmanifest()
            return self._manifest
        elif name == '_parents':
            p = self._repo.dirstate.parents()
            if p[1] == nullid:
                p = p[:-1]
            self._parents = [changectx(self._repo, x) for x in p]
            return self._parents
        else:
            raise AttributeError, name

    def _buildmanifest(self):
        """generate a manifest corresponding to the working directory"""

        man = self._parents[0].manifest().copy()
        copied = self._repo.dirstate.copies()
        cf = lambda x: man.flags(copied.get(x, x))
        ff = self._repo.dirstate.flagfunc(cf)
        modified, added, removed, deleted, unknown = self._status[:5]
        for i, l in (("a", added), ("m", modified), ("u", unknown)):
            for f in l:
                man[f] = man.get(copied.get(f, f), nullid) + i
                try:
                    man.set(f, ff(f))
                except OSError:
                    pass

        for f in deleted + removed:
            if f in man:
                del man[f]

        self._manifest = man

    def manifest(self): return self._manifest

    def user(self): return self._user
    def date(self): return self._date
    def description(self): return self._text
    def files(self):
        f = self.modified() + self.added() + self.removed()
        f.sort()
        return f

    def modified(self): return self._status[0]
    def added(self): return self._status[1]
    def removed(self): return self._status[2]
    def deleted(self): return self._status[3]
    def unknown(self): return self._status[4]
    def clean(self): return self._status[5]
    def branch(self): return self._extra['branch']
    def extra(self): return self._extra

    def tags(self):
        t = []
        [t.extend(p.tags()) for p in self.parents()]
        return t

    def children(self):
        return []

    def flags(self, path):
        if '_manifest' in self.__dict__:
            try:
                return self._manifest.flags(path)
            except KeyError:
                return ''

        pnode = self._parents[0].changeset()[0]
        orig = self._repo.dirstate.copies().get(path, path)
        node, flag = self._repo.manifest.find(pnode, orig)
        try:
            ff = self._repo.dirstate.flagfunc(lambda x: flag or '')
            return ff(path)
        except OSError:
            pass

        if not node or path in self.deleted() or path in self.removed():
            return ''
        return flag

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return workingfilectx(self._repo, path, workingctx=self,
                              filelog=filelog)

    def ancestor(self, c2):
        """return the ancestor context of self and c2"""
        return self._parents[0].ancestor(c2) # punt on two parents for now

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

    def __getattr__(self, name):
        if name == '_changectx':
            self._changectx = workingctx(self._repo)
            return self._changectx
        elif name == '_repopath':
            self._repopath = (self._repo.dirstate.copied(self._path)
                              or self._path)
            return self._repopath
        elif name == '_filelog':
            self._filelog = self._repo.file(self._repopath)
            return self._filelog
        else:
            raise AttributeError, name

    def __nonzero__(self):
        return True

    def __str__(self):
        return "%s@%s" % (self.path(), self._changectx)

    def filectx(self, fileid):
        '''opens an arbitrary revision of the file without
        opening a new filelog'''
        return filectx(self._repo, self._repopath, fileid=fileid,
                       filelog=self._filelog)

    def rev(self):
        if '_changectx' in self.__dict__:
            return self._changectx.rev()
        return self._filelog.linkrev(self._filenode)

    def data(self): return self._repo.wread(self._path)
    def renamed(self):
        rp = self._repopath
        if rp == self._path:
            return None
        return rp, self._changectx._parents[0]._manifest.get(rp, nullid)

    def parents(self):
        '''return parent filectxs, following copies if necessary'''
        p = self._path
        rp = self._repopath
        pcl = self._changectx._parents
        fl = self._filelog
        pl = [(rp, pcl[0]._manifest.get(rp, nullid), fl)]
        if len(pcl) > 1:
            if rp != p:
                fl = None
            pl.append((p, pcl[1]._manifest.get(p, nullid), fl))

        return [filectx(self._repo, p, fileid=n, filelog=l)
                for p,n,l in pl if n != nullid]

    def children(self):
        return []

    def size(self): return os.stat(self._repo.wjoin(self._path)).st_size
    def date(self):
        t, tz = self._changectx.date()
        try:
            return (int(os.lstat(self._repo.wjoin(self._path)).st_mtime), tz)
        except OSError, err:
            if err.errno != errno.ENOENT: raise
            return (t, tz)

    def cmp(self, text): return self._repo.wread(self._path) == text

class memctx(object):
    """A memctx is a subset of changectx supposed to be built on memory
    and passed to commit functions.

    NOTE: this interface and the related memfilectx are experimental and
    may change without notice.

    parents - a pair of parent nodeids.
    filectxfn - a callable taking (repo, memctx, path) arguments and
    returning a memctx object.
    date - any valid date string or (unixtime, offset), or None.
    user - username string, or None.
    extra - a dictionary of extra values, or None.
    """
    def __init__(self, repo, parents, text, files, filectxfn, user=None,
                 date=None, extra=None):
        self._repo = repo
        self._rev = None
        self._node = None
        self._text = text
        self._date = date and util.parsedate(date) or util.makedate()
        self._user = user or self._repo.ui.username()
        parents = [(p or nullid) for p in parents]
        p1, p2 = parents
        self._parents = [self._repo.changectx(p) for p in (p1, p2)]
        files = list(files)
        files.sort()
        self._status = [files, [], [], [], []]
        self._filectxfn = filectxfn

        self._extra = extra and extra.copy() or {}
        if 'branch' not in self._extra:
            self._extra['branch'] = 'default'
        elif self._extra.get('branch') == '':
            self._extra['branch'] = 'default'

    def __str__(self):
        return str(self._parents[0]) + "+"

    def __nonzero__(self):
        return True

    def user(self): return self._user
    def date(self): return self._date
    def description(self): return self._text
    def files(self): return self.modified()
    def modified(self): return self._status[0]
    def added(self): return self._status[1]
    def removed(self): return self._status[2]
    def deleted(self): return self._status[3]
    def unknown(self): return self._status[4]
    def clean(self): return self._status[5]
    def branch(self): return self._extra['branch']
    def extra(self): return self._extra
    def flags(self, f): return self[f].flags()

    def parents(self):
        """return contexts for each parent changeset"""
        return self._parents

    def filectx(self, path, filelog=None):
        """get a file context from the working directory"""
        return self._filectxfn(self._repo, self, path)

class memfilectx(object):
    """A memfilectx is a subset of filectx supposed to be built by client
    code and passed to commit functions.
    """
    def __init__(self, path, data, islink, isexec, copied):
        """copied is the source file path, or None."""
        self._path = path
        self._data = data
        self._flags = (islink and 'l' or '') + (isexec and 'x' or '')
        self._copied = None
        if copied:
            self._copied = (copied, nullid)

    def __nonzero__(self): return True
    def __str__(self): return "%s@%s" % (self.path(), self._changectx)
    def path(self): return self._path
    def data(self): return self._data
    def flags(self): return self._flags
    def isexec(self): return 'x' in self._flags
    def islink(self): return 'l' in self._flags
    def renamed(self): return self._copied

