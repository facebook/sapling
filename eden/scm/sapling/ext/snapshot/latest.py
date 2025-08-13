# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from sapling import error, node
from sapling.edenapi_upload import filetypefromfile
from sapling.i18n import _

from .createremote import (
    getdefaultmaxuntrackedsize,
    parsemaxuntracked,
    parsemaxuntrackedbytes,
    workingcopy,
)
from .metalog import fetchlatestsnapshot
from .update import fetchsnapshot


def _isworkingcopy(ui, repo, snapshot, maxuntrackedsize, pats=None, opts=None):
    """Fails if working copy is not the provided snapshot"""
    from sapling import scmutil

    if pats is None:
        pats = []
    if opts is None:
        opts = {}

    if (
        repo.dirstate.p1() != snapshot["hg_parents"]
        or repo.dirstate.p2() != node.nullid
    ):
        return False, _("parent commits differ"), None

    wc = workingcopy.fromrepo(repo, maxuntrackedsize, pats, opts)
    filechanges = snapshot["file_changes"]

    # Apply the same pattern filtering to snapshot paths
    wctx = repo[None]
    matcher = scmutil.match(wctx, pats, opts)
    filtered_filechanges = [(path, fc) for (path, fc) in filechanges if matcher(path)]

    # Log excluded files (from the snapshot side) for user awareness
    all_snapshot_paths = {path for (path, _) in filechanges}
    filtered_snapshot_paths = {path for (path, _) in filtered_filechanges}
    excluded_files = all_snapshot_paths - filtered_snapshot_paths

    if excluded_files and not ui.plain():
        ui.status(
            _(
                "snapshot has {} file(s) that are excluded due to the provided filters\n"
            ).format(len(excluded_files)),
            component="snapshot",
        )

    allpaths = {path for (path, _) in filtered_filechanges}

    if set(wc.all()) != allpaths:
        diff = set(wc.all()).symmetric_difference(allpaths)
        return (
            False,
            _("some paths are differently modified: {}").format(sorted(diff)[:3]),
            wc,
        )

    incorrectmod = _("'{}' has incorrect modification")
    incorrectfiletype = _("'{}' has incorrect file type")
    files2check = []
    for path, fc in filtered_filechanges:
        if fc == "Deletion":
            if path not in wc.removed:
                return False, incorrectmod.format(path), wc
        elif fc == "UntrackedDeletion":
            if path not in wc.missing:
                return False, incorrectmod.format(path), wc
        elif "Change" in fc:
            if path not in wc.added and path not in wc.modified:
                return False, incorrectmod.format(path), wc
            filetype = fc["Change"]["file_type"]
            if filetype != filetypefromfile(wctx[path]):
                return False, incorrectfiletype.format(path), wc
            files2check.append((path, fc["Change"]["upload_token"], filetype))
        elif "UntrackedChange" in fc:
            if path not in wc.untracked:
                return False, incorrectmod.format(path), wc
            filetype = fc["UntrackedChange"]["file_type"]
            if filetype != filetypefromfile(wctx[path]):
                return False, incorrectfiletype.format(path), wc
            files2check.append(
                (
                    path,
                    fc["UntrackedChange"]["upload_token"],
                    filetype,
                )
            )

    differentfiles = repo.edenapi.checkfiles(repo.root, files2check)
    if differentfiles:
        return (
            False,
            _("files differ in content: {}").format(sorted(differentfiles)[:3]),
            wc,
        )

    return True, "", wc


def latest(ui, repo, **opts):
    csid = fetchlatestsnapshot(repo.metalog())
    isworkingcopy = opts.get("is_working_copy") is True
    maxuntrackedsize = parsemaxuntracked(opts)
    maxuntrackedsizebytes = parsemaxuntrackedbytes(opts)

    if (
        maxuntrackedsizebytes or maxuntrackedsize
    ) is not None and isworkingcopy is False:
        raise error.Abort(
            _(
                "--max-untracked-size/--max-untracked-size-bytes can only be used together with --is-working-copy"
            )
        )

    # Use bytes-based limit if specified, otherwise fall back to MiB-based limit, then config default
    effective_max_untracked_size = (
        maxuntrackedsizebytes or maxuntrackedsize or getdefaultmaxuntrackedsize(ui)
    )

    if csid is None:
        if isworkingcopy:
            raise error.Abort(_("latest snapshot not found"))
        if not ui.plain():
            ui.status(_("no snapshot found\n"))
    else:
        if isworkingcopy:
            snapshot = fetchsnapshot(repo, csid)
            iswc, reason, _wc = _isworkingcopy(
                ui, repo, snapshot, effective_max_untracked_size, [], {}
            )
            if iswc:
                if not ui.plain():
                    ui.status(_("latest snapshot is the working copy\n"))
            else:
                raise error.Abort(
                    _("latest snapshot is not the working copy: {}").format(reason)
                )
        else:
            csid = csid.hex()
            if ui.plain():
                ui.status(f"{csid}\n")
            else:
                ui.status(_("latest snapshot is {}\n").format(csid))
