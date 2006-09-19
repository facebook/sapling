# context.py - changeset and file context objects for mercurial
#
# Copyright 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

from node import *
from demandload import demandload
demandload(globals(), "heapq")

class changectx(object):
    """A changecontext object makes access to data related to a particular
    changeset convenient."""
    def __init__(self, repo, changeid):
        """changeid is a revision number, node, or tag"""
        self._repo = repo

        self._node = self._repo.lookup(changeid)
        self._rev = self._repo.changelog.rev(self._node)

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

        assert changeid or fileid

        if filelog:
            self._filelog = filelog
        else:
            self._filelog = self._repo.file(self._path)

        if not fileid:
            # if given a changeset id, go ahead and look up the file
            self._changeid = changeid
            self._changectx = self.changectx()
            self._filenode = self._changectx.filenode(self._path)
        else:
            # else delay changectx creation
            self._filenode = self._filelog.lookup(fileid)
            self._changeid = self._filelog.linkrev(self._filenode)
        self._filerev = self._filelog.rev(self._filenode)

    def changectx(self):
        try:
            return self._changectx
        except AttributeError:
            self._changectx = changectx(self._repo, self._changeid)
            return self._changectx

    def filerev(self): return self._filerev
    def filenode(self): return self._filenode
    def filelog(self): return self._filelog

    def rev(self): return self.changectx().rev()
    def node(self): return self.changectx().node()
    def user(self): return self.changectx().user()
    def date(self): return self.changectx().date()
    def files(self): return self.changectx().files()
    def description(self): return self.changectx().description()
    def manifest(self): return self.changectx().manifest()

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
        return self._filelog.annotate(self._filenode)

    def ancestor(self, fc2):
        """
        find the common ancestor file context, if any, of self, and fc2
        """

        a, b = (self._path, self._filenode), (fc2._path, fc2._filenode)

        if a == b:
            return self

        if a[0] == b[0]:
            n = self._filelog.ancestor(a[1], b[1])
            if n != nullid:
                return filectx(self._repo, self._path,
                               fileid=n, filelog=self._filelog)

        # build a graph of all ancestors, crossing renames
        ag = {}
        fv = [a, b]
        flcache = {self._path:self._filelog, fc2._path:fc2._filelog}

        while fv:
            f,n = fv.pop()
            try:
                fl = flcache[f]
            except KeyError:
                flcache[f] = self._repo.file(f)
                fl = flcache[f]
            v = [n]
            while v:
                n = v.pop()
                c = (f, n)
                if c in ag:
                    continue

                pl = [ n for n in fl.parents(n) if n != nullid ]
                v += pl
                pl = [ (f, n) for n in pl ]
                re = fl.renamed(n)
                if re:
                    pl.append(re)
                    if re not in ag:
                        fv.append(re)
                ag[c] = pl

        dist = {}
        def depth(node):
            try:
                return dist[node]
            except KeyError:
                pl = ag[node]
                if not pl:
                    dist[node] = 0
                else:
                    dist[node] = max([depth(p) for p in pl]) + 1
                return dist[node]

        # traverse ancestors in order of decreasing distance from root
        def ancestors(vertex):
            h = [(-depth(vertex), vertex)]
            seen = {}
            while h:
                d, v = heapq.heappop(h)
                if v not in seen:
                    seen[v] = 1
                    yield (-d, v)
                    for p in ag[v]:
                        heapq.heappush(h, (-depth(p), p))

        def generations(vertex):
            sg, s = None, {}
            for g,v in ancestors(vertex):
                if g != sg:
                    if sg:
                        yield sg, s
                    sg, s = g, {v:1}
                else:
                    s[v] = 1
            yield sg, s

        x = generations(a)
        y = generations(b)
        gx = x.next()
        gy = y.next()

        # increment each ancestor list until it is closer to root than
        # the other, or they match
        try:
            while 1:
                if gx[0] == gy[0]:
                    # find the intersection
                    i = [ n for n in gx[1] if n in gy[1] ]
                    if i:
                        fp,fn = i[0]
                        fl = flcache[fp]
                        return filectx(self._repo, fp, fileid=fn, filelog=fl)
                    else:
                        gy = y.next()
                        gx = x.next()
                elif gx[0] < gy[0]:
                    gy = y.next()
                else:
                    gx = x.next()
        except StopIteration:
            return None
