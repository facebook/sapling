# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import os
import shutil
import time

import bindings

from .. import changelog2, clone, hg, progress, pycompat, util
from ..i18n import _
from ..node import nullid, short
from ..revlog import hash as revloghash
from .cmdtable import command


@command("debugrebuildchangelog", [])
def debugrebuildchangelog(ui, repo, **opts):
    """rebuild changelog by recloning and copying draft commits"""

    commits = _readdrafts(repo)
    ui.write(_("read %s draft commits\n") % len(commits))

    tmprepopath = repo.svfs.join("changelog-rebuild")
    tmprepo = _clonetotmp(repo, tmprepopath)

    _addcommits(tmprepo, commits)
    ui.write(_("recreated %s draft commits\n") % len(commits))

    _replacechangelog(tmprepo, repo)
    ui.write(_("changelog rebuilt\n"))

    tmprepo.close()


def _readdrafts(repo):
    """read draft commits as [(node, parents, text)]"""
    ui = repo.ui
    zstore = bindings.zstore.zstore(repo.svfs.join(changelog2.HGCOMMITS_DIR))
    revlog = changelog2.changelog.openrevlog(repo.svfs, ui.uiconfig())

    draftrevs = repo.revs("draft()")
    cl = repo.changelog
    tonode = cl.node
    commits = []  # [(node, parents, text)]
    with progress.bar(
        ui, _("reading draft commits"), _("commits"), len(draftrevs)
    ) as prog:
        for rev in draftrevs:
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
    clone.shallowclone("default", tmprepo)
    return tmprepo


def _addcommits(repo, commits):
    with repo.lock(), repo.transaction("debugrebuildchangelog"):
        repo.changelog.inner.addcommits(commits)
        repo.changelog.inner.flush([])


def _replacechangelog(srcrepo, dstrepo):
    """Replace changelog (revlog) at dstrepo with revlog from srcrepo.

    Revlog is used because it's still the only supported format for
    streamclone.
    """
    with dstrepo.lock():
        suffix = time.strftime("%m%d%H%M%S")
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
