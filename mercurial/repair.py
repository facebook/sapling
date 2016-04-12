# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno

from .i18n import _
from .node import short
from . import (
    bundle2,
    changegroup,
    error,
    exchange,
    obsolete,
    util,
)

def _bundle(repo, bases, heads, node, suffix, compress=True):
    """create a bundle with the specified revisions as a backup"""
    cgversion = changegroup.safeversion(repo)

    cg = changegroup.changegroupsubset(repo, bases, heads, 'strip',
                                       version=cgversion)
    backupdir = "strip-backup"
    vfs = repo.vfs
    if not vfs.isdir(backupdir):
        vfs.mkdir(backupdir)

    # Include a hash of all the nodes in the filename for uniqueness
    allcommits = repo.set('%ln::%ln', bases, heads)
    allhashes = sorted(c.hex() for c in allcommits)
    totalhash = util.sha1(''.join(allhashes)).hexdigest()
    name = "%s/%s-%s-%s.hg" % (backupdir, short(node), totalhash[:8], suffix)

    comp = None
    if cgversion != '01':
        bundletype = "HG20"
        if compress:
            comp = 'BZ'
    elif compress:
        bundletype = "HG10BZ"
    else:
        bundletype = "HG10UN"
    return bundle2.writebundle(repo.ui, cg, name, bundletype, vfs,
                                   compression=comp)

def _collectfiles(repo, striprev):
    """find out the filelogs affected by the strip"""
    files = set()

    for x in xrange(striprev, len(repo)):
        files.update(repo[x].files())

    return sorted(files)

def _collectbrokencsets(repo, files, striprev):
    """return the changesets which will be broken by the truncation"""
    s = set()
    def collectone(revlog):
        _, brokenset = revlog.getstrippoint(striprev)
        s.update([revlog.linkrev(r) for r in brokenset])

    collectone(repo.manifest)
    for fname in files:
        collectone(repo.file(fname))

    return s

def strip(ui, repo, nodelist, backup=True, topic='backup'):
    # This function operates within a transaction of its own, but does
    # not take any lock on the repo.
    # Simple way to maintain backwards compatibility for this
    # argument.
    if backup in ['none', 'strip']:
        backup = False

    repo = repo.unfiltered()
    repo.destroying()

    cl = repo.changelog
    # TODO handle undo of merge sets
    if isinstance(nodelist, str):
        nodelist = [nodelist]
    striplist = [cl.rev(node) for node in nodelist]
    striprev = min(striplist)

    # Some revisions with rev > striprev may not be descendants of striprev.
    # We have to find these revisions and put them in a bundle, so that
    # we can restore them after the truncations.
    # To create the bundle we use repo.changegroupsubset which requires
    # the list of heads and bases of the set of interesting revisions.
    # (head = revision in the set that has no descendant in the set;
    #  base = revision in the set that has no ancestor in the set)
    tostrip = set(striplist)
    for rev in striplist:
        for desc in cl.descendants([rev]):
            tostrip.add(desc)

    files = _collectfiles(repo, striprev)
    saverevs = _collectbrokencsets(repo, files, striprev)

    # compute heads
    saveheads = set(saverevs)
    for r in xrange(striprev + 1, len(cl)):
        if r not in tostrip:
            saverevs.add(r)
            saveheads.difference_update(cl.parentrevs(r))
            saveheads.add(r)
    saveheads = [cl.node(r) for r in saveheads]

    # compute base nodes
    if saverevs:
        descendants = set(cl.descendants(saverevs))
        saverevs.difference_update(descendants)
    savebases = [cl.node(r) for r in saverevs]
    stripbases = [cl.node(r) for r in tostrip]

    # For a set s, max(parents(s) - s) is the same as max(heads(::s - s)), but
    # is much faster
    newbmtarget = repo.revs('max(parents(%ld) - (%ld))', tostrip, tostrip)
    if newbmtarget:
        newbmtarget = repo[newbmtarget.first()].node()
    else:
        newbmtarget = '.'

    bm = repo._bookmarks
    updatebm = []
    for m in bm:
        rev = repo[bm[m]].rev()
        if rev in tostrip:
            updatebm.append(m)

    # create a changegroup for all the branches we need to keep
    backupfile = None
    vfs = repo.vfs
    node = nodelist[-1]
    if backup:
        backupfile = _bundle(repo, stripbases, cl.heads(), node, topic)
        repo.ui.status(_("saved backup bundle to %s\n") %
                       vfs.join(backupfile))
        repo.ui.log("backupbundle", "saved backup bundle to %s\n",
                    vfs.join(backupfile))
    if saveheads or savebases:
        # do not compress partial bundle if we remove it from disk later
        chgrpfile = _bundle(repo, savebases, saveheads, node, 'temp',
                            compress=False)

    mfst = repo.manifest

    curtr = repo.currenttransaction()
    if curtr is not None:
        del curtr  # avoid carrying reference to transaction for nothing
        msg = _('programming error: cannot strip from inside a transaction')
        raise error.Abort(msg, hint=_('contact your extension maintainer'))

    try:
        with repo.transaction("strip") as tr:
            offset = len(tr.entries)

            tr.startgroup()
            cl.strip(striprev, tr)
            mfst.strip(striprev, tr)
            for fn in files:
                repo.file(fn).strip(striprev, tr)
            tr.endgroup()

            for i in xrange(offset, len(tr.entries)):
                file, troffset, ignore = tr.entries[i]
                repo.svfs(file, 'a').truncate(troffset)
                if troffset == 0:
                    repo.store.markremoved(file)

        if saveheads or savebases:
            ui.note(_("adding branch\n"))
            f = vfs.open(chgrpfile, "rb")
            gen = exchange.readbundle(ui, f, chgrpfile, vfs)
            if not repo.ui.verbose:
                # silence internal shuffling chatter
                repo.ui.pushbuffer()
            if isinstance(gen, bundle2.unbundle20):
                with repo.transaction('strip') as tr:
                    tr.hookargs = {'source': 'strip',
                                   'url': 'bundle:' + vfs.join(chgrpfile)}
                    bundle2.applybundle(repo, gen, tr, source='strip',
                                        url='bundle:' + vfs.join(chgrpfile))
            else:
                gen.apply(repo, 'strip', 'bundle:' + vfs.join(chgrpfile), True)
            if not repo.ui.verbose:
                repo.ui.popbuffer()
            f.close()

        for m in updatebm:
            bm[m] = repo[newbmtarget].node()
        lock = tr = None
        try:
            lock = repo.lock()
            tr = repo.transaction('repair')
            bm.recordchange(tr)
            tr.close()
        finally:
            tr.release()
            lock.release()

        # remove undo files
        for undovfs, undofile in repo.undofiles():
            try:
                undovfs.unlink(undofile)
            except OSError as e:
                if e.errno != errno.ENOENT:
                    ui.warn(_('error removing %s: %s\n') %
                            (undovfs.join(undofile), str(e)))

    except: # re-raises
        if backupfile:
            ui.warn(_("strip failed, full bundle stored in '%s'\n")
                    % vfs.join(backupfile))
        elif saveheads:
            ui.warn(_("strip failed, partial bundle stored in '%s'\n")
                    % vfs.join(chgrpfile))
        raise
    else:
        if saveheads or savebases:
            # Remove partial backup only if there were no exceptions
            vfs.unlink(chgrpfile)

    repo.destroyed()

def rebuildfncache(ui, repo):
    """Rebuilds the fncache file from repo history.

    Missing entries will be added. Extra entries will be removed.
    """
    repo = repo.unfiltered()

    if 'fncache' not in repo.requirements:
        ui.warn(_('(not rebuilding fncache because repository does not '
                  'support fncache)\n'))
        return

    with repo.lock():
        fnc = repo.store.fncache
        # Trigger load of fncache.
        if 'irrelevant' in fnc:
            pass

        oldentries = set(fnc.entries)
        newentries = set()
        seenfiles = set()

        repolen = len(repo)
        for rev in repo:
            ui.progress(_('rebuilding'), rev, total=repolen,
                        unit=_('changesets'))

            ctx = repo[rev]
            for f in ctx.files():
                # This is to minimize I/O.
                if f in seenfiles:
                    continue
                seenfiles.add(f)

                i = 'data/%s.i' % f
                d = 'data/%s.d' % f

                if repo.store._exists(i):
                    newentries.add(i)
                if repo.store._exists(d):
                    newentries.add(d)

        ui.progress(_('rebuilding'), None)

        if 'treemanifest' in repo.requirements: # safe but unnecessary otherwise
            for dir in util.dirs(seenfiles):
                i = 'meta/%s/00manifest.i' % dir
                d = 'meta/%s/00manifest.d' % dir

                if repo.store._exists(i):
                    newentries.add(i)
                if repo.store._exists(d):
                    newentries.add(d)

        addcount = len(newentries - oldentries)
        removecount = len(oldentries - newentries)
        for p in sorted(oldentries - newentries):
            ui.write(_('removing %s\n') % p)
        for p in sorted(newentries - oldentries):
            ui.write(_('adding %s\n') % p)

        if addcount or removecount:
            ui.write(_('%d items added, %d removed from fncache\n') %
                     (addcount, removecount))
            fnc.entries = newentries
            fnc._dirty = True

            with repo.transaction('fncache') as tr:
                fnc.write(tr)
        else:
            ui.write(_('fncache already up to date\n'))

def stripbmrevset(repo, mark):
    """
    The revset to strip when strip is called with -B mark

    Needs to live here so extensions can use it and wrap it even when strip is
    not enabled or not present on a box.
    """
    return repo.revs("ancestors(bookmark(%s)) - "
                     "ancestors(head() and not bookmark(%s)) - "
                     "ancestors(bookmark() and not bookmark(%s))",
                     mark, mark, mark)

def deleteobsmarkers(obsstore, indices):
    """Delete some obsmarkers from obsstore and return how many were deleted

    'indices' is a list of ints which are the indices
    of the markers to be deleted.

    Every invocation of this function completely rewrites the obsstore file,
    skipping the markers we want to be removed. The new temporary file is
    created, remaining markers are written there and on .close() this file
    gets atomically renamed to obsstore, thus guaranteeing consistency."""
    if not indices:
        # we don't want to rewrite the obsstore with the same content
        return

    left = []
    current = obsstore._all
    n = 0
    for i, m in enumerate(current):
        if i in indices:
            n += 1
            continue
        left.append(m)

    newobsstorefile = obsstore.svfs('obsstore', 'w', atomictemp=True)
    for bytes in obsolete.encodemarkers(left, True, obsstore._version):
        newobsstorefile.write(bytes)
    newobsstorefile.close()
    return n
