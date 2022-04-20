# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time

from edenscm.mercurial import hg, scmutil, cmdutil, error
from edenscm.mercurial.i18n import _

from .metalog import storelatest


def _hasanychanges(repo):
    wctx = repo[None]
    return (
        bool(wctx.dirty(missing=True)) or len(wctx.status(listunknown=True).unknown) > 0
    )


def _fullclean(ui, repo, exclude):
    start_time = time.perf_counter()
    ui.status(_("cleaning up uncommitted code\n"), component="snapshot")
    # The order of operations to cleanup here is very deliberate, to avoid errors.
    # Most errors happen due to file/dir clashes, see https://fburl.com/jwyhd0fk
    # Step 1: Forget files that were "hg added"
    # WARNING: Don't call cmdutil.forget because it might be slow
    wctx = repo[None]
    forget = set(wctx.added()) - set(exclude)
    if forget:
        wctx.forget(list(forget), "")
    # Step 2: Remove "untracked changes" (e.g. untracked files)
    repo.dirstate._fs.purge(
        scmutil.match(repo[None], opts={"exclude": exclude}),
        removefiles=True,
        removedirs=True,
        removeignored=False,
        dryrun=False,
    )
    # Step 3: Remove "tracked changes"
    cmdutil.revert(
        ui,
        repo,
        scmutil.revsingle(repo, None),
        repo.dirstate.parents(),
        all=True,
        no_backup=True,
        exclude=exclude,
    )
    duration = time.perf_counter() - start_time
    ui.status(
        _("cleaned up uncommitted code in {duration:0.5f} seconds\n").format(
            duration=duration
        ),
        component="snapshot",
    )


def fetchsnapshot(repo, csid):
    return repo.edenapi.fetchsnapshot(
        {
            "cs_id": csid,
        },
    )


def update(ui, repo, csid, clean=False):
    ui.status(
        _("Will restore snapshot {}\n").format(csid.format()), component="snapshot"
    )
    start_snapshot = time.perf_counter()
    csid_bytes = bytes.fromhex(csid)

    snapshot = fetchsnapshot(repo, csid_bytes)

    # Once merges/conflicted states are supported, we'll need to support more
    # than one parent
    assert isinstance(snapshot["hg_parents"], bytes)

    with repo.wlock(), repo.lock(), repo.transaction("snapshot-restore"):
        haschanges = _hasanychanges(repo)
        if haschanges and not clean:
            raise error.Abort(
                _(
                    "Can't restore snapshot with unclean working copy, unless --clean is specified"
                )
            )

        parent = snapshot["hg_parents"]
        if parent != repo.dirstate.p1():
            if haschanges:
                _fullclean(ui, repo, [])
            start_parent_update = time.perf_counter()
            ui.status(
                _("Updating to parent {}\n").format(parent.hex()),
                component="snapshot",
            )

            # This will resolve the parent revision even if it's not available locally
            # and needs pulling from server.
            if parent not in repo:
                repo.pull(headnodes=(parent,))

            hg.updatetotally(ui, repo, parent, None, clean=False, updatecheck="abort")
            duration = time.perf_counter() - start_parent_update
            ui.status(
                _("Updated to parent {parent} in {duration:0.5f} seconds\n").format(
                    parent=parent.hex(), duration=duration
                ),
                component="snapshot",
            )
        else:
            if haschanges:
                # We might be able to reuse files that were already downloaded locally,
                # so let's not delete files related to the snapshot
                _fullclean(
                    ui, repo, [f"path:{path}" for (path, _) in snapshot["file_changes"]]
                )

        files2download = []
        pathtype = []

        wctx = repo[None]
        for (path, fc) in snapshot["file_changes"]:
            fctx = wctx[path]
            # fc is either a string or a dict, can't use `"Deletion" in fc` because
            # that applies to "UntrackedDeletion" as well
            if fc == "Deletion":
                wctx.forget([path], quiet=True)
                if fctx.exists():
                    fctx.remove()
            elif fc == "UntrackedDeletion":
                if repo.dirstate[path] == "?":
                    # File was hg added then deleted
                    repo.dirstate.add(path)
                elif repo.dirstate[path] == "r":
                    # Missing file, but its marked as deleted. To mark it as missing,
                    # we need to first create a dummy file and mark it as normal
                    repo.wwrite(path, b"", "")
                    repo.dirstate.normal(path)
                    fctx = wctx[path]
                if fctx.exists():
                    fctx.remove()
            elif "Change" in fc:
                filetype = fc["Change"]["file_type"]
                pathtype.append((path, filetype))
                files2download.append((path, fc["Change"]["upload_token"], filetype))
            elif "UntrackedChange" in fc:
                wctx.forget([path], quiet=True)
                filetype = fc["UntrackedChange"]["file_type"]
                pathtype.append((path, filetype))
                files2download.append(
                    (
                        path,
                        fc["UntrackedChange"]["upload_token"],
                        filetype,
                    )
                )
        ui.status(
            _("Downloading files for restoring snapshot\n"),
            component="snapshot",
        )
        start_download = time.perf_counter()
        repo.edenapi.downloadfiles(repo.root, files2download)
        duration = time.perf_counter() - start_download
        ui.status(
            _(
                "Downloaded files for restoring snapshot in {duration:0.5f} seconds\n"
            ).format(duration=duration),
            component="snapshot",
        )
        # Need to add changed files after they are populated in the working dir
        wctx.add(
            [path for (path, fc) in snapshot["file_changes"] if "Change" in fc],
            quiet=True,
        )

        storelatest(repo.metalog(), csid_bytes, snapshot["bubble_id"])
        duration = time.perf_counter() - start_snapshot
        ui.status(
            _("Restored snapshot in {duration:0.5f} seconds\n").format(
                duration=duration
            ),
            component="snapshot",
        )
