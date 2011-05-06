"""reorder a revlog (the manifest by default) to save space

Specifically, this topologically sorts the revisions in the revlog so that
revisions on the same branch are adjacent as much as possible. This is a
workaround for the fact that Mercurial computes deltas relative to the
previous revision rather than relative to a parent revision.

This is *not* safe to run on a changelog.
"""

# Originally written by Benoit Boissinot <benoit.boissinot at ens-lyon.org>
# as a patch to rewrite-log. Cleaned up, refactored, documented, and
# renamed by Greg Ward <greg at gerg.ca>.

# XXX would be nice to have a way to verify the repository after shrinking,
# e.g. by comparing "before" and "after" states of random changesets
# (maybe: export before, shrink, export after, diff).

import os, tempfile, errno
from mercurial import revlog, transaction, node, util, scmutil
from mercurial import changegroup
from mercurial.i18n import _


def postorder(start, edges):
    result = []
    visit = list(start)
    finished = set()

    while visit:
        cur = visit[-1]
        for p in edges[cur]:
            # defend against node.nullrev because it's occasionally
            # possible for a node to have parents (null, something)
            # rather than (something, null)
            if p not in finished and p != node.nullrev:
                visit.append(p)
                break
        else:
            result.append(cur)
            finished.add(cur)
            visit.pop()

    return result

def toposort_reversepostorder(ui, rl):
    # postorder of the reverse directed graph

    # map rev to list of parent revs (p2 first)
    parents = {}
    heads = set()
    ui.status(_('reading revs\n'))
    try:
        for rev in rl:
            ui.progress(_('reading'), rev, total=len(rl))
            (p1, p2) = rl.parentrevs(rev)
            if p1 == p2 == node.nullrev:
                parents[rev] = ()       # root node
            elif p1 == p2 or p2 == node.nullrev:
                parents[rev] = (p1,)    # normal node
            else:
                parents[rev] = (p2, p1) # merge node
            heads.add(rev)
            for p in parents[rev]:
                heads.discard(p)
    finally:
        ui.progress(_('reading'), None)

    heads = list(heads)
    heads.sort(reverse=True)

    ui.status(_('sorting revs\n'))
    return postorder(heads, parents)

def toposort_postorderreverse(ui, rl):
    # reverse-postorder of the reverse directed graph

    children = {}
    roots = set()
    ui.status(_('reading revs\n'))
    try:
        for rev in rl:
            ui.progress(_('reading'), rev, total=len(rl))
            (p1, p2) = rl.parentrevs(rev)
            if p1 == p2 == node.nullrev:
                roots.add(rev)
            children[rev] = []
            if p1 != node.nullrev:
                children[p1].append(rev)
            if p2 != node.nullrev:
                children[p2].append(rev)
    finally:
        ui.progress(_('reading'), None)

    roots = list(roots)
    roots.sort()

    ui.status(_('sorting revs\n'))
    result = postorder(roots, children)
    result.reverse()
    return result

def writerevs(ui, r1, r2, order, tr):

    ui.status(_('writing revs\n'))


    order = [r1.node(r) for r in order]

    # this is a bit ugly, but it works
    count = [0]
    def lookup(revl, x):
        count[0] += 1
        ui.progress(_('writing'), count[0], total=len(order))
        return "%020d" % revl.linkrev(revl.rev(x))

    unlookup = lambda x: int(x, 10)

    try:
        bundler = changegroup.bundle10(lookup)
        group = util.chunkbuffer(r1.group(order, bundler))
        group = changegroup.unbundle10(group, "UN")
        r2.addgroup(group, unlookup, tr)
    finally:
        ui.progress(_('writing'), None)

def report(ui, r1, r2):
    def getsize(r):
        s = 0
        for fn in (r.indexfile, r.datafile):
            try:
                s += os.stat(fn).st_size
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise
        return s

    oldsize = float(getsize(r1))
    newsize = float(getsize(r2))

    # argh: have to pass an int to %d, because a float >= 2^32
    # blows up under Python 2.5 or earlier
    ui.write(_('old file size: %12d bytes (%6.1f MiB)\n')
             % (int(oldsize), oldsize / 1024 / 1024))
    ui.write(_('new file size: %12d bytes (%6.1f MiB)\n')
             % (int(newsize), newsize / 1024 / 1024))

    shrink_percent = (oldsize - newsize) / oldsize * 100
    shrink_factor = oldsize / newsize
    ui.write(_('shrinkage: %.1f%% (%.1fx)\n')
             % (shrink_percent, shrink_factor))

def shrink(ui, repo, **opts):
    """shrink a revlog by reordering revisions

    Rewrites all the entries in some revlog of the current repository
    (by default, the manifest log) to save space.

    Different sort algorithms have different performance
    characteristics.  Use ``--sort`` to select a sort algorithm so you
    can determine which works best for your data.
    """

    if not repo.local():
        raise util.Abort(_('not a local repository: %s') % repo.root)

    fn = opts.get('revlog')
    if not fn:
        indexfn = repo.sjoin('00manifest.i')
    else:
        if not fn.endswith('.i'):
            raise util.Abort(_('--revlog option must specify the revlog index '
                               'file (*.i), not %s') % opts.get('revlog'))

        indexfn = os.path.realpath(fn)
        store = repo.sjoin('')
        if not indexfn.startswith(store):
            raise util.Abort(_('--revlog option must specify a revlog in %s, '
                               'not %s') % (store, indexfn))

    sortname = opts['sort']
    try:
        toposort = globals()['toposort_' + sortname]
    except KeyError:
        raise util.Abort(_('no such toposort algorithm: %s') % sortname)

    if not os.path.exists(indexfn):
        raise util.Abort(_('no such file: %s') % indexfn)
    if '00changelog' in indexfn:
        raise util.Abort(_('shrinking the changelog '
                           'will corrupt your repository'))

    ui.write(_('shrinking %s\n') % indexfn)
    prefix = os.path.basename(indexfn)[:-1]
    tmpindexfn = util.mktempcopy(indexfn, emptyok=True)

    r1 = revlog.revlog(scmutil.opener(os.getcwd(), audit=False), indexfn)
    r2 = revlog.revlog(scmutil.opener(os.getcwd(), audit=False), tmpindexfn)

    datafn, tmpdatafn = r1.datafile, r2.datafile

    oldindexfn = indexfn + '.old'
    olddatafn = datafn + '.old'
    if os.path.exists(oldindexfn) or os.path.exists(olddatafn):
        raise util.Abort(_('one or both of\n'
                           '  %s\n'
                           '  %s\n'
                           'exists from a previous run; please clean up '
                           'before running again') % (oldindexfn, olddatafn))

    # Don't use repo.transaction(), because then things get hairy with
    # paths: some need to be relative to .hg, and some need to be
    # absolute. Doing it this way keeps things simple: everything is an
    # absolute path.
    lock = repo.lock(wait=False)
    tr = transaction.transaction(ui.warn,
                                 open,
                                 repo.sjoin('journal'))

    def ignoremissing(func):
        def f(*args, **kw):
            try:
                return func(*args, **kw)
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise
        return f

    try:
        try:
            order = toposort(ui, r1)

            suboptimal = 0
            for i in xrange(1, len(order)):
                parents = [p for p in r1.parentrevs(order[i])
                           if p != node.nullrev]
                if parents and order[i - 1] not in parents:
                    suboptimal += 1
            ui.note(_('%d suboptimal nodes\n') % suboptimal)

            writerevs(ui, r1, r2, order, tr)
            report(ui, r1, r2)
            tr.close()
        except:
            # Abort transaction first, so we truncate the files before
            # deleting them.
            tr.abort()
            for fn in (tmpindexfn, tmpdatafn):
                ignoremissing(os.unlink)(fn)
            raise
        if not opts.get('dry_run'):
            # racy, both files cannot be renamed atomically
            # copy files
            util.oslink(indexfn, oldindexfn)
            ignoremissing(util.oslink)(datafn, olddatafn)

            # rename
            util.rename(tmpindexfn, indexfn)
            try:
                os.chmod(tmpdatafn, os.stat(datafn).st_mode)
                util.rename(tmpdatafn, datafn)
            except OSError, inst:
                if inst.errno != errno.ENOENT:
                    raise
                ignoremissing(os.unlink)(datafn)
        else:
            for fn in (tmpindexfn, tmpdatafn):
                ignoremissing(os.unlink)(fn)
    finally:
        lock.release()

    if not opts.get('dry_run'):
        ui.write(_('note: old revlog saved in:\n'
                   '  %s\n'
                   '  %s\n'
                   '(You can delete those files when you are satisfied that your\n'
                   'repository is still sane.  '
                   'Running \'hg verify\' is strongly recommended.)\n')
                 % (oldindexfn, olddatafn))

cmdtable = {
    'shrink': (shrink,
               [('', 'revlog', '', _('index (.i) file of the revlog to shrink')),
                ('n', 'dry-run', None, _('do not shrink, simulate only')),
                ('', 'sort', 'reversepostorder', 'name of sort algorithm to use'),
                ],
               _('hg shrink [--revlog PATH]'))
}

if __name__ == "__main__":
    print "shrink-revlog.py is now an extension (see hg help extensions)"
