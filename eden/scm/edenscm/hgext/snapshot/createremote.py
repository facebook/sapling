# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass
from pathlib import Path

from edenscm.mercurial import error, perftrace, util
from edenscm.mercurial.edenapi_upload import filetypefromfile, uploadhgchangesets
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import nullid
from edenscm.mercurial.revset import parseage

from .metalog import fetchlatestbubble, storelatest


@util.timefunction("snapshot_backup_parents", 0, "ui")
def _backupparents(repo, wctx):
    """make sure this commit's ancestors are backed up in commitcloud"""
    parents = (wctx.p1().node(), wctx.p2().node())
    draftrevs = repo.changelog.torevset(
        repo.dageval(lambda: ancestors(parents) & draft())
    )

    (success, failed) = uploadhgchangesets(repo, draftrevs)
    if failed:
        raise error.Abort(
            _("failed to upload ancestors to commit cloud: {}").format(
                [repo[node].hex() for node in failed]
            )
        )


def _parselifetime(opts):
    if opts["lifetime"] != "":
        return parseage(opts["lifetime"])
    else:
        return None


def parsemaxuntracked(opts):
    if opts["max_untracked_size"] != "":
        return int(opts["max_untracked_size"]) * 1000 * 1000
    else:
        return None


def parentsfromwctx(ui, wctx):
    p1 = wctx.p1().node()
    p2 = wctx.p2().node()
    if p2 != nullid and not ui.plain():
        ui.warn(
            _(
                "Conflict snapshots are not yet properly supported, "
                "but provided as best effort.\n"
            )
        )
    if p1 == nullid:
        return None
    return p1


@dataclass(frozen=True)
class workingcopy(object):
    untracked: list
    removed: list
    modified: list
    added: list
    missing: list

    def all(self):
        return self.untracked + self.removed + self.modified + self.added + self.missing

    @staticmethod
    @perftrace.tracefunc("Create working copy")
    def fromrepo(repo, maxuntrackedsize):
        wctx = repo[None]

        def filterlarge(f):
            if maxuntrackedsize is None:
                return True
            else:
                size = Path(repo.root, f).lstat().st_size
                if size <= maxuntrackedsize:
                    return True
                else:
                    if not repo.ui.plain():
                        repo.ui.warn(
                            _(
                                "Not snapshotting '{}' because it is {} bytes large, and max untracked size is {} bytes\n"
                            ).format(f, size, maxuntrackedsize)
                        )
                    return False

        untracked = [f for f in wctx.status(listunknown=True).unknown if filterlarge(f)]
        removed = []
        for f in wctx.removed():
            # If a file is marked as removed but still exists, it means it was hg rm'ed
            # but then new content was written to it, in which case we consider it as
            # untracked changes.
            if wctx[f].exists():
                untracked.append(f)
            else:
                removed.append(f)

        return workingcopy(
            untracked=untracked,
            removed=removed,
            modified=wctx.modified(),
            added=wctx.added(),
            missing=wctx.deleted(),
        )


@util.timefunction("snapshot_upload", 0, "ui")
def uploadsnapshot(
    repo, wctx, wc, time, tz, hgparents, lifetime, previousbubble, reusestorage
):
    return repo.edenapi.uploadsnapshot(
        {
            "files": {
                "root": repo.root,
                "modified": [(f, filetypefromfile(wctx[f])) for f in wc.modified],
                "added": [(f, filetypefromfile(wctx[f])) for f in wc.added],
                "untracked": [(f, filetypefromfile(wctx[f])) for f in wc.untracked],
                "removed": wc.removed,
                "missing": wc.missing,
            },
            "author": wctx.user(),
            "time": int(time),
            "tz": tz,
            "hg_parents": hgparents,
        },
        lifetime,
        previousbubble,
        previousbubble if reusestorage else None,
    )


def createremote(ui, repo, **opts):
    lifetime = _parselifetime(opts)
    maxuntrackedsize = parsemaxuntracked(opts)
    reusestorage = opts.get("reuse_storage") is True
    overrides = {}
    if ui.plain():
        overrides[("ui", "quiet")] = True
    with repo.lock(), ui.configoverride(overrides):
        # Current working context
        wctx = repo[None]

        hgparents = parentsfromwctx(ui, wctx)
        _backupparents(repo, wctx)

        (time, tz) = wctx.date()

        wc = workingcopy.fromrepo(repo, maxuntrackedsize)
        previousbubble = fetchlatestbubble(repo.metalog())

        response = uploadsnapshot(
            repo, wctx, wc, time, tz, hgparents, lifetime, previousbubble, reusestorage
        )

    csid = bytes(response["changeset_token"]["data"]["id"]["BonsaiChangesetId"])
    bubble = response["bubble_id"]

    storelatest(repo, csid, bubble)
    csid = csid.hex()

    if ui.plain():
        ui.status(f"{csid}\n")
    else:
        ui.status(_("Snapshot created with id {}\n").format(csid), component="snapshot")
