# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import hg, scmutil, cmdutil
from edenscm.mercurial.edenapi_upload import (
    getreponame,
)


def restore(ui, repo, csid, **opts):
    ui.status(f"Will restore snapshot {csid}\n", component="snapshot")

    snapshot = repo.edenapi.fetchsnapshot(
        getreponame(repo),
        {
            "cs_id": bytes.fromhex(csid),
            # TODO(yancouto): Find bubble id from snapshot id
            "bubble_id": 1,
        },
    )

    # Once merges/conflicted states are supported, we'll need to support more
    # than one parent
    assert isinstance(snapshot["hg_parents"], bytes)

    ui.status(
        f"Updating to parent {snapshot['hg_parents'].hex()}\n", component="snapshot"
    )

    with repo.wlock():
        hg.updatetotally(
            ui, repo, repo[snapshot["hg_parents"]], None, updatecheck="abort"
        )

        files2download = []

        for (path, fc) in snapshot["file_changes"]:
            matcher = scmutil.matchfiles(repo, [path])
            fctx = repo[None][path]
            if "Deletion" in fc:
                cmdutil.remove(ui, repo, matcher, "", False, False)
            elif "UntrackedDeletion" in fc:
                if not fctx.exists():
                    # File was hg added and is now missing. Let's add an empty file first
                    repo.wwrite(path, b"", "")
                    cmdutil.add(ui, repo, matcher, prefix="", explicitonly=True)
                fctx.remove()
            elif "Change" in fc:
                if fctx.exists():
                    # File exists, was modified
                    fctx.remove()
                files2download.append((path, fc["Change"]["upload_token"]))
            elif "UntrackedChange" in fc:
                if fctx.exists():
                    # File was hg rm'ed and then overwritten
                    cmdutil.remove(
                        ui, repo, matcher, prefix="", after=False, force=False
                    )
                files2download.append((path, fc["UntrackedChange"]["upload_token"]))

        repo.edenapi.downloadfiles(getreponame(repo), repo.root, files2download)

        for (path, fc) in snapshot["file_changes"]:
            if "Change" in fc:
                # Doesn't hurt to add again if it was already tracked
                cmdutil.add(ui, repo, scmutil.matchfiles(repo, [path]), "", True)
