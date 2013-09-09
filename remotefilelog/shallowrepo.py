# shallowrepo.py - shallow repository that uses remote filelogs
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.node import hex, nullid, bin
from mercurial.i18n import _
from mercurial import localrepo, context, mdiff, util
from mercurial.extensions import wrapfunction
import remotefilelog, remotefilectx, fileserverclient, os

def wraprepo(repo):
    class shallowrepository(repo.__class__):
        def file(self, f):
            if f[0] == '/':
                f = f[1:]
            return remotefilelog.remotefilelog(self.sopener, f, self)

        def filectx(self, path, changeid=None, fileid=None):
            """changeid can be a changeset revision, node, or tag.
               fileid can be a file revision or node."""
            return remotefilectx.remotefilectx(self, path, changeid, fileid)

        def addchangegroupfiles(self, source, revmap, trp, pr, needfiles):
            files = 0
            visited = set()
            revisiondatas = {}
            queue = []

            # read all the file chunks but don't add them
            while True:
                chunkdata = source.filelogheader()
                if not chunkdata:
                    break
                f = chunkdata["filename"]
                self.ui.debug("adding %s revisions\n" % f)
                pr()
                chain = None
                while True:
                    revisiondata = source.deltachunk(chain)
                    if not revisiondata:
                        break

                    chain = revisiondata['node']

                    revisiondatas[(f, chain)] = revisiondata
                    queue.append((f, chain))

                    if f not in visited:
                        files += 1
                        visited.add(f)

                if chain == None:
                    raise util.Abort(_("received file revlog group is empty"))

            processed = set()
            def available(f, node, depf, depnode):
                if depnode != nullid and (depf, depnode) not in processed:
                    if not (depf, depnode) in revisiondatas:
                        # It's not in the changegroup, assume it's already
                        # in the repo
                        return True
                    # re-add self to queue
                    queue.insert(0, (f, node))
                    # add dependency in front
                    queue.insert(0, (depf, depnode))
                    return False
                return True

            skipcount = 0

            while queue:
                f, node = queue.pop(0)
                if (f, node) in processed:
                    continue

                skipcount += 1
                if skipcount > len(queue) + 1:
                    raise util.Abort(_("circular node dependency"))

                fl = self.file(f)

                revisiondata = revisiondatas[(f, node)]
                p1 = revisiondata['p1']
                p2 = revisiondata['p2']
                linknode = revisiondata['cs']
                deltabase = revisiondata['deltabase']
                delta = revisiondata['delta']

                if not available(f, node, f, deltabase):
                    continue

                base = fl.revision(deltabase)
                text = mdiff.patch(base, delta)
                if isinstance(text, buffer):
                    text = str(text)

                meta, text = remotefilelog._parsemeta(text)
                if 'copy' in meta:
                    copyfrom = meta['copy']
                    copynode = bin(meta['copyrev'])
                    copyfl = self.file(copyfrom)
                    if not available(f, node, copyfrom, copynode):
                        continue

                for p in [p1, p2]:
                    if p != nullid:
                        if not available(f, node, f, p):
                            continue

                fl.add(text, meta, trp, linknode, p1, p2)
                processed.add((f, node))
                skipcount = 0

            self.ui.progress(_('files'), None)

            return len(revisiondatas), files

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

            fileserverclient.client.prefetch(repo, files)

        return (lookup, modified, added, removed, deleted, unknown, \
                ignored, clean)

    wrapfunction(repo.dirstate, 'status', status)

    repo.__class__ = shallowrepository

    localpath = os.path.join(repo.sopener.vfs.base, 'data')
    if not os.path.exists(localpath):
        os.makedirs(localpath)
