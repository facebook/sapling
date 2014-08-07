# shallowrepo.py - shallow repository that uses remote filelogs
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.node import hex, nullid, nullrev, bin
from mercurial.i18n import _
from mercurial import localrepo, context, util, match, scmutil
from mercurial.extensions import wrapfunction
import remotefilelog, remotefilectx, fileserverclient, shallowbundle, os

requirement = "remotefilelog"

def wraprepo(repo):
    class shallowrepository(repo.__class__):
        @util.propertycache
        def name(self):
            return self.ui.config('remotefilelog', 'reponame', '')

        def file(self, f):
            if f[0] == '/':
                f = f[1:]

            if self.shallowmatch(f):
                return remotefilelog.remotefilelog(self.sopener, f, self)
            else:
                return super(shallowrepository, self).file(f)

        def filectx(self, path, changeid=None, fileid=None):
            if self.shallowmatch(path):
                return remotefilectx.remotefilectx(self, path, changeid, fileid)
            else:
                return super(shallowrepository, self).filectx(path, changeid, fileid)

        def pull(self, remote, *args, **kwargs):
            # Hook into the callstream/getbundle to insert bundle capabilities
            # during a pull.
            def remotecallstream(orig, command, **opts):
                if command == 'getbundle' and 'remotefilelog' in remote._capabilities():
                    bundlecaps = opts.get('bundlecaps')
                    if bundlecaps:
                        bundlecaps = [bundlecaps]
                    else:
                        bundlecaps = []
                    bundlecaps.append('remotefilelog')
                    if self.includepattern:
                        bundlecaps.append("includepattern=" + '\0'.join(self.includepattern))
                    if self.excludepattern:
                        bundlecaps.append("excludepattern=" + '\0'.join(self.excludepattern))
                    opts['bundlecaps'] = ','.join(bundlecaps)
                return orig(command, **opts)

            def localgetbundle(orig, source, heads=None, common=None, bundlecaps=None):
                if not bundlecaps:
                    bundlecaps = []
                bundlecaps.append('remotefilelog')
                return orig(source, heads=heads, common=common, bundlecaps=bundlecaps)

            if hasattr(remote, '_callstream'):
                wrapfunction(remote, '_callstream', remotecallstream)
            elif hasattr(remote, 'getbundle'):
                wrapfunction(remote, 'getbundle', localgetbundle)

            return super(shallowrepository, self).pull(remote, *args, **kwargs)

        def prefetch(self, revs, pats=None, opts=None):
            """Prefetches all the necessary file revisions for the given revs
            """
            files = set()
            visited = set()
            visited.add(nullrev)
            for rev in sorted(revs):
                ctx = repo[rev]
                if pats:
                    m = scmutil.match(ctx, pats, opts)

                mf = repo.manifest
                mfnode = ctx.manifestnode()
                mfrev = mf.rev(mfnode)

                # Decompressing manifests is expensive.
                # When possible, only read the deltas.
                p1, p2 = mf.parentrevs(mfrev)
                if p1 in visited and p2 in visited:
                    mfdict = mf.readfast(mfnode)
                else:
                    mfdict = mf.read(mfnode)

                for path, fnode in mfdict.iteritems():
                    if not pats or m(path):
                        files.add((path, hex(fnode)))

                visited.add(mfrev)

            repo.fileservice.prefetch(files)

    # Wrap dirstate.status here so we can prefetch all file nodes in
    # the lookup set before localrepo.status uses them.
    def status(orig, match, subrepos, ignored, clean, unknown):
        lookup, modified, added, removed, deleted, unknown, ignored, \
            clean = orig(match, subrepos, ignored, clean, unknown)

        if lookup:
            files = []
            parents = repo.parents()
            for fname in lookup:
                for ctx in parents:
                    if fname in ctx:
                        fnode = ctx.filenode(fname)
                        files.append((fname, hex(fnode)))

            repo.fileservice.prefetch(files)

        return (lookup, modified, added, removed, deleted, unknown, \
                ignored, clean)

    wrapfunction(repo.dirstate, 'status', status)

    repo.__class__ = shallowrepository

    repo.shallowmatch = match.always(repo.root, '')
    repo.fileservice = fileserverclient.fileserverclient(repo)

    repo.includepattern = repo.ui.configlist("remotefilelog", "includepattern", None)
    repo.excludepattern = repo.ui.configlist("remotefilelog", "excludepattern", None)
    if repo.includepattern or repo.excludepattern:
        repo.shallowmatch = match.match(repo.root, '', None,
            repo.includepattern, repo.excludepattern)

    localpath = os.path.join(repo.sopener.vfs.base, 'data')
    if not os.path.exists(localpath):
        os.makedirs(localpath)
