# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import hg, scmutil, cmdutil, error
from edenscm.mercurial.edenapi_upload import (
    getreponame,
)
from edenscm.mercurial.i18n import _


def _hasanychanges(repo):
    wctx = repo[None]
    return (
        bool(wctx.dirty(missing=True)) or len(wctx.status(listunknown=True).unknown) > 0
    )


def _fullclean(ui, repo):
    ui.status(_("cleaning up uncommitted code\n"), component="snapshot")
    # Remove "tracked changes"
    cmdutil.revert(
        ui,
        repo,
        scmutil.revsingle(repo, None),
        repo.dirstate.parents(),
        all=True,
        no_backup=True,
    )
    # Remove "untracked changes" (e.g. untracked files)
    repo.dirstate._fs.purge(
        scmutil.match(repo[None]),
        removefiles=True,
        removedirs=True,
        removeignored=False,
        dryrun=False,
    )


def restore(ui, repo, csid, clean=False):
    ui.status(_(f"Will restore snapshot {csid}\n"), component="snapshot")

    snapshot = repo.edenapi.fetchsnapshot(
        getreponame(repo),
        {
            "cs_id": bytes.fromhex(csid),
        },
    )

    # Once merges/conflicted states are supported, we'll need to support more
    # than one parent
    assert isinstance(snapshot["hg_parents"], bytes)

    with repo.wlock():
        if _hasanychanges(repo):
            if clean:
                _fullclean(ui, repo)
            else:
                raise error.Abort(
                    _(
                        "Can't restore snapshot with unclean working copy, unless --clean is specified"
                    )
                )

        ui.status(
            _(f"Updating to parent {snapshot['hg_parents'].hex()}\n"),
            component="snapshot",
        )

        hg.updatetotally(
            ui, repo, repo[snapshot["hg_parents"]], None, updatecheck="abort"
        )

        files2download = []
        files2hgadd = []

        for (path, fc) in snapshot["file_changes"]:
            matcher = scmutil.matchfiles(repo, [path])
            fctx = repo[None][path]
            # fc is either a string or a dict, can't use "Deletion" in fc because
            # that applies to "UntrackedDeletion" as well
            if fc == "Deletion":
                cmdutil.remove(ui, repo, matcher, "", False, False)
            elif fc == "UntrackedDeletion":
                if not fctx.exists():
                    # File was hg added and is now missing. Let's add an empty file first
                    repo.wwrite(path, b"", "")
                    cmdutil.add(ui, repo, matcher, prefix="", explicitonly=True)
                fctx.remove()
            elif "Change" in fc:
                if fctx.exists():
                    # File exists, was modified
                    fctx.remove()
                else:
                    # File was created and added
                    files2hgadd.append(path)
                files2download.append((path, fc["Change"]["upload_token"]))
            elif "UntrackedChange" in fc:
                if fctx.exists():
                    # File was hg rm'ed and then overwritten
                    cmdutil.remove(
                        ui, repo, matcher, prefix="", after=False, force=False
                    )
                files2download.append((path, fc["UntrackedChange"]["upload_token"]))

        repo.edenapi.downloadfiles(getreponame(repo), repo.root, files2download)

        for path in files2hgadd:
            cmdutil.add(ui, repo, scmutil.matchfiles(repo, [path]), "", True)
