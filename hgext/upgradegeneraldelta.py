# upgradegeneraldelta.py
#
# Copyright 2014 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""upgrade manifests to generaldelta in-place

Performs an upgrade of this repo's manifest to generaldelta on pull, without
needing to reclone. This tries to be as safe as possible but is inherently not a
completely safe operation.

Filelogs are not touched -- the manifest is often the revlog that benefits the
most from generaldelta. New filelogs will be generaldelta, though.

The following configuration options are available:

:upgradegeneraldelta.upgrade: Set to True to actually perform the
    upgrade. Default is False.

:upgradegeneraldelta.dryrun: Write out the generaldelta manifest, but do not
    move it into place. This will cause the generaldelta manifest to be
    rewritten every single time a Mercurial command is run. Default is False.

:upgradegeneraldelta.backup: Set to False to disable backing up the old manifest
    as 00manifestold.{i,d}. Default is True.

"""

import os
import weakref

from mercurial import commands, extensions, progress, revlog, util
from mercurial.i18n import _


def wrappull(orig, ui, repo, *args, **kwargs):
    _upgrade(ui, repo)
    orig(ui, repo, *args, **kwargs)


def _upgrade(ui, repo):
    if not util.safehasattr(repo, "svfs"):
        return
    if not ui.configbool("upgradegeneraldelta", "upgrade"):
        return

    f = repo.svfs("00manifest.i")
    i = f.read(4)
    f.close()
    if len(i) < 4:
        # empty manifest
        return

    # probably only works with revlogng -- it became the default years ago so
    # that's fine
    v = revlog.versionformat_unpack(i)[0]
    isgeneraldelta = v & revlog.FLAG_GENERALDELTA
    if isgeneraldelta:
        return

    # write out a new revlog, this time with generaldelta
    oldopts = repo.svfs.options
    lock = repo.lock()
    tr = repo.transaction("upgradegeneraldelta")
    try:
        ui.warn(_("upgrading repository to generaldelta\n"))
        trp = weakref.proxy(tr)

        newopts = oldopts.copy()
        newopts["generaldelta"] = 1
        repo.svfs.options = newopts
        # remove 00manifestgd.i if present
        repo.svfs.unlinkpath("00manifestgd.i", ignoremissing=True)
        newmf = revlog.revlog(repo.svfs, "00manifestgd.i")
        oldmf = repo.manifest
        i = oldmf.index
        chunk = oldmf._chunk

        with progress.bar(ui, _("upgrading"), total=len(oldmf)) as prog:
            for rev in oldmf:
                prog.value = rev
                e = i[rev]
                # if the delta base is the rev, this rev is a fulltext
                isdelta = rev != e[3]
                revchunk = chunk(rev)
                if isdelta:
                    newmf.addrevision(
                        None,
                        trp,
                        e[4],
                        i[e[5]][7],
                        i[e[6]][7],
                        cachedelta=(rev - 1, revchunk),
                        node=e[7],
                    )
                else:
                    newmf.addrevision(
                        revchunk, trp, e[4], i[e[5]][7], i[e[6]][7], node=e[7]
                    )
            tr.close()
        if not ui.configbool("upgradegeneraldelta", "dryrun", default=False):
            manifesti = repo.sjoin("00manifest.i")
            manifestd = repo.sjoin("00manifest.d")
            manifestgdi = repo.sjoin("00manifestgd.i")
            manifestgdd = repo.sjoin("00manifestgd.d")
            manifestoldi = repo.sjoin("00manifestold.i")
            manifestoldd = repo.sjoin("00manifestold.d")
            # move the newly created manifests over
            if ui.configbool("upgradegeneraldelta", "backup", default=True):
                os.rename(manifesti, manifestoldi)
                if os.path.exists(manifestd):
                    os.rename(manifestd, manifestoldd)
            os.rename(manifestgdi, manifesti)
            if os.path.exists(manifestgdd):
                os.rename(manifestgdd, manifestd)
            with repo.opener("requires", "a+") as f:
                f.write("generaldelta\n")
        repo.invalidate()
    finally:
        if tr:
            tr.release()
        lock.release()
        repo.svfs.options = oldopts


def extsetup(ui):
    extensions.wrapcommand(commands.table, "pull", wrappull)
