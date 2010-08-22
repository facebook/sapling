# Copyright 2010 Pradeepkumar Gayam <in3xes@gmail.com>
#
# Author(s):
# Pradeepkumar Gayam <in3xes@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import util, changegroup, localrepo
import os

def compress(ui, repo, dest):
    dest = os.path.realpath(util.expandpath(dest))
    target = localrepo.instance(ui, dest, create=1)
    tr = target.transaction("compress")
    src_cl = repo.changelog
    tar_cl = target.changelog
    changedfiles = set()
    mmfs = {}
    collect = changegroup.collector(src_cl, mmfs, changedfiles)
    total = len(repo)

    for r in src_cl:
        p = [src_cl.node(i) for i in src_cl.parentrevs(r)]
        nd = tar_cl.addrevision(src_cl.revision(src_cl.node(r)), tr,
                                 src_cl.linkrev(r), p[0], p[1])
        collect(nd)
        ui.progress(('adding changesets'), r, unit=('revisions'),
                    total=total)

    src_mnfst = repo.manifest
    tar_mnfst = target.manifest
    for r in src_mnfst:
        p = [src_mnfst.node(i) for i in src_mnfst.parentrevs(r)]
        tar_mnfst.addrevision(src_mnfst.revision(src_mnfst.node(r)), tr,
                               src_mnfst.linkrev(r), p[0], p[1])
        ui.progress(('adding manifest'), r, unit=('revisions'),
                    total=total)

    total = len(changedfiles)
    for cnt, f in enumerate(changedfiles):
        sf = repo.file(f)
        tf = target.file(f)
        for r in sf:
            p = [sf.node(i) for i in sf.parentrevs(r)]
            tf.addrevision(sf.revision(sf.node(r)), tr, sf.linkrev(r),
                               p[0], p[1])
        ui.progress(('adding files'), cnt, item=f, unit=('file'), total=total)

    tr.close()

cmdtable = {
    "compress" : (compress, [], "DEST")
    }
