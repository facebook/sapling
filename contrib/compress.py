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

def _copyrevlog(ui, src, dst, tr, progress=None):
    if progress:
        desc = 'adding %s' % progress
        total = len(src)
        def progress(count):
            ui.progress(desc, count, unit=('revisions'), total=total)
    else:
        progress = lambda x: None
    for r in src:
        p = [src.node(i) for i in src.parentrevs(r)]
        dst.addrevision(src.revision(src.node(r)), tr, src.linkrev(r),
                        p[0], p[1])
        progress(r)

def compress(ui, repo, dest):
    dest = hg.localpath(ui.expandpath(dest))
    target = localrepo.instance(ui, dest, create=True)

    tr = lock = tlock = None
    try:
        lock = repo.lock()
        tlock = target.lock()
        tr = target.transaction("compress")
        trp = weakref.proxy(tr)

        _copyrevlog(ui, repo.manifest, target.manifest, trp, 'manifest')

        # only keep indexes and filter "data/" prefix and ".i" suffix
        datafiles = [fn[5:-2] for fn, f2, size in repo.store.datafiles()
                                      if size and fn.endswith('.i')]
        total = len(datafiles)
        for cnt, f in enumerate(datafiles):
            _copyrevlog(ui, repo.file(f), target.file(f), trp)
            ui.progress(('adding files'), cnt, item=f, unit=('file'),
                        total=total)

        _copyrevlog(ui, repo.changelog, target.changelog, trp, 'changesets')

        tr.close()
    finally:
        if tr:
            tr.release()
        release(tlock, lock)

cmdtable = {
    "compress" : (compress, [], "DEST")
    }
