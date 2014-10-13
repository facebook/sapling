import os

from mercurial.node import bin
from mercurial import util as hgutil

from git_handler import GitHandler
from gitrepo import gitrepo
import util

def generate_repo_subclass(baseclass):
    class hgrepo(baseclass):
        @util.transform_notgit
        def pull(self, remote, heads=None, force=False):  # Mercurial < 3.2
            if isinstance(remote, gitrepo):
                return self.githandler.fetch(remote.path, heads)
            else: #pragma: no cover
                return super(hgrepo, self).pull(remote, heads, force)

        # TODO figure out something useful to do with the newbranch param
        @util.transform_notgit
        def push(self, remote, force=False, revs=None, newbranch=False):
            if isinstance(remote, gitrepo):
                return self.githandler.push(remote.path, revs, force)
            else: #pragma: no cover
                return super(hgrepo, self).push(remote, force, revs, newbranch)

        @util.transform_notgit
        def findoutgoing(self, remote, base=None, heads=None, force=False):
            if isinstance(remote, gitrepo):
                base, heads = self.githandler.get_refs(remote.path)
                out, h = super(hgrepo, self).findoutgoing(remote, base, heads, force)
                return out
            else: #pragma: no cover
                return super(hgrepo, self).findoutgoing(remote, base, heads, force)

        def _findtags(self):
            (tags, tagtypes) = super(hgrepo, self)._findtags()

            for tag, rev in self.githandler.tags.iteritems():
                tags[tag] = bin(rev)
                tagtypes[tag] = 'git'

            tags.update(self.githandler.remote_refs)
            return (tags, tagtypes)

        @hgutil.propertycache
        def githandler(self):
            '''get the GitHandler for an hg repo

            This only makes sense if the repo talks to at least one git remote.
            '''
            return GitHandler(self, self.ui)

        def tags(self):
            # TODO consider using self._tagscache
            tagscache = super(hgrepo, self).tags()
            tagscache.update(self.githandler.remote_refs)
            for tag, rev in self.githandler.tags.iteritems():
                if tag in tagscache:
                    continue

                tagscache[tag] = bin(rev)

            return tagscache

    return hgrepo
