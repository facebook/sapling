# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error, node
from edenscm.mercurial.i18n import _

from .createremote import workingcopy
from .metalog import fetchlatestsnapshot
from .update import fetchsnapshot


def _isworkingcopy(ui, repo, csid):
    """Fails if working copy is not the provided snapshot"""
    snapshot = fetchsnapshot(repo, csid)

    if (
        repo.dirstate.p1() != snapshot["hg_parents"]
        or repo.dirstate.p2() != node.nullid
    ):
        return False, _("parent commits differ")

    wc = workingcopy.fromrepo(repo, None)
    filechanges = snapshot["file_changes"]

    allpaths = {path for (path, _) in filechanges}
    if set(wc.all()) != allpaths:
        diff = set(wc.all()).symmetric_difference(allpaths)
        return False, _("some paths are differently modified: {}").format(
            sorted(diff)[:3]
        )

    incorrectmod = _("'{}' has incorrect modification")
    files2check = []
    for (path, fc) in filechanges:
        if fc == "Deletion":
            if path not in wc.removed:
                return False, incorrectmod.format(path)
        elif fc == "UntrackedDeletion":
            if path not in wc.missing:
                return False, incorrectmod.format(path)
        elif "Change" in fc:
            if path not in wc.added and path not in wc.modified:
                return False, incorrectmod.format(path)
            files2check.append((path, fc["Change"]["upload_token"]))
        elif "UntrackedChange" in fc:
            if path not in wc.untracked:
                return False, incorrectmod.format(path)
            files2check.append((path, fc["UntrackedChange"]["upload_token"]))

    differentfiles = repo.edenapi.checkfiles(repo.root, files2check)
    if differentfiles:
        return False, _("files differ in content: {}").format(
            sorted(differentfiles)[:3]
        )

    return True, ""


def latest(ui, repo, **opts):
    csid = fetchlatestsnapshot(repo.metalog())
    isworkingcopy = opts.get("is_working_copy") is True
    if csid is None:
        if isworkingcopy:
            raise error.Abort(_("latest snapshot not found"))
        if not ui.plain():
            ui.status(_("no snapshot found\n"))
    else:
        csid = csid.hex()
        if isworkingcopy:
            iswc, reason = _isworkingcopy(ui, repo, csid)
            if iswc:
                if not ui.plain():
                    ui.status(_("latest snapshot is the working copy\n"))
            else:
                raise error.Abort(
                    _("latest snapshot is not the working copy: {}").format(reason)
                )
        else:
            if ui.plain():
                ui.status(f"{csid}\n")
            else:
                ui.status(_("latest snapshot is {}\n").format(csid))
