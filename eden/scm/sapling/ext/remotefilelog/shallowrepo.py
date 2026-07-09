# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# shallowrepo.py - shallow repository that uses remote filelogs


import bindings
from sapling import match as matchmod, progress, util
from sapling.i18n import _

from . import fileserverclient, remotefilectx, remotefilelog

requirement = "remotefilelog"


def _prefetch_matcher(repo, rev, matcher=None):
    sparse_matcher = repo.maybesparsematch(rev)
    if matcher is None:
        return sparse_matcher or matchmod.always(repo.root, repo.getcwd())
    if sparse_matcher is None:
        return matcher
    return matchmod.intersectmatchers(matcher, sparse_matcher)


def wraprepo(repo) -> None:
    class shallowrepository(repo.__class__):
        @util.propertycache
        def name(self):
            return self.ui.config("remotefilelog", "reponame", "unknown")

        @util.propertycache
        def fallbackpath(self):
            path = self.ui.config(
                "remotefilelog",
                "fallbackpath",
                # fallbackrepo is the old, deprecated name
                self.ui.config(
                    "remotefilelog", "fallbackrepo", self.ui.config("paths", "default")
                ),
            )
            if not path:
                # raise AttributeError insteal of error.Abort, so `getattr(repo, "fallbackpath", None)`
                # will not break
                raise AttributeError("fallbackpath")

            return path

        @util.propertycache
        def fileslog(self):
            return remotefilelog.remotefileslog(self)

        def maybesparsematch(self, *revs, **kwargs):
            """
            A wrapper that allows the remotefilelog to invoke sparsematch() if
            this is a sparse repository, or returns None if this is not a
            sparse repository.
            """
            if hasattr(self, "sparsematch"):
                return self.sparsematch(*revs, **kwargs)

            return None

        def file(self, f):
            if f[0] == "/":
                f = f[1:]

            return remotefilelog.remotefilelog(self.svfs, f, self)

        def filectx(self, path, changeid=None, fileid=None):
            return remotefilectx.remotefilectx(self, path, changeid, fileid)

        def close(self):
            result = super(shallowrepository, self).close()
            if "fileslog" in self.__dict__:
                self.fileslog.abortpending()
            return result

        def commitpending(self):
            super(shallowrepository, self).commitpending()

        def backgroundprefetch(self, revs, base=None, pats=None, opts=None):
            """Runs prefetch in background"""
            cmd = [util.hgexecutable(), "-R", self.origroot, "prefetch"]
            if revs:
                cmd += ["-r", revs]
            if base:
                cmd += ["-b", base]

            util.spawndetached(cmd)

        def prefetch(self, revs, base=None, matcher=None):
            """Prefetches all the necessary file revisions for the given revs"""
            with self._lock(
                self.svfs,
                "prefetchlock",
                True,
                None,
                None,
                _("prefetching in %s") % self.origroot,
            ):
                self._prefetch(revs, base, matcher)

        def _prefetch(self, revs, base=None, matcher=None):
            base_manifest = self[base].manifest() if base is not None else None

            with progress.bar(self.ui, _("prefetching"), total=len(revs)) as prog:
                for rev in sorted(revs):
                    ctx = self[rev]
                    current_matcher = _prefetch_matcher(self, rev, matcher)
                    with progress.spinner(self.ui, _("ensuring files fetched")):
                        stats = bindings.filewalk.walkandcache(
                            self._rsrepo,
                            [ctx.manifest()],
                            current_matcher,
                            self.ui._rcfg,
                            base_manifest,
                        )
                        if self.ui.configbool(
                            "experimental", "print-prefetch-count", False
                        ):
                            count = stats["local"] + stats["remote"]
                            self.ui.status(
                                _("prefetch: rev %s has %d files\n") % (rev, count)
                            )
                    prog.value += 1

    repo.__class__ = shallowrepository

    repo.fileservice = fileserverclient.fileserverclient(repo)
