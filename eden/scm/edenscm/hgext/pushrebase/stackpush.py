# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# stackpush - specialized pushrebase
"""
push a stack of linear commits to the destination.

Typically a push looks like this:

  F onto bookmark (in critical section)
  .
  .
  E onto bookmark (outside critical section)
  .
  . D stack top
  | .
  | .
  | C
  | |
  | B stack bottom
  |/
  A stack parent

Pushrebase would need to check files changed in B::D are not touched in A::F.

stackpush tries to minimize steps inside the critical section:

  1. Avoid constructing a bundle repo in the critical section.
     Instead, collect all the data needed for *checking* and pushing B::D
     beforehand. That is, a {path: old_filenode} map for checking, and
     [(commit_metadata, {path: new_file})] for pushing.
  2. Only check F's manifest for the final decision for conflicts.
     Do not read E::F in the critical section.
"""

from __future__ import absolute_import

import time

from edenscm.mercurial import context, error, mutation
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid, nullrev

from .errors import ConflictsError, StackPushUnsupportedError


class pushcommit(object):
    def __init__(
        self, user, date, desc, extra, filechanges, examinepaths, orignode=None
    ):
        """constructor for pushcommit

        This class is designed to only include simple types (list, dict,
        strings), without coupling with Mercurial internals, for maximum
        portability.

        Do not add states that are not simple types (ex. repo, ui, or bundle).
        """
        self.user = user
        self.date = date
        self.desc = desc
        self.extra = extra
        self.filechanges = filechanges  # {path: (mode, content, copysource) | None}
        self.examinepaths = examinepaths  # {path}
        self.orignode = orignode

    @classmethod
    def fromctx(cls, ctx):
        filechanges = {}
        examinepaths = set(ctx.files())
        for path in ctx.files():
            try:
                fctx = ctx[path]
            except error.ManifestLookupError:
                filechanges[path] = None
            else:
                if fctx.rawflags():
                    raise StackPushUnsupportedError("stackpush does not support LFS")
                renamed = fctx.renamed()
                if renamed:
                    copysource = renamed[0]
                    examinepaths.add(copysource)
                else:
                    copysource = None
                filechanges[path] = (fctx.flags(), fctx.data(), copysource)
        return cls(
            ctx.user(),
            ctx.date(),
            ctx.description(),
            ctx.extra(),
            filechanges,
            examinepaths,
            orignode=ctx.node(),
        )


class pushrequest(object):
    def __init__(self, stackparentnode, pushcommits, fileconditions):
        """constructor for pushrequest

        This class is designed to only include simple types (list, dict,
        strings), without coupling with Mercurial internals, for maximum
        portability.

        Do not add states that are not simple types (ex. repo, ui, or bundle).
        """

        self.stackparentnode = stackparentnode
        self.pushcommits = pushcommits
        self.fileconditions = fileconditions  # {path: None | filenode}

    @classmethod
    def fromrevset(cls, repo, spec):
        """Construct a pushrequest from revset"""
        # No merge commits allowed.
        revs = list(repo.revs(spec))
        if repo.revs("%ld and merge()", revs):
            raise StackPushUnsupportedError("stackpush does not support merges")
        parentrevs = list(repo.revs("parents(%ld)-%ld", revs, revs))
        if len(parentrevs) > 1:
            raise StackPushUnsupportedError(
                "stackpush only supports single linear stack"
            )

        examinepaths = set()

        # calculate "pushcommit"s, and paths to examine
        pushcommits = []
        for rev in revs:
            ctx = repo[rev]
            commit = pushcommit.fromctx(ctx)
            examinepaths.update(commit.examinepaths)
            pushcommits.append(commit)

        parentctx = repo[(parentrevs + [nullrev])[0]]
        return cls(
            parentctx.node(),
            pushcommits,
            cls._calculatefileconditions(parentctx, examinepaths),
        )

    @classmethod
    def frommemcommit(cls, repo, commitparams):
        changelist = commitparams.changelist
        metadata = commitparams.metadata

        files = changelist.files
        filechanges = {}
        examinepaths = set(files.keys())

        for path, info in files.iteritems():
            if info.deleted:
                filechanges[path] = None
            else:
                copysource = info.copysource
                if copysource:
                    examinepaths.add(copysource)
                filechanges[path] = (info.flags, info.content, copysource)

        commit = pushcommit(
            metadata.author,
            None,
            metadata.description,
            metadata.extra,
            filechanges,
            examinepaths,
        )

        def resolveparentctx(repo, originalparent):
            if not originalparent:
                raise error.Abort(_("parent commit must be specified"))

            return repo[originalparent]

        p1 = resolveparentctx(repo, changelist.parent)
        return cls(p1.node(), [commit], cls._calculatefileconditions(p1, examinepaths))

    @staticmethod
    def _calculatefileconditions(parentctx, examinepaths):
        """calculate 'fileconditions' - filenodes in the signal parent commit
        """
        parentmanifest = parentctx.manifestctx()
        fileconditions = {}
        for path in examinepaths:
            try:
                filenodemode = parentmanifest.find(path)
            except KeyError:
                filenodemode = None
            fileconditions[path] = filenodemode

        return fileconditions

    def pushonto(self, ctx, getcommitdatefn=None):
        """Push the stack onto ctx

        getcommitdatefn is a functor:

        (ui, originalcommithash, originalcommitdate) -> replacementcommitdate

        to allow rewriting replacement commit time as a function of the original
        commit hash and time. Therefore, it is not required for creating new
        commits.

        Return (added, replacements)
        """
        self.check(ctx)
        return self._pushunchecked(ctx, getcommitdatefn=getcommitdatefn)

    def check(self, ctx):
        """Check if push onto ctx can be done

        Raise ConflictsError if there are conflicts.
        """
        mctx = ctx.manifestctx()
        conflicts = []
        for path, expected in self.fileconditions.iteritems():
            try:
                actual = mctx.find(path)
            except KeyError:
                actual = None
            if actual != expected:
                conflicts.append(path)
        if conflicts:
            raise ConflictsError(conflicts)

    def _pushunchecked(self, ctx, getcommitdatefn=None):
        added = []
        replacements = {}
        repo = ctx.repo()
        for commit in self.pushcommits:
            newnode = self._pushsingleunchecked(
                ctx, commit, getcommitdatefn=getcommitdatefn
            )
            added.append(newnode)
            orignode = commit.orignode
            if orignode:
                replacements[orignode] = newnode
            ctx = repo[newnode]
        return added, replacements

    @staticmethod
    def _pushsingleunchecked(ctx, commit, getcommitdatefn=None):
        """Return newly pushed node"""
        repo = ctx.repo()

        def getfilectx(repo, memctx, path):
            assert path in commit.filechanges
            entry = commit.filechanges[path]
            if entry is None:
                # deleted
                return None
            else:
                # changed or created
                mode, content, copysource = entry
                return context.memfilectx(
                    repo,
                    memctx,
                    path,
                    content,
                    islink=("l" in mode),
                    isexec=("x" in mode),
                    copied=copysource,
                )

        extra = commit.extra.copy()
        date = commit.date
        loginfo = {}

        orignode = commit.orignode
        if orignode:
            mutation.record(repo, extra, [orignode], "pushrebase")
            loginfo = {"predecessors": hex(orignode), "mutation": "pushrebase"}
            date = getcommitdatefn(repo.ui, hex(orignode), commit.date)

        return context.memctx(
            repo,
            [ctx.node(), nullid],
            commit.desc,
            sorted(commit.filechanges),
            getfilectx,
            commit.user,
            date,
            extra,
            loginfo=loginfo,
        ).commit()
