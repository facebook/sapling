# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# repair.py - functions for repository repair for mercurial
#
# Copyright 2005, 2006 Chris Mason <mason@suse.com>
# Copyright 2007 Matt Mackall
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import hashlib

from . import (
    bundle2,
    changegroup,
    discovery,
    error,
    exchange,
    obsolete,
    obsutil,
    progress,
    pycompat,
    util,
    visibility,
)
from .i18n import _
from .node import hex, short
from .pycompat import range


def _bundle(repo, bases, heads, node, suffix, compress=True, obsolescence=True):
    """create a bundle with the specified revisions as a backup"""

    backupdir = "strip-backup"
    vfs = repo.localvfs
    if not vfs.isdir(backupdir):
        vfs.mkdir(backupdir)

    # Include a hash of all the nodes in the filename for uniqueness
    allcommits = repo.set("%ln::%ln", bases, heads)
    allhashes = sorted(c.hex() for c in allcommits)
    totalhash = hashlib.sha1("".join(allhashes)).digest()
    name = "%s/%s-%s-%s.hg" % (backupdir, short(node), hex(totalhash[:4]), suffix)

    cgversion = changegroup.localversion(repo)
    comp = None
    if cgversion != "01":
        bundletype = "HG20"
        if compress:
            comp = "BZ"
    elif compress:
        bundletype = "HG10BZ"
    else:
        bundletype = "HG10UN"

    outgoing = discovery.outgoing(repo, missingroots=bases, missingheads=heads)
    contentopts = {
        "cg.version": cgversion,
        "obsolescence": obsolescence,
        "phases": True,
    }
    return bundle2.writenewbundle(
        repo.ui,
        repo,
        "strip",
        name,
        bundletype,
        outgoing,
        contentopts,
        vfs,
        compression=comp,
    )


def _collectfiles(repo, striprev):
    """find out the filelogs affected by the strip"""
    files = set()

    for x in range(striprev, len(repo)):
        files.update(repo[x].files())

    return sorted(files)


def _collectrevlog(revlog, striprev):
    _, brokenset = revlog.getstrippoint(striprev)
    return [revlog.linkrev(r) for r in brokenset]


def _collectmanifest(repo, striprev):
    return _collectrevlog(repo.manifestlog._revlog, striprev)


def _collectbrokencsets(repo, files, striprev):
    """return the changesets which will be broken by the truncation"""
    s = set()

    s.update(_collectmanifest(repo, striprev))
    for fname in files:
        s.update(_collectrevlog(repo.file(fname), striprev))

    return s


def strip(ui, repo, nodelist, backup=True, topic="backup"):
    # This function requires the caller to lock the repo, but it operates
    # within a transaction of its own, and thus requires there to be no current
    # transaction when it is called.
    if repo.currenttransaction() is not None:
        raise error.ProgrammingError("cannot strip from inside a transaction")

    # Simple way to maintain backwards compatibility for this
    # argument.
    if backup in ["none", "strip"]:
        backup = False

    repo = repo.unfiltered()
    repo.destroying()

    cl = repo.changelog
    # TODO handle undo of merge sets
    if isinstance(nodelist, str):
        nodelist = [nodelist]
    striplist = [cl.rev(node) for node in nodelist]
    striprev = min(striplist)

    files = _collectfiles(repo, striprev)
    saverevs = _collectbrokencsets(repo, files, striprev)

    # Some revisions with rev > striprev may not be descendants of striprev.
    # We have to find these revisions and put them in a bundle, so that
    # we can restore them after the truncations.
    # To create the bundle we use repo.changegroupsubset which requires
    # the list of heads and bases of the set of interesting revisions.
    # (head = revision in the set that has no descendant in the set;
    #  base = revision in the set that has no ancestor in the set)
    tostrip = set(striplist)
    saveheads = set(saverevs)
    for r in cl.revs(start=striprev + 1):
        if any(p in tostrip for p in cl.parentrevs(r)):
            tostrip.add(r)

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

    stripobsidx = obsmarkers = ()
    if repo.ui.configbool("devel", "strip-obsmarkers"):
        obsmarkers = obsutil.exclusivemarkers(repo, stripbases)
    if obsmarkers:
        stripobsidx = [i for i, m in enumerate(repo.obsstore) if m in obsmarkers]

    # For a set s, max(parents(s) - s) is the same as max(heads(::s - s)), but
    # is much faster
    newbmtarget = repo.revs("max(parents(%ld) - (%ld))", tostrip, tostrip)
    if newbmtarget:
        newbmtarget = repo[newbmtarget.first()].node()
    else:
        newbmtarget = "."

    bm = repo._bookmarks
    updatebm = []
    for m in bm:
        rev = repo[bm[m]].rev()
        if rev in tostrip:
            updatebm.append(m)

    # create a changegroup for all the branches we need to keep
    backupfile = None
    vfs = repo.localvfs
    node = nodelist[-1]
    if backup:
        backupfile = _bundle(repo, stripbases, cl.heads(), node, topic)
        repo.ui.status(_("saved backup bundle to %s\n") % vfs.join(backupfile))
        repo.ui.log("backupbundle", "saved backup bundle to %s\n", vfs.join(backupfile))
    tmpbundlefile = None
    if saveheads:
        # do not compress temporary bundle if we remove it from disk later
        #
        # We do not include obsolescence, it might re-introduce prune markers
        # we are trying to strip.  This is harmless since the stripped markers
        # are already backed up and we did not touched the markers for the
        # saved changesets.
        tmpbundlefile = _bundle(
            repo, savebases, saveheads, node, "temp", compress=False, obsolescence=False
        )

    try:
        with repo.transaction("strip") as tr:
            offset = len(tr.entries)

            visibility.remove(repo, stripbases)

            tr.startgroup()
            cl.strip(striprev, tr)
            stripmanifest(repo, striprev, tr, files)

            for fn in files:
                repo.file(fn).strip(striprev, tr)
            tr.endgroup()

            for i in range(offset, len(tr.entries)):
                file, troffset, ignore = tr.entries[i]
                with repo.svfs(file, "a", checkambig=True) as fp:
                    util.truncate(fp, troffset)
                if troffset == 0:
                    repo.store.markremoved(file)

            deleteobsmarkers(repo.obsstore, stripobsidx)
            del repo.obsstore

        repo._phasecache.filterunknown(repo)
        if tmpbundlefile:
            ui.note(_("adding branch\n"))
            f = vfs.open(tmpbundlefile, "rb")
            gen = exchange.readbundle(ui, f, tmpbundlefile, vfs)
            if not repo.ui.verbose:
                # silence internal shuffling chatter
                repo.ui.pushbuffer()
            tmpbundleurl = "bundle:" + vfs.join(tmpbundlefile)
            txnname = "strip"
            if not isinstance(gen, bundle2.unbundle20):
                txnname = "strip\n%s" % util.hidepassword(tmpbundleurl)
            with repo.transaction(txnname) as tr:
                bundle2.applybundle(repo, gen, tr, source="strip", url=tmpbundleurl)
            if not repo.ui.verbose:
                repo.ui.popbuffer()
            f.close()
        repo._phasecache.invalidate()

        with repo.transaction("repair") as tr:
            bmchanges = [(m, repo[newbmtarget].node()) for m in updatebm]
            bm.applychanges(repo, tr, bmchanges)

        # remove undo files
        for undovfs, undofile in repo.undofiles():
            try:
                undovfs.unlink(undofile)
            except OSError as e:
                if e.errno != errno.ENOENT:
                    ui.warn(
                        _("error removing %s: %s\n") % (undovfs.join(undofile), str(e))
                    )

    except:  # re-raises
        if backupfile:
            ui.warn(
                _("strip failed, backup bundle stored in '%s'\n") % vfs.join(backupfile)
            )
        if tmpbundlefile:
            ui.warn(
                _("strip failed, unrecovered changes stored in '%s'\n")
                % vfs.join(tmpbundlefile)
            )
            ui.warn(
                _(
                    "(fix the problem, then recover the changesets with "
                    "\"hg unbundle '%s'\")\n"
                )
                % vfs.join(tmpbundlefile)
            )
        raise
    else:
        if tmpbundlefile:
            # Remove temporary bundle only if there were no exceptions
            vfs.unlink(tmpbundlefile)

    repo.destroyed()
    # return the backup file path (or None if 'backup' was False) so
    # extensions can use it
    return backupfile


def safestriproots(ui, repo, nodes):
    """return list of roots of nodes where descendants are covered by nodes"""
    torev = repo.unfiltered().changelog.rev
    revs = set(torev(n) for n in nodes)
    # tostrip = wanted - unsafe = wanted - ancestors(orphaned)
    # orphaned = affected - wanted
    # affected = descendants(roots(wanted))
    # wanted = revs
    tostrip = set(repo.revs("%ld-(::((roots(%ld)::)-%ld))", revs, revs, revs))
    notstrip = revs - tostrip
    if notstrip:
        nodestr = ", ".join(sorted(short(repo[n].node()) for n in notstrip))
        ui.warn(
            _("warning: orphaned descendants detected, " "not stripping %s\n") % nodestr
        )
    return [c.node() for c in repo.set("roots(%ld)", tostrip)]


class stripcallback(object):
    """used as a transaction postclose callback"""

    def __init__(self, ui, repo, backup, topic):
        self.ui = ui
        self.repo = repo
        self.backup = backup
        self.topic = topic or "backup"
        self.nodelist = []

    def addnodes(self, nodes):
        self.nodelist.extend(nodes)

    def __call__(self, tr):
        roots = safestriproots(self.ui, self.repo, self.nodelist)
        if roots:
            strip(self.ui, self.repo, roots, self.backup, self.topic)


def delayedstrip(ui, repo, nodelist, topic=None):
    """like strip, but works inside transaction and won't strip irreverent revs

    nodelist must explicitly contain all descendants. Otherwise a warning will
    be printed that some nodes are not stripped.

    Always do a backup. The last non-None "topic" will be used as the backup
    topic name. The default backup topic name is "backup".
    """
    tr = repo.currenttransaction()
    if not tr:
        nodes = safestriproots(ui, repo, nodelist)
        return strip(ui, repo, nodes, True, topic)
    # transaction postclose callbacks are called in alphabet order.
    # use '\xff' as prefix so we are likely to be called last.
    callback = tr.getpostclose("\xffstrip")
    if callback is None:
        callback = stripcallback(ui, repo, True, topic)
        tr.addpostclose("\xffstrip", callback)
    if topic:
        callback.topic = topic
    callback.addnodes(nodelist)


def stripmanifest(repo, striprev, tr, files):
    revlog = repo.manifestlog._revlog
    revlog.strip(striprev, tr)
    striptrees(repo, tr, striprev, files)


def striptrees(repo, tr, striprev, files):
    if "treemanifest" in repo.requirements:  # safe but unnecessary
        # otherwise
        for unencoded, encoded, size in repo.store.datafiles():
            if unencoded.startswith("meta/") and unencoded.endswith("00manifest.i"):
                dir = unencoded[5:-12]
                repo.manifestlog._revlog.dirlog(dir).strip(striprev, tr)


def rebuildfncache(ui, repo):
    """Rebuilds the fncache file from repo history.

    Missing entries will be added. Extra entries will be removed.
    """
    repo = repo.unfiltered()

    if "fncache" not in repo.requirements:
        ui.warn(
            _(
                "(not rebuilding fncache because repository does not "
                "support fncache)\n"
            )
        )
        return

    with repo.lock():
        fnc = repo.store.fncache
        # Trigger load of fncache.
        if "irrelevant" in fnc:
            pass

        oldentries = set(fnc.entries)
        newentries = set()
        seenfiles = set()

        repolen = len(repo)
        with progress.bar(ui, _("rebuilding"), _("changesets"), repolen) as prog:
            for rev in repo:
                prog.value = rev
                ctx = repo[rev]
                for f in ctx.files():
                    # This is to minimize I/O.
                    if f in seenfiles:
                        continue
                    seenfiles.add(f)

                    i = "data/%s.i" % f
                    d = "data/%s.d" % f

                    if repo.store._exists(i):
                        newentries.add(i)
                    if repo.store._exists(d):
                        newentries.add(d)

        if "treemanifest" in repo.requirements:  # safe but unnecessary otherwise
            for dir in util.dirs(seenfiles):
                i = "meta/%s/00manifest.i" % dir
                d = "meta/%s/00manifest.d" % dir

                if repo.store._exists(i):
                    newentries.add(i)
                if repo.store._exists(d):
                    newentries.add(d)

        addcount = len(newentries - oldentries)
        removecount = len(oldentries - newentries)
        for p in sorted(oldentries - newentries):
            ui.write(_("removing %s\n") % p)
        for p in sorted(newentries - oldentries):
            ui.write(_("adding %s\n") % p)

        if addcount or removecount:
            ui.write(
                _("%d items added, %d removed from fncache\n") % (addcount, removecount)
            )
            fnc.entries = newentries
            fnc._dirty = True

            with repo.transaction("fncache") as tr:
                fnc.write(tr)
        else:
            ui.write(_("fncache already up to date\n"))


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

    newobsstorefile = obsstore.svfs("obsstore", "w", atomictemp=True)
    for bytes in obsolete.encodemarkers(left, True, obsstore._version):
        newobsstorefile.write(bytes)
    newobsstorefile.close()
    return n
