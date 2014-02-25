# overlay classes for repositories
# unifies access to unimported git objects and committed hg objects
# designed to support incoming
#
# incomplete, implemented on demand

from mercurial import ancestor
from mercurial import manifest
from mercurial import context
from mercurial.node import bin, hex, nullid
from mercurial import localrepo

def _maybehex(n):
    if len(n) == 20:
        return hex(n)
    return n

class overlaymanifest(object):
    def __init__(self, repo, sha):
        self.repo = repo
        self.tree = repo.handler.git.get_object(sha)
        self._map = None
        self._flagmap = None

    def withflags(self):
        self.load()
        return set([path for path, flag in self._flagmap.iteritems()
                    if flag & 020100])

    def copy(self):
        return overlaymanifest(self.repo, self.tree.id)

    def keys(self):
        self.load()
        return self._map.keys()

    def iterkeys(self):
        return iter(self.keys())

    def flags(self, path):
        self.load()

        def hgflag(gitflag):
            if gitflag & 0100:
                return 'x'
            elif gitflag & 020000:
                return 'l'
            else:
                return ''

        return hgflag(self._flagmap[path])

    def load(self):
        if self._map is not None:
            return

        self._map = {}
        self._flagmap = {}

        def addtree(tree, dirname):
            for entry in tree.iteritems():
                if entry.mode & 040000:
                    # expand directory
                    subtree = self.repo.handler.git.get_object(entry.sha)
                    addtree(subtree, dirname + entry.path + '/')
                else:
                    path = dirname + entry.path
                    self._map[path] = bin(entry.sha)
                    self._flagmap[path] = entry.mode

        addtree(self.tree, '')

    def iteritems(self):
        self.load()
        return self._map.iteritems()

    def __iter__(self):
        self.load()
        return self._map.__iter__()

    def __getitem__(self, path):
        self.load()
        return self._map[path]

    def __delitem__(self, path):
        del self._map[path]

class overlayfilectx(object):
    def __init__(self, repo, path, fileid=None):
        self.repo = repo
        self._path = path
        self.fileid = fileid

    # this is a hack to skip copy detection
    def ancestors(self):
        return [self, self]

    def filenode(self):
        return nullid

    def rev(self):
        return -1

    def path(self):
        return self._path

    def filelog(self):
        return self.fileid

    def data(self):
        blob = self.repo.handler.git.get_object(_maybehex(self.fileid))
        return blob.data

class overlaychangectx(context.changectx):
    def __init__(self, repo, sha):
        self.repo = repo
        if not isinstance(sha, basestring):
          sha = sha.hex()
        self.commit = repo.handler.git.get_object(_maybehex(sha))
        self._overlay = getattr(repo, 'gitoverlay', repo)
        self._rev = self._overlay.rev(bin(self.commit.id))

    def node(self):
        return bin(self.commit.id)

    def rev(self):
        return self._rev

    def date(self):
        return self.commit.author_time, self.commit.author_timezone

    def branch(self):
        return 'default'

    def user(self):
        return self.commit.author

    def files(self):
        return []

    def extra(self):
        return {}

    def description(self):
        return self.commit.message

    def parents(self):
        return [overlaychangectx(self.repo, sha) for sha in self.commit.parents]

    def manifestnode(self):
        return bin(self.commit.tree)

    def hex(self):
        return self.commit.id

    def tags(self):
        return []

    def bookmarks(self):
        return []

    def manifest(self):
        return overlaymanifest(self._overlay, self.commit.tree)

    def filectx(self, path, filelog=None):
        mf = self.manifest()
        return overlayfilectx(self._overlay, path, mf[path])

    def flags(self, path):
        mf = self.manifest()
        return mf.flags(path)

    def __nonzero__(self):
        return True

    def phase(self):
        try:
            from mercurial import phases
            return phases.draft
        except ImportError:
            return 1

class overlayrevlog(object):
    def __init__(self, repo, base):
        self.repo = repo
        self.base = base

    def parents(self, n):
        gitrev = self.repo.revmap.get(n)
        if gitrev is None:
            # we've reached a revision we have
            return self.base.parents(n)
        commit = self.repo.handler.git.get_object(_maybehex(n))

        if not commit.parents:
            return [nullid, nullid]

        def gitorhg(n):
            hn = self.repo.handler.map_hg_get(hex(n))
            if hn is not None:
                return bin(hn)
            return n

        # currently ignores the octopus
        p1 = gitorhg(bin(commit.parents[0]))
        if len(commit.parents) > 1:
            p2 = gitorhg(bin(commit.parents[1]))
        else:
            p2 = nullid

        return [p1, p2]

    def ancestor(self, a, b):
        anode = self.repo.nodemap.get(a)
        bnode = self.repo.nodemap.get(b)
        if anode is None and bnode is None:
          return self.base.ancestor(a, b)
        ancs = ancestor.ancestors(self.parentrevs, a, b)
        if ancs:
          return min(map(self.node, ancs))
        return nullid

    def parentrevs(self, rev):
        return [self.rev(p) for p in self.parents(self.node(rev))]

    def node(self, rev):
        gitnode = self.repo.nodemap.get(rev)
        if gitnode is None:
            return self.base.node(rev)
        return gitnode

    def rev(self, n):
        gitrev = self.repo.revmap.get(n)
        if gitrev is None:
             return self.base.rev(n)
        return gitrev

    def __len__(self):
        return len(self.repo.handler.repo) + len(self.repo.revmap)

class overlaymanifestlog(overlayrevlog):
    def read(self, sha):
        if sha == nullid:
            return manifest.manifestdict()
        return overlaymanifest(self.repo, sha)

class overlaychangelog(overlayrevlog):
    def read(self, sha):
        if isinstance(sha, int):
            sha = self.node(sha)
        if sha == nullid:
            return (nullid, "", (0, 0), [], "", {})
        return overlaychangectx(self.repo, sha)


class overlayrepo(object):
    def __init__(self, handler, commits, refs):
        self.handler = handler

        self.changelog = overlaychangelog(self, handler.repo.changelog)
        self.manifest = overlaymanifestlog(self, handler.repo.manifest)

        # for incoming -p
        self.root = handler.repo.root
        self.getcwd = handler.repo.getcwd
        # self.status = handler.repo.status
        self.ui = handler.repo.ui

        self.revmap = None
        self.nodemap = None
        self.refmap = None
        self.tagmap = None

        self._makemaps(commits, refs)

    def __getitem__(self, n):
        if n not in self.revmap:
            return self.handler.repo[n]
        return overlaychangectx(self, n)

    def _handlerhack(self, method, *args, **kwargs):
        nothing = object()
        r = self.handler.repo
        oldhandler = getattr(r, 'handler', nothing)
        oldoverlay = getattr(r, 'gitoverlay', nothing)
        r.handler = self.handler
        r.gitoverlay = self
        try:
          return getattr(r, method)(*args, **kwargs)
        finally:
          if oldhandler is nothing:
            del r.handler
          else:
            r.handler = oldhandler
          if oldoverlay is nothing:
            del r.gitoverlay
          else:
            r.gitoverlay = oldoverlay

    def status(self, *args, **kwargs):
      return self._handlerhack('status', *args, **kwargs)

    def node(self, n):
        """Returns an Hg or Git hash for the specified Git hash"""
        if bin(n) in self.revmap:
            return n
        return self.handler.map_hg_get(n)

    def nodebookmarks(self, n):
        return self.refmap.get(n, [])

    def nodetags(self, n):
        return self.tagmap.get(n, [])

    def rev(self, n):
        return self.revmap[n]

    def filectx(self, path, fileid=None):
        return overlayfilectx(self, path, fileid=fileid)

    def _makemaps(self, commits, refs):
        baserev = self.handler.repo['tip'].rev()
        self.revmap = {}
        self.nodemap = {}
        for i, n in enumerate(commits):
            rev = baserev + i + 1
            self.revmap[n] = rev
            self.nodemap[rev] = n

        self.refmap = {}
        self.tagmap = {}
        for ref in refs:
            if ref.startswith('refs/heads/'):
                refname = ref[11:]
                self.refmap.setdefault(bin(refs[ref]), []).append(refname)
            elif ref.startswith('refs/tags/'):
                tagname = ref[10:]
                self.tagmap.setdefault(bin(refs[ref]), []).append(tagname)
