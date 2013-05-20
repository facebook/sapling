# shallowrepo.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial.node import hex
from mercurial.i18n import _
from mercurial import localrepo, context, mdiff
import remotefilelog
import remotefilectx

def wraprepo(repo):    
    class shallowrepository(repo.__class__):
        def file(self, f):
            if f[0] == '/':
                f = f[1:]
            return remotefilelog.remotefilelog(self.sopener, f)

        def filectx(self, path, changeid=None, fileid=None):
            """changeid can be a changeset revision, node, or tag.
               fileid can be a file revision or node."""
            return remotefilectx.remotefilectx(self, path, changeid, fileid)

        def addchangegroupfiles(self, source, revmap, trp, pr, needfiles):
            files = 0
            visited = set()
            revisiondatas = []

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

                    revisiondatas.append((f, revisiondata))

                    if f not in visited:
                        files += 1
                        visited.add(f)

                if chain == None:
                    raise util.Abort(_("received file revlog group is empty"))

            # sort the revisions by linkrev
            revisiondatas = sorted(revisiondatas, key=lambda x: revmap(x[1]['cs']))

            # add the file chunks in sorted order, since some may
            # require their parents to exist first
            for f, revisiondata in revisiondatas:
                fl = self.file(f)

                node = revisiondata['node']
                p1 = revisiondata['p1']
                p2 = revisiondata['p2']
                linknode = revisiondata['cs']
                deltabase = revisiondata['deltabase']
                delta = revisiondata['delta']

                base = fl.revision(deltabase)
                text = mdiff.patch(base, delta)
                if isinstance(text, buffer):
                    text = str(text)

                meta, text = remotefilelog._parsemeta(text)
                fl.add(text, meta, trp, linknode, p1, p2)

            self.ui.progress(_('files'), None)

            return len(revisiondatas), files

    repo.__class__ = shallowrepository
