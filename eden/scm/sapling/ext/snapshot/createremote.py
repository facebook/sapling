# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import time as mtime
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from sapling import error, perftrace, util
from sapling.edenapi_upload import filetypefromfile, uploadhgchangesets
from sapling.i18n import _
from sapling.node import nullid
from sapling.revset import parseage

from .metalog import (
    fetchlatestbubble,
    fetchlatestsnapshot,
    getcsidbubblemapping,
    SnapshotMetadata,
    storelatest,
    storesnapshotmetadata,
)

from .update import fetchsnapshot


@util.timefunction("snapshot_backup_parents", 0, "ui")
def _backupparents(repo, wctx) -> None:
    """make sure this commit's ancestors are backed up in commitcloud"""
    parents = (wctx.p1().node(), wctx.p2().node())
    # pyre-fixme[10]: Name `ancestors` is used but not defined.
    # pyre-fixme[10]: Name `draft` is used but not defined.
    draftnodes = repo.dageval(lambda: draft() & ancestors(parents))

    (success, failed) = uploadhgchangesets(repo, draftnodes)
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


def parselabels(opts):
    if opts["labels"] != "":
        return [label.strip() for label in opts["labels"].split(",") if label.strip()]
    else:
        return None


def parsemaxuntracked(opts) -> Optional[int]:
    if opts["max_untracked_size"] != "":
        return int(opts["max_untracked_size"]) * 1000 * 1000
    else:
        return None


def parsemaxfilecount(opts):
    if opts["max_file_count"] != "":
        return opts["max_file_count"]
    else:
        return None


def parsecontinuationof(opts, repo):
    """Parse the continuation-of option to get the previous snapshot ID"""
    continuation_value = opts.get("continuation_of", "")
    if not continuation_value:
        return None

    if continuation_value == "latest":
        # Get the latest snapshot ID from metalog
        latest_snapshot = fetchlatestsnapshot(repo.metalog())
        if latest_snapshot is None:
            raise error.Abort(_("no latest snapshot found to continue from"))
        return latest_snapshot.hex()
    else:
        # Validate the hash format (should be 64 character hex string for bonsai hash)
        if len(continuation_value) != 64:
            raise error.Abort(
                _(
                    "invalid snapshot id format: expected 64 character hex string, got {}"
                ).format(len(continuation_value))
            )
        try:
            int(continuation_value, 16)
        except ValueError:
            raise error.Abort(
                _("invalid snapshot id format: '{}' is not a valid hex string").format(
                    continuation_value
                )
            )
        return continuation_value


def _fetchremotebubble(repo, cs_id_bytes):
    """Fetch bubble ID for a changeset from the remote server"""
    try:
        response = fetchsnapshot(repo, cs_id_bytes)
        bubble_id = response.get("bubble_id")
        if bubble_id is not None:
            return bubble_id
        else:
            return None

    except Exception as e:
        # Log the error but don't fail the operation
        repo.ui.debug(
            _("failed to fetch remote bubble for {}: {}\n").format(
                cs_id_bytes.hex(), str(e)
            )
        )
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
class workingcopy:
    untracked: list
    removed: list
    modified: list
    added: list
    missing: list

    def all(self):
        return self.untracked + self.removed + self.modified + self.added + self.missing

    def filecount(self):
        return (
            len(self.untracked)
            + len(self.removed)
            + len(self.modified)
            + len(self.added)
            + len(self.missing)
        )

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
    repo, wctx, wc, time, tz, hgparents, lifetime, previousbubble, reusestorage, labels
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
        labels,
    )


def createremote(ui, repo, **opts) -> None:
    lifetime = _parselifetime(opts)
    maxuntrackedsize = parsemaxuntracked(opts)
    maxfilecount = parsemaxfilecount(opts)
    reusestorage = opts.get("reuse_storage") is True
    labels = parselabels(opts)
    continuationof = parsecontinuationof(opts, repo)
    skipempty = opts.get("skip_empty") is True

    # Validate that --continuation-of and --reuse-storage are not used together
    if continuationof and reusestorage:
        raise error.Abort(
            _(
                "--continuation-of cannot be used with --reuse-storage (legacy option that does not include TTL extension)"
            )
        )

    overrides = {}
    if ui.plain() or opts.get("template"):
        overrides[("ui", "quiet")] = True
    with repo.wlock(), repo.lock(), repo.transaction("snapshot"), ui.configoverride(
        overrides
    ):
        # Current working context
        wctx = repo[None]

        hgparents = parentsfromwctx(ui, wctx)
        if hgparents is None:
            raise error.Abort(_("snapshot creation requires working copy checkout"))

        # Always backup parents first
        _backupparents(repo, wctx)

        # Get working copy state
        wc = workingcopy.fromrepo(repo, maxuntrackedsize)
        filecount = wc.filecount()

        # Check for --skip-empty option and handle empty working copy
        if skipempty and filecount == 0:
            parent_hex = hgparents.hex()

            # Handle JSON output if template is specified
            if opts.get("template"):
                with ui.formatter("snapshot", opts) as fm:
                    fm.startitem()
                    fm.data(message="no changes")
                    fm.data(parent=parent_hex)
                    if not ui.quiet and not ui.plain():
                        fm.plain(
                            _("nothing to snapshot, parent commit is {}\n").format(
                                parent_hex
                            )
                        )
            else:
                ui.status(
                    _("nothing to snapshot, parent commit is {}\n").format(parent_hex),
                    component="snapshot",
                )
            return

        if filecount > maxfilecount:
            raise error.AbortSnapshotFileCountLimit(
                _(
                    "snapshot file count limit exceeded: file count is {}, limit is {}"
                ).format(filecount, maxfilecount)
            )

        # Handle continuation-of option
        previousbubble = None
        if continuationof:
            # Look up bubble ID from the previous snapshot
            previousbubble = getcsidbubblemapping(repo.metalog(), continuationof)
            if previousbubble is None:
                # Try remote lookup for bubble ID if not found locally
                ui.status(
                    _("bubble not found locally for {}, trying remote lookup\n").format(
                        continuationof
                    ),
                    component="snapshot",
                )
                continuationof_bytes = bytes.fromhex(continuationof)
                previousbubble = _fetchremotebubble(repo, continuationof_bytes)

                if previousbubble is None:
                    # Still not found, raise an error
                    raise error.Abort(
                        _(
                            "cannot find bubble for previous snapshot {} (checked both local and remote)"
                        ).format(continuationof)
                    )
                else:
                    # Cache the remote result locally for future use
                    with repo.transaction("snapshot_cache_bubble"):
                        metadata = SnapshotMetadata(
                            bubble=previousbubble, created_at=mtime.time()
                        )
                        storesnapshotmetadata(repo, continuationof_bytes, metadata)
                    ui.status(
                        _("found bubble {} for {} via remote lookup\n").format(
                            previousbubble, continuationof
                        ),
                        component="snapshot",
                    )

            # The storage must be reused
            reusestorage = True
            ui.status(
                _("continuing from snapshot {} using bubble {}\n").format(
                    continuationof, previousbubble
                ),
                component="snapshot",
            )
        else:
            previousbubble = fetchlatestbubble(repo.metalog())

        (time, tz) = wctx.date()
        response = uploadsnapshot(
            repo,
            wctx,
            wc,
            time,
            tz,
            hgparents,
            lifetime,
            previousbubble,
            reusestorage,
            labels,
        )

        csid = bytes(response["changeset_token"]["data"]["id"]["BonsaiChangesetId"])
        bubble = response["bubble_id"]

        # Store latest snapshot and bubble mapping in the local cache
        storelatest(repo, csid, bubble)

        # Store metadata for this snapshot in the local cache
        metadata = SnapshotMetadata(bubble=bubble, created_at=mtime.time())
        storesnapshotmetadata(repo, csid, metadata)

    csid = csid.hex()

    # Use formatter for JSON output support
    if opts.get("template"):
        with ui.formatter("snapshot", opts) as fm:
            fm.startitem()
            fm.data(id=csid)
            fm.data(bubble=bubble)
            # the variable actually always holds p1
            parent = hgparents.hex()
            fm.data(parent=parent)
            if labels:
                fm.data(labels=labels)

            # For non-JSON output, provide the traditional format
            if not ui.quiet:
                if ui.plain():
                    fm.plain(f"{csid}\n")
                elif labels:
                    labels_str = ",".join(labels)
                    fm.plain(
                        _(
                            "snapshot: Snapshot created with id {} and labels {}\n"
                        ).format(csid, labels_str)
                    )
                else:
                    fm.plain(_("snapshot: Snapshot created with id {}\n").format(csid))
    else:
        if ui.plain():
            ui.status(f"{csid}\n")
        elif labels:
            labels = ",".join(labels)
            ui.status(
                _("Snapshot created with id {} and labels {}\n").format(csid, labels),
                component="snapshot",
            )
        else:
            ui.status(
                _("Snapshot created with id {}\n").format(csid), component="snapshot"
            )
