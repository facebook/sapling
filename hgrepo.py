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
                git.fetch(remote.path, heads)
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

        def _findtags(self):
            (tags, tagtypes) = super(hgrepo, self)._findtags()

            git = GitHandler(self, self.ui)
            for tag, rev in git.tags.iteritems():
                if tag in tags:
                    continue

                tags[tag] = bin(rev)
                tagtypes[tag] = 'git'

            return (tags, tagtypes)

        def tags(self):
            if not hasattr(self, 'tagscache'):
                # mercurial 1.4
                return super(hgrepo, self).tags()

            if self.tagscache:
                return self.tagscache

            git = GitHandler(self, self.ui)
            tagscache = super(hgrepo, self).tags()
            for tag, rev in git.tags.iteritems():
                if tag in tagscache:
                    continue

                tagscache[tag] = bin(rev)
                self._tagstypecache[tag] = 'git'

            return tagscache

    return hgrepo
