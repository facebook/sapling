# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import marshal
import os
import shutil
import time

import bindings

from .. import (
    bookmarks as bookmod,
    changelog2,
    clone,
    extensions,
    hg,
    progress,
    pycompat,
    util,
    visibility,
)
from ..i18n import _
from ..node import hex, nullid, short
from ..revlog import hash as revloghash
from .cmdtable import command


@command(
    "debugrebuildchangelog",
    [
        ("", "revlog", False, _("use legacy revlog backend (DEPRECATED)")),
    ],
)
def debugrebuildchangelog(ui, repo, **opts):
    """rebuild changelog by recloning and copying draft commits

    This is a destructive command that will remove invisible commits including
    shelved changes based on invisible commits, and truncate metalog history.
    """

    shelved = _readshelved(repo)
    ts = _timestamp()

    if opts.get("revlog"):
        commits = _readdrafts(repo) + shelved
        _backupcommits(repo, commits, ts)

        tmprepopath = repo.svfs.join("changelog-rebuild")
        tmprepo = _clonetotmp(repo, tmprepopath)

        _addcommits(tmprepo, commits)
        ui.write(_("recreated %s local commits\n") % len(commits))

        _replacechangelogrevlog(tmprepo, repo)
        ui.write(_("changelog rebuilt\n"))

        tmprepo.close()
    else:
        api = repo.edenapi
        assert api

        # Import segments (lazy changelog) to temporary directories.
        tmpsuffix = "tmp.%s" % ts
        hgcommits = bindings.dag.commits.openhybrid(
            revlogdir=None,
            segmentsdir=repo.svfs.join(_withsuffix(changelog2.SEGMENTS_DIR, tmpsuffix)),
            commitsdir=repo.svfs.join(_withsuffix(changelog2.HGCOMMITS_DIR, tmpsuffix)),
            edenapi=api,
            lazyhash=True,
        )
        data = api.clonedata()
        hgcommits.importclonedata(data)

        # The "try" block also protects repo lock.__exit__, etc.
        baksuffix = None
        try:
            with repo.wlock(), repo.lock(), repo.transaction(
                "debugrebuildchangelog"
            ), repo.dirstate.parentchange():
                # Backup non-master commits
                commits = _readnonmasterdrafts(repo) + shelved
                _backupcommits(repo, commits, ts)

                allnodes = hgcommits.dagalgo().all()

                remotenames = {}
                tip = allnodes.first()
                main = bookmod.mainbookmark(repo)
                if tip:
                    # Write a "fake" remote bookmark for the imported tip to make
                    # pull discovery cheaper.
                    remotename = ui.config("remotenames", "rename.default") or "default"
                    remotenames["%s/%s" % (remotename, main)] = tip
                    ui.write(_("imported clone data with tip %s\n") % hex(tip))

                # Reset references so they won't complain about unknown nodes
                # during pull. remotenames and visibleheads will be rebuilt.
                # bookmarks will be restored to the original state.
                origvisibleheads = repo.svfs.read("visibleheads")
                origbookmarks = repo.svfs.read("bookmarks")
                origdparents = repo.dirstate.parents()
                repo.svfs.write("tip", tip or b"")
                repo.svfs.write("remotenames", bookmod.encoderemotenames(remotenames))
                repo.svfs.write("visibleheads", visibility.encodeheads(tip and [tip]))
                repo.svfs.write("bookmarks", b"")
                repo.dirstate.setparents(nullid, nullid)

                # This is the *destructive* operation that makes commits "missing".
                # Before this, hgcommits is the way to access the commit graph.
                # After this, repo.changelog.inner is the way to access the
                # commit graph.
                baksuffix = _replacechangelogsegments(repo, tmpsuffix, ts)

                try:
                    # Pull. This is a code path that accesses a lot of stuffs and
                    # can go wrong in various ways. So it's protected by "try".
                    ui.write(_("pulling latest commits\n"))
                    util.failpoint("debugrebuildchangelog-before-pull")
                    repo.pull(bookmarknames=[main])

                    # Re-add the commits. Note: In rare cases (ex. server master
                    # moves back), this might fail.
                    # Some commits might become "known" after pull. So filter them
                    # out. Also, prefetch parents of commits.
                    nodes = [c[0] for c in commits] + [p for c in commits for p in c[1]]
                    known = set(repo.changelog.filternodes(nodes))

                    ui.write(_("recreating %s local commits\n") % len(commits))
                    repo.changelog.inner.addcommits(
                        [c for c in commits if c[0] not in known]
                    )
                finally:
                    # Restore dirstate parents, bookmarks and visibleheads.
                    repo.dirstate.setparents(origdparents[0], origdparents[1])
                    repo.svfs.write("bookmarks", origbookmarks)
                    repo.svfs.write("visibleheads", origvisibleheads)
        except BaseException:
            if baksuffix:
                ui.write(_("restoring changelog from previous state\n"))
                _replacechangelogsegments(repo, baksuffix, ts)
            raise

        ui.write(_("changelog rebuilt\n"))

    # Truncate metalog since older commit references are probably invalidated.
    with repo.lock():
        ml = repo.metalog()
        ml.compact(ml.path())


def _withsuffix(name, suffix):
    """segments/v1 -> segments.suffix/v1"""
    split = name.split("/", 1)
    split[0] = "%s.%s" % (split[0], suffix)
    return "/".join(split)


def _readdrafts(repo):
    revs = repo.revs("draft()")
    return _readcommits(repo, revs)


def _readshelved(repo):
    try:
        extensions.find("shelve")
    except KeyError:
        return []
    # Only consider shelved changes based on visible commits to reduce
    # overhead.
    cl = repo.changelog
    shelved = cl.tonodes(repo.revs("shelved()"))
    visible = cl.tonodes(repo.revs("all()"))
    visibleshelved = repo.dageval(lambda: shelved & children(visible))
    return _readcommits(repo, cl.torevset(visibleshelved))


def _backupcommits(repo, commits, ts):
    bakname = "commits-%s-%s.bak" % (len(commits), ts)
    with open(repo.svfs.join(bakname), "wb") as f:
        f.write(marshal.dumps(commits))
    repo.ui.write(_("backed up %s commits to %s\n") % (len(commits), bakname))
    return bakname


def _readnonmasterdrafts(repo):
    main = bookmod.mainbookmark(repo)
    revs = repo.revs("draft() %% present(%s)", main)
    return _readcommits(repo, revs)


def _readcommits(repo, revs):
    """read commits as [(node, parents, text)]"""
    ui = repo.ui
    zstore = bindings.zstore.zstore(repo.svfs.join(changelog2.HGCOMMITS_DIR))
    revlog = changelog2.changelog.openrevlog(repo, ui.uiconfig())

    cl = repo.changelog
    tonode = cl.node
    commits = []  # [(node, parents, text)]
    with progress.bar(ui, _("reading commits"), _("commits"), len(revs)) as prog:
        for rev in revs:
            prog.value += 1
            try:
                node = tonode(rev)
            except Exception as e:
                ui.write(_("cannot translate rev %s: %s\n") % (rev, e))
                continue

            textp1p2 = _tryreadtextp1p2(node, zstore, revlog)
            if textp1p2 is None:
                ui.write(_("cannot read commit %s\n") % short(node))
                continue

            text, p1, p2 = textp1p2
            parents = [p for p in (p1, p2) if p != nullid]
            commits.append((node, parents, text))

    return commits


def _tryreadtextp1p2(node, zstore, revlog):
    """Attempt to read (text, p1, p2) from multiple sources, including:

    - changelog revlog
    - zstore (used by Rust segments backend)
    """
    try:
        text = revlog.revision(node)
        p1, p2 = revlog.parents(node)
        if revloghash(text, p1, p2) == node:
            return text, p1, p2
    except Exception:
        pass
    try:
        # The zstore stores sorted(p1, p2) + text to match SHA1 checksum.
        # The order of p1, p2 is lost as the SHA1 hash does not include
        # the order. For non-merge commits the nullid comes first, so
        # we read it as (p2, p1, text).
        p2p1text = zstore.get(node)
        p2 = p2p1text[:20]
        p1 = p2p1text[20:40]
        text = p2p1text[40:]
        if revloghash(text, p1, p2) == node:
            return text, p1, p2
    except Exception:
        pass
    return None


def _clonetotmp(repo, tmprepopath):
    """Stream clone to a temp repo"""
    # streamclone is still the fastest way of getting changelog from the server
    # create a new repo for streaming clone
    try:
        shutil.rmtree(tmprepopath)
    except OSError:
        pass
    util.makedirs(tmprepopath)
    tmprepo = hg.repository(repo.ui, path=tmprepopath, create=True)
    with tmprepo.lock():
        tmprepo.requirements.add("remotefilelog")
        tmprepo._writerequirements()
        tmprepo.storerequirements.add("rustrevlogchangelog")
        tmprepo._writestorerequirements()
        with tmprepo.localvfs.open("hgrc", "a") as f:
            f.write(
                b"\n%%include %s\n" % pycompat.encodeutf8(repo.localvfs.join("hgrc"))
            )
    tmprepo = hg.repository(repo.ui, path=tmprepopath)
    clone.revlogclone("default", tmprepo)
    return tmprepo


def _addcommits(repo, commits):
    with repo.lock(), repo.transaction("debugrebuildchangelog"):
        repo.changelog.inner.addcommits(commits)
        repo.changelog.inner.flush([])


def _timestamp():
    """Return a timestamp string that is likely unique"""
    if util.istest():
        return "0000"
    else:
        return time.strftime("%m%d%H%M%S")


def _replacechangelogrevlog(srcrepo, dstrepo):
    """Replace changelog (revlog) at dstrepo with revlog from srcrepo.

    Revlog is used because it's still the only supported format for
    streamclone.
    """
    with dstrepo.lock():
        suffix = _timestamp()
        dstrepo.svfs.rename("00changelog.i", "00changelog.i.%s" % suffix)
        dstrepo.svfs.rename("00changelog.d", "00changelog.d.%s" % suffix)
        dstrepo.svfs.tryunlink("00changelog.len")
        os.rename(
            srcrepo.svfs.join("00changelog.d"), dstrepo.svfs.join("00changelog.d")
        )
        os.rename(
            srcrepo.svfs.join("00changelog.i"), dstrepo.svfs.join("00changelog.i")
        )
        changelog2._removechangelogrequirements(dstrepo)
        dstrepo.storerequirements.add("rustrevlogchangelog")
        dstrepo._writestorerequirements()


def _replacechangelogsegments(repo, suffix, timestamp):
    """Replace changelog segments from segments.suffix

    Return the backup suffix if the original segments were backed up.
    """
    with repo.lock():
        if repo.svfs.exists("segments"):
            baksuffix = "old.%s" % timestamp
            # Pick a unique suffix.
            count = 0
            while repo.svfs.exists("segments.%s" % baksuffix):
                count += 1
                baksuffix = "%s_%s" % (baksuffix.split("_")[0], count)
            if repo.svfs.exists("segments"):
                repo.svfs.rename("segments", "segments.%s" % baksuffix)
        else:
            baksuffix = None
        os.rename(
            repo.svfs.join("segments.%s" % suffix),
            repo.svfs.join("segments"),
        )
        changelog2._removechangelogrequirements(repo)
        repo.storerequirements.add("lazychangelog")
        repo._writestorerequirements()
        repo.invalidatechangelog()
        repo.invalidate(True)
        return baksuffix
