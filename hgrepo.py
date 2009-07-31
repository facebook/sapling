import weakref

from mercurial import localrepo, lock, node
from mercurial import changelog, dirstate, filelog, manifest, context
from mercurial.node import bin, hex, nullid, nullrev, short

from git_handler import GitHandler
from gitrepo import gitrepo


def generate_repo_subclass(baseclass):
    class hgrepo(baseclass):
        def pull(self, remote, heads=None, force=False):
            if isinstance(remote, gitrepo):
                git = GitHandler(self, self.ui)
                git.fetch(remote.path)
            else: #pragma: no cover
                return super(hgrepo, self).pull(remote, heads, force)

        def push(self, remote, force=False, revs=None):
            if isinstance(remote, gitrepo):
                git = GitHandler(self, self.ui)
                git.push(remote.path, revs, force)
            else: #pragma: no cover
                return super(hgrepo, self).push(remote, force, revs)

        def findoutgoing(self, remote, base=None, heads=None, force=False):
            if isinstance(remote, gitrepo):
                git = GitHandler(self, self.ui)
                base, heads = git.get_refs(remote.path)
                out, h = super(hgrepo, self).findoutgoing(remote, base, heads, force)
                return out
            else: #pragma: no cover
                return super(hgrepo, self).findoutgoing(remote, base, heads, force)

        def tags(self):
            if self.tagscache:
                return self.tagscache

            git = GitHandler(self, self.ui)
            tagscache = super(hgrepo, self).tags()
            tagscache.update(dict([(tag, bin(rev)) for (tag,rev) in git.tags.iteritems()]))
            tagstypes = dict([(tag, 'git') for tag in git.tags])
            self._tagstypecache.update(tagstypes)
            return tagscache

    return hgrepo
