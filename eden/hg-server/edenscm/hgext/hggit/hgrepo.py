# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import localrepo, pycompat, util as hgutil
from edenscm.mercurial.node import bin

from . import util
from .git_handler import GitHandler
from .gitrepo import gitrepo


def generate_repo_subclass(baseclass):
    class hgrepo(baseclass):
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

            for tag, rev in pycompat.iteritems(self.githandler.tags):
                if isinstance(tag, pycompat.unicode):
                    tag = tag.encode("utf-8")
                tags[tag] = bin(rev)
                tagtypes[tag] = "git"
            for tag, rev in pycompat.iteritems(self.githandler.remote_refs):
                if isinstance(tag, pycompat.unicode):
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
