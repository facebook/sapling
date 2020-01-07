# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-fixme[21]: Could not find `util`.
import util
from edenscm.mercurial import localrepo, util as hgutil
from edenscm.mercurial.node import bin

# pyre-fixme[21]: Could not find `git_handler`.
from git_handler import GitHandler

# pyre-fixme[21]: Could not find `gitrepo`.
from gitrepo import gitrepo


try:
    # pyre-fixme[18]: Global name `unicode` is undefined.
    unicode
except NameError:
    unicode = str


def generate_repo_subclass(baseclass):
    class hgrepo(baseclass):
        if hgutil.safehasattr(localrepo.localrepository, "pull"):
            # Mercurial < 3.2
            @util.transform_notgit
            def pull(self, remote, heads=None, force=False):
                if isinstance(remote, gitrepo):
                    return self.githandler.fetch(remote.path, heads)
                else:  # pragma: no cover
                    return super(hgrepo, self).pull(remote, heads, force)

        if hgutil.safehasattr(localrepo.localrepository, "push"):
            # Mercurial < 3.2
            @util.transform_notgit
            def push(self, remote, force=False, revs=None):
                if isinstance(remote, gitrepo):
                    return self.githandler.push(remote.path, revs, force)
                else:  # pragma: no cover
                    return super(hgrepo, self).push(remote, force, revs)

        @util.transform_notgit
        def findoutgoing(self, remote, base=None, heads=None, force=False):
            if isinstance(remote, gitrepo):
                base, heads = self.githandler.get_refs(remote.path)
                out, h = super(hgrepo, self).findoutgoing(remote, base, heads, force)
                return out
            else:  # pragma: no cover
                return super(hgrepo, self).findoutgoing(remote, base, heads, force)

        def _findtags(self):
            (tags, tagtypes) = super(hgrepo, self)._findtags()

            for tag, rev in self.githandler.tags.iteritems():
                if isinstance(tag, unicode):
                    tag = tag.encode("utf-8")
                tags[tag] = bin(rev)
                tagtypes[tag] = "git"
            for tag, rev in self.githandler.remote_refs.iteritems():
                if isinstance(tag, unicode):
                    tag = tag.encode("utf-8")
                tags[tag] = rev
                tagtypes[tag] = "git-remote"
            tags.update(self.githandler.remote_refs)
            return (tags, tagtypes)

        @hgutil.propertycache
        def githandler(self):
            """get the GitHandler for an hg repo

            This only makes sense if the repo talks to at least one git remote.
            """
            return GitHandler(self, self.ui)

        def tags(self):
            return {}

    return hgrepo
