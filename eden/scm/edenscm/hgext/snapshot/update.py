# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import hg, scmutil, cmdutil, error
from edenscm.mercurial.edenapi_upload import (
    getreponame,
)
from edenscm.mercurial.i18n import _

from .metalog import storelatest


def _hasanychanges(repo):
    wctx = repo[None]
    return (
        bool(wctx.dirty(missing=True)) or len(wctx.status(listunknown=True).unknown) > 0
    )


def _fullclean(ui, repo, exclude):
    ui.status(_("cleaning up uncommitted code\n"), component="snapshot")
    # Remove "tracked changes"
    cmdutil.revert(
        ui,
        repo,
        scmutil.revsingle(repo, None),
        repo.dirstate.parents(),
        all=True,
        no_backup=True,
        exclude=exclude,
    )
    # Remove "untracked changes" (e.g. untracked files)
    repo.dirstate._fs.purge(
        scmutil.match(repo[None], opts={"exclude": exclude}),
        removefiles=True,
        removedirs=True,
        removeignored=False,
        dryrun=False,
    )


def fetchsnapshot(repo, csid):
    return repo.edenapi.fetchsnapshot(
        getreponame(repo),
        {
            "cs_id": csid,
        },
    )


def update(ui, repo, csid, clean=False):
    ui.status(_("Will restore snapshot {}\n").format(csid), component="snapshot")
    csid = bytes.fromhex(csid)

    snapshot = fetchsnapshot(repo, csid)

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
            ui.status(
                _("Updating to parent {}\n").format(parent.hex()),
                component="snapshot",
            )

            # This will resolve the parent revision even if it's not available locally
            # and needs pulling from server.
            if parent not in repo:
                repo.pull(headnodes=(parent,))

            hg.updatetotally(ui, repo, parent, None, clean=False, updatecheck="abort")
        else:
            if haschanges:
                # We might be able to reuse files that were already downloaded locally,
                # so let's not delete files related to the snapshot
                _fullclean(ui, repo, [path for (path, _) in snapshot["file_changes"]])

        files2download = []

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
                # Untracked deletion means the file was tracked, so add it, in case it
                # was hg added and then deleted
                # Using dirstate directly so we don't need to create a dummy file
                if repo.dirstate[path] == "?":
                    repo.dirstate.add(path)
                if fctx.exists():
                    fctx.remove()
            elif "Change" in fc:
                files2download.append((path, fc["Change"]["upload_token"]))
            elif "UntrackedChange" in fc:
                wctx.forget([path], quiet=True)
                files2download.append((path, fc["UntrackedChange"]["upload_token"]))

        repo.edenapi.downloadfiles(getreponame(repo), repo.root, files2download)

        # Need to add changed files after they are populated in the working dir
        wctx.add(
            [path for (path, fc) in snapshot["file_changes"] if "Change" in fc],
            quiet=True,
        )

        # TODO(yancouto): Also update bubble here, need to get it from server
        storelatest(repo.metalog(), csid, None)
