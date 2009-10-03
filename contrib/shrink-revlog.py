#!/usr/bin/env python

"""\
Reorder a revlog (by default the the manifest file in the current
repository) to save space.  Specifically, this topologically sorts the
revisions in the revlog so that revisions on the same branch are adjacent
as much as possible.  This is a workaround for the fact that Mercurial
computes deltas relative to the previous revision rather than relative to a
parent revision.  This is *not* safe to run on a changelog.
"""

# Originally written by Benoit Boissinot <benoit.boissinot at ens-lyon.org>
# as a patch to rewrite-log.  Cleaned up, refactored, documented, and
# renamed by Greg Ward <greg at gerg.ca>.

# XXX would be nice to have a way to verify the repository after shrinking,
# e.g. by comparing "before" and "after" states of random changesets
# (maybe: export before, shrink, export after, diff).

import sys, os, tempfile
import optparse
from mercurial import ui as ui_, hg, revlog, transaction, node, util

def toposort(rl):
    write = sys.stdout.write

    children = {}
    root = []
    # build children and roots
    write('reading %d revs ' % len(rl))
    try:
        for i in rl:
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

            if i % 1000 == 0:
                write('.')
    finally:
        write('\n')

    # XXX this is a reimplementation of the 'branchsort' topo sort
    # algorithm in hgext.convert.convcmd... would be nice not to duplicate
    # the algorithm
    write('sorting ...')
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
    write('\n')
    return ret

def writerevs(r1, r2, order, tr):
    write = sys.stdout.write
    write('writing %d revs ' % len(order))
    try:
        count = 0
        for rev in order:
            n = r1.node(rev)
            p1, p2 = r1.parents(n)
            l = r1.linkrev(rev)
            t = r1.revision(n)
            n2 = r2.addrevision(t, tr, l, p1, p2)

            if count % 1000 == 0:
                write('.')
            count += 1
    finally:
        write('\n')
    
def report(olddatafn, newdatafn):
    oldsize = float(os.stat(olddatafn).st_size)
    newsize = float(os.stat(newdatafn).st_size)

    # argh: have to pass an int to %d, because a float >= 2^32 
    # blows up under Python 2.5 or earlier
    sys.stdout.write('old file size: %12d bytes (%6.1f MiB)\n'
                     % (int(oldsize), oldsize/1024/1024))
    sys.stdout.write('new file size: %12d bytes (%6.1f MiB)\n'
                     % (int(newsize), newsize/1024/1024))

    shrink_percent = (oldsize - newsize) / oldsize * 100
    shrink_factor = oldsize / newsize
    sys.stdout.write('shrinkage: %.1f%% (%.1fx)\n'
                     % (shrink_percent, shrink_factor))

def main():

    # Unbuffer stdout for nice progress output.
    sys.stdout = os.fdopen(sys.stdout.fileno(), 'w', 0)
    write = sys.stdout.write

    parser = optparse.OptionParser(description=__doc__)
    parser.add_option('-R', '--repository',
                      default=os.path.curdir,
                      metavar='REPO',
                      help='repository root directory [default: current dir]')
    parser.add_option('--revlog',
                      metavar='FILE',
                      help='shrink FILE [default: REPO/hg/store/00manifest.i]')
    (options, args) = parser.parse_args()
    if args:
        parser.error('too many arguments')

    # Open the specified repository.
    ui = ui_.ui()
    repo = hg.repository(ui, options.repository)
    if not repo.local():
        parser.error('not a local repository: %s' % options.repository)

    if options.revlog is None:
        indexfn = repo.sjoin('00manifest.i')
    else:
        if not options.revlog.endswith('.i'):
            parser.error('--revlog option must specify the revlog index file '
                         '(*.i), not %s' % options.revlog)

        indexfn = os.path.realpath(options.revlog)
        store = repo.sjoin('')
        if not indexfn.startswith(store):
            parser.error('--revlog option must specify a revlog in %s, not %s'
                         % (store, indexfn))

    datafn = indexfn[:-2] + '.d'
    if not os.path.exists(indexfn):
        parser.error('no such file: %s' % indexfn)
    if '00changelog' in indexfn:
        parser.error('shrinking the changelog will corrupt your repository')
    if not os.path.exists(datafn):
        # This is just a lazy shortcut because I can't be bothered to
        # handle all the special cases that entail from no .d file.
        parser.error('%s does not exist: revlog not big enough '
                     'to be worth shrinking' % datafn)

    oldindexfn = indexfn + '.old'
    olddatafn = datafn + '.old'
    if os.path.exists(oldindexfn) or os.path.exists(olddatafn):
        parser.error('one or both of\n'
                     '  %s\n'
                     '  %s\n'
                     'exists from a previous run; please clean up before '
                     'running again'
                     % (oldindexfn, olddatafn))

    write('shrinking %s\n' % indexfn)
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
    # absolute.  Doing it this way keeps things simple: everything is an
    # absolute path.
    lock = repo.lock(wait=False)
    tr = transaction.transaction(sys.stderr.write,
                                 open,
                                 repo.sjoin('journal'))

    try:
        try:
            order = toposort(r1)
            writerevs(r1, r2, order, tr)
            report(datafn, tmpdatafn)
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
    finally:
        lock.release()

    os.link(indexfn, oldindexfn)
    os.link(datafn, olddatafn)
    os.rename(tmpindexfn, indexfn)
    os.rename(tmpdatafn, datafn)
    write('note: old revlog saved in:\n'
          '  %s\n'
          '  %s\n'
          '(You can delete those files when you are satisfied that your\n'
          'repository is still sane.  '
          'Running \'hg verify\' is strongly recommended.)\n'
          % (oldindexfn, olddatafn))

try:
    main()
except KeyboardInterrupt:
    sys.exit("interrupted")
