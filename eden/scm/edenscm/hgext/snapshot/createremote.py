# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from dataclasses import dataclass
from pathlib import Path

from edenscm.mercurial.edenapi_upload import (
    filetypefromfile,
    parentsfromctx,
    uploadhgchangesets,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.revset import parseage

from .metalog import fetchlatestbubble, storelatest


def _backupcurrentcommit(repo):
    """make sure the current commit is backed up in commitcloud"""
    currentcommit = (repo["."].node(),)
    draftrevs = repo.changelog.torevset(
        repo.dageval(lambda: ancestors(currentcommit) & draft())
    )

    uploadhgchangesets(repo, draftrevs)


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
    def fromrepo(repo, maxuntrackedsize):
        wctx = repo[None]

        def filterlarge(f):
            if maxuntrackedsize is None:
                return True
            else:
                return Path(repo.root, f).stat().st_size <= maxuntrackedsize

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


def createremote(ui, repo, **opts):
    lifetime = _parselifetime(opts)
    maxuntrackedsize = parsemaxuntracked(opts)
    with repo.lock():
        _backupcurrentcommit(repo)

        # Current working context
        wctx = repo[None]

        (time, tz) = wctx.date()

        wc = workingcopy.fromrepo(repo, maxuntrackedsize)
        previousbubble = fetchlatestbubble(repo.metalog())

        response = repo.edenapi.uploadsnapshot(
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
                "hg_parents": parentsfromctx(wctx),
            },
            lifetime,
            previousbubble,
        )

    csid = bytes(response["changeset_token"]["data"]["id"]["BonsaiChangesetId"])
    bubble = response["bubble_id"]

    storelatest(repo.metalog(), csid, bubble)
    csid = csid.hex()

    if ui.plain():
        ui.status(f"{csid}\n")
    else:
        ui.status(_("Snapshot created with id {}\n").format(csid), component="snapshot")
