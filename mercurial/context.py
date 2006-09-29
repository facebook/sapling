# context.py - changeset and file context objects for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from demandload import demandload
demandload(globals(), "ancestor util")

class changectx(object):
    """A changecontext object makes access to data related to a particular
    changeset convenient."""
    def __init__(self, repo, changeid=None):
        """changeid is a revision number, node, or tag"""
        self._repo = repo

        if not changeid and changeid != 0:
            p1, p2 = self._repo.dirstate.parents()
            self._rev = self._repo.changelog.rev(p1)
            if self._rev == -1:
                changeid = 'tip'
            else:
                self._node = p1
                return

        self._node = self._repo.lookup(changeid)
        self._rev = self._repo.changelog.rev(self._node)

    def __str__(self):
        return short(self.node())

    def __repr__(self):
        return "<changectx %s>" % short(self.node())

    def __eq__(self, other):
        return self._rev == other._rev

    def __nonzero__(self):
        return self._rev != -1

    def changeset(self):
        try:
            return self._changeset
        except AttributeError:
            self._changeset = self._repo.changelog.read(self.node())
            return self._changeset

    def manifest(self):
        try:
            return self._manifest
        except AttributeError:
            self._manifest = self._repo.manifest.read(self.changeset()[0])
            return self._manifest

    def rev(self): return self._rev
    def node(self): return self._node
    def user(self): return self.changeset()[1]
    def date(self): return self.changeset()[2]
    def files(self): return self.changeset()[3]
    def description(self): return self.changeset()[4]

    def parents(self):
        """return contexts for each parent changeset"""
        p = self._repo.changelog.parents(self._node)
        return [ changectx(self._repo, x) for x in p ]

    def children(self):
        """return contexts for each child changeset"""
        c = self._repo.changelog.children(self._node)
        return [ changectx(self._repo, x) for x in c ]

    def filenode(self, path):
        node, flag = self._repo.manifest.find(self.changeset()[0], path)
        return node

    def filectx(self, path, fileid=None):
        """get a file context from this changeset"""
        if fileid is None:
            fileid = self.filenode(path)
        return filectx(self._repo, path, fileid=fileid)

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
    def __init__(self, repo, path, changeid=None, fileid=None, filelog=None):
        """changeid can be a changeset revision, node, or tag.
           fileid can be a file revision or node."""
        self._repo = repo
        self._path = path

        assert changeid is not None or fileid is not None

        if filelog:
            self._filelog = filelog
        else:
            self._filelog = self._repo.file(self._path)

        if fileid is None:
            self._changeid = changeid
        else:
            self._filenode = self._filelog.lookup(fileid)
            self._changeid = self._filelog.linkrev(self._filenode)

    def __getattr__(self, name):
        if name == '_changectx':
            self._changectx = changectx(self._repo, self._changeid)
            return self._changectx
        elif name == '_filenode':
            self._filenode = self._changectx.filenode(self._path)
            return self._filenode
        elif name == '_filerev':
            self._filerev = self._filelog.rev(self._filenode)
            return self._filerev
        else:
            raise AttributeError, name

    def __nonzero__(self):
        return self._filerev != nullid

    def __str__(self):
        return "%s@%s" % (self.path(), short(self.node()))

    def __repr__(self):
        return "<filectx %s@%s>" % (self.path(), short(self.node()))

    def __eq__(self, other):
        return self._path == other._path and self._changeid == other._changeid

    def filerev(self): return self._filerev
    def filenode(self): return self._filenode
    def filelog(self): return self._filelog

    def rev(self):
        if hasattr(self, "_changectx"):
            return self._changectx.rev()
        return self._filelog.linkrev(self._filenode)

    def node(self): return self._changectx.node()
    def user(self): return self._changectx.user()
    def date(self): return self._changectx.date()
    def files(self): return self._changectx.files()
    def description(self): return self._changectx.description()
    def manifest(self): return self._changectx.manifest()
    def changectx(self): return self._changectx

    def data(self): return self._filelog.read(self._filenode)
    def renamed(self): return self._filelog.renamed(self._filenode)
    def path(self): return self._path

    def parents(self):
        p = self._path
        fl = self._filelog
        pl = [ (p, n, fl) for n in self._filelog.parents(self._filenode) ]

        r = self.renamed()
        if r:
            pl[0] = (r[0], r[1], None)

        return [ filectx(self._repo, p, fileid=n, filelog=l)
                 for p,n,l in pl if n != nullid ]

    def children(self):
        # hard for renames
        c = self._filelog.children(self._filenode)
        return [ filectx(self._repo, self._path, fileid=x,
                         filelog=self._filelog) for x in c ]

    def annotate(self):
        getctx = util.cachefunc(lambda x: filectx(self._repo, self._path,
                                                  changeid=x,
                                                  filelog=self._filelog))
        hist = self._filelog.annotate(self._filenode)

        return [(getctx(rev), line) for rev, line in hist]

    def ancestor(self, fc2):
        """
        find the common ancestor file context, if any, of self, and fc2
        """

        acache = {}
        flcache = {self._path:self._filelog, fc2._path:fc2._filelog}
        def parents(vertex):
            if vertex in acache:
                return acache[vertex]
            f, n = vertex
            if f not in flcache:
                flcache[f] = self._repo.file(f)
            fl = flcache[f]
            pl = [ (f,p) for p in fl.parents(n) if p != nullid ]
            re = fl.renamed(n)
            if re:
                pl.append(re)
            acache[vertex]=pl
            return pl

        a, b = (self._path, self._filenode), (fc2._path, fc2._filenode)
        v = ancestor.ancestor(a, b, parents)
        if v:
            f,n = v
            return filectx(self._repo, f, fileid=n, filelog=flcache[f])

        return None
