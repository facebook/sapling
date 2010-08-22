# Copyright 2010 Pradeepkumar Gayam <in3xes@gmail.com>
#
# Author(s):
# Pradeepkumar Gayam <in3xes@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


from mercurial import hg, localrepo
from mercurial.lock import release
import weakref

def compress(ui, repo, dest):
    dest = hg.localpath(ui.expandpath(dest))
    target = localrepo.instance(ui, dest, create=True)

    tr = lock = tlock = None
    try:
        lock = repo.lock()
        tlock = target.lock()
        tr = target.transaction("compress")
        trp = weakref.proxy(tr)

        src_cl = repo.changelog
        tar_cl = target.changelog
        total = len(repo)

        for r in src_cl:
            p = [src_cl.node(i) for i in src_cl.parentrevs(r)]
            tar_cl.addrevision(src_cl.revision(src_cl.node(r)), trp,
                               src_cl.linkrev(r), p[0], p[1])
            ui.progress(('adding changesets'), r, unit=('revisions'),
                        total=total)

        src_mnfst = repo.manifest
        tar_mnfst = target.manifest
        for r in src_mnfst:
            p = [src_mnfst.node(i) for i in src_mnfst.parentrevs(r)]
            tar_mnfst.addrevision(src_mnfst.revision(src_mnfst.node(r)), trp,
                                   src_mnfst.linkrev(r), p[0], p[1])
            ui.progress(('adding manifest'), r, unit=('revisions'),
                        total=total)

        # only keep indexes and filter "data/" prefix and ".i" suffix
        datafiles = [fn[5:-2] for fn, f2, size in repo.store.datafiles()
                                      if size and fn.endswith('.i')]
        total = len(datafiles)
        for cnt, f in enumerate(datafiles):
            sf = repo.file(f)
            tf = target.file(f)
            for r in sf:
                p = [sf.node(i) for i in sf.parentrevs(r)]
                tf.addrevision(sf.revision(sf.node(r)), trp, sf.linkrev(r),
                               p[0], p[1])
            ui.progress(('adding files'), cnt, item=f, unit=('file'),
                        total=total)
        tr.close()
    finally:
        if tr:
            tr.release()
        release(tlock, lock)

cmdtable = {
    "compress" : (compress, [], "DEST")
    }
