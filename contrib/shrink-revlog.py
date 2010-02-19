#!/usr/bin/env python

"""\
reorder a revlog (the manifest by default) to save space

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

import sys, os, tempfile
import optparse
from mercurial import ui as ui_, hg, revlog, transaction, node, util
from mercurial import changegroup
from mercurial.i18n import _

def toposort(ui, rl):

    children = {}
    root = []
    # build children and roots
    ui.status(_('reading revs\n'))
    try:
        for i in rl:
            ui.progress(_('reading'), i, total=len(rl))
            children[i] = []
            parents = [p for p in rl.parentrevs(i) if p != node.nullrev]
            # in case of duplicate parents
            if len(parents) == 2 and parents[0] == parents[1]:
                del parents[1]
            for p in parents:
                assert p in children
                children[p].append(i)

            if len(parents) == 0:
                root.append(i)
    finally:
        ui.progress(_('reading'), None, total=len(rl))

    # XXX this is a reimplementation of the 'branchsort' topo sort
    # algorithm in hgext.convert.convcmd... would be nice not to duplicate
    # the algorithm
    ui.status(_('sorting revs\n'))
    visit = root
    ret = []
    while visit:
        i = visit.pop(0)
        ret.append(i)
        if i not in children:
            # This only happens if some node's p1 == p2, which can
            # happen in the manifest in certain circumstances.
            continue
        next = []
        for c in children.pop(i):
            parents_unseen = [p for p in rl.parentrevs(c)
                              if p != node.nullrev and p in children]
            if len(parents_unseen) == 0:
                next.append(c)
        visit = next + visit
    return ret

def writerevs(ui, r1, r2, order, tr):

    ui.status(_('writing revs\n'))

    count = [0]
    def progress(*args):
        ui.progress(_('writing'), count[0], total=len(order))
        count[0] += 1

    order = [r1.node(r) for r in order]

    # this is a bit ugly, but it works
    lookup = lambda x: "%020d" % r1.linkrev(r1.rev(x))
    unlookup = lambda x: int(x, 10)

    try:
        group = util.chunkbuffer(r1.group(order, lookup, progress))
        chunkiter = changegroup.chunkiter(group)
        r2.addgroup(chunkiter, unlookup, tr)
    finally:
        ui.progress(_('writing'), None, len(order))

def report(ui, olddatafn, newdatafn):
    oldsize = float(os.stat(olddatafn).st_size)
    newsize = float(os.stat(newdatafn).st_size)

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
    """
    Shrink revlog by re-ordering revisions. Will operate on manifest for
    the given repository if no other revlog is specified."""

    # Unbuffer stdout for nice progress output.
    sys.stdout = os.fdopen(sys.stdout.fileno(), 'w', 0)

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

    datafn = indexfn[:-2] + '.d'
    if not os.path.exists(indexfn):
        raise util.Abort(_('no such file: %s') % indexfn)
    if '00changelog' in indexfn:
        raise util.Abort(_('shrinking the changelog '
                           'will corrupt your repository'))
    if not os.path.exists(datafn):
        # This is just a lazy shortcut because I can't be bothered to
        # handle all the special cases that entail from no .d file.
        raise util.Abort(_('%s does not exist: revlog not big enough '
                           'to be worth shrinking') % datafn)

    oldindexfn = indexfn + '.old'
    olddatafn = datafn + '.old'
    if os.path.exists(oldindexfn) or os.path.exists(olddatafn):
        raise util.Abort(_('one or both of\n'
                           '  %s\n'
                           '  %s\n'
                           'exists from a previous run; please clean up '
                           'before running again') % (oldindexfn, olddatafn))

    ui.write(_('shrinking %s\n') % indexfn)
    prefix = os.path.basename(indexfn)[:-1]
    (tmpfd, tmpindexfn) = tempfile.mkstemp(dir=os.path.dirname(indexfn),
                                           prefix=prefix,
                                           suffix='.i')
    tmpdatafn = tmpindexfn[:-2] + '.d'
    os.close(tmpfd)

    r1 = revlog.revlog(util.opener(os.getcwd(), audit=False), indexfn)
    r2 = revlog.revlog(util.opener(os.getcwd(), audit=False), tmpindexfn)

    # Don't use repo.transaction(), because then things get hairy with
    # paths: some need to be relative to .hg, and some need to be
    # absolute. Doing it this way keeps things simple: everything is an
    # absolute path.
    lock = repo.lock(wait=False)
    tr = transaction.transaction(ui.warn,
                                 open,
                                 repo.sjoin('journal'))

    try:
        try:
            order = toposort(ui, r1)
            writerevs(ui, r1, r2, order, tr)
            report(ui, datafn, tmpdatafn)
            tr.close()
        except:
            # Abort transaction first, so we truncate the files before
            # deleting them.
            tr.abort()
            if os.path.exists(tmpindexfn):
                os.unlink(tmpindexfn)
            if os.path.exists(tmpdatafn):
                os.unlink(tmpdatafn)
            raise
        if not opts.get('dry_run'):
            # Racy since both files cannot be renamed atomically
            util.os_link(indexfn, oldindexfn)
            util.os_link(datafn, olddatafn)
            util.rename(tmpindexfn, indexfn)
            util.rename(tmpdatafn, datafn)
        else:
            os.unlink(tmpindexfn)
            os.unlink(tmpdatafn)
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
                ],
               _('hg shrink [--revlog PATH]'))
}

if __name__ == "__main__":
    print "shrink-revlog.py is now an extension (see hg help extensions)"
