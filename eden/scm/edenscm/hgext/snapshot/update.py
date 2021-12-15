# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import hg, scmutil, cmdutil, error
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

        repo.edenapi.downloadfiles(repo.root, files2download)

        # Need to add changed files after they are populated in the working dir
        wctx.add(
            [path for (path, fc) in snapshot["file_changes"] if "Change" in fc],
            quiet=True,
        )

        for (path, filetype) in pathtype:
            if filetype == "Executable":
                if not wctx[path].isexec():
                    wctx[path].setflags(l=False, x=True)
            else:
                if wctx[path].isexec():
                    wctx[path].setflags(l=False, x=False)

        # TODO(yancouto): Also update bubble here, need to get it from server
        storelatest(repo.metalog(), csid, None)
