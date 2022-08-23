# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error, scmutil
from edenscm.mercurial.cmdutil import changeset_printer, jsonchangeset
from edenscm.mercurial.context import memctx, memfilectx
from edenscm.mercurial.i18n import _
from edenscm.mercurial.util import pickle


def _snapshot2ctx(repo, snapshot):
    """Build a memctx for this snapshot.

    This is not precisely correct as it doesn't differentiate untracked/added
    but it's good enough for diffing.
    """

    parent = snapshot["hg_parents"]
    # Once merges/conflicted states are supported, we'll need to support more
    # than one parent
    assert isinstance(parent, bytes)
    # Fetch parent if not present locally
    if parent not in repo:
        repo.pull(headnodes=(parent,))

    parents = [repo[parent]]
    path2filechange = {f[0]: f[1] for f in snapshot["file_changes"]}

    def token2cacheable(token):
        data = token["data"]
        return pickle.dumps((data["id"], data["bubble_id"]))

    cache = {}

    def getfile(repo, memctx, path):
        change = path2filechange.get(path)
        if change is None:
            return repo[parent][path]
        if change == "Deletion" or change == "UntrackedDeletion":
            return None
        elif "Change" in change or "UntrackedChange" in change:
            change = change.get("Change") or change["UntrackedChange"]
            token = change["upload_token"]
            key = token2cacheable(token)
            if key not in cache:
                # Possible future optimisation: Download files in parallel
                cache[key] = repo.edenapi.downloadfiletomemory(token)
            islink = change["file_type"] == "Symlink"
            isexec = change["file_type"] == "Executable"
            return memfilectx(
                repo, None, path, data=cache[key], islink=islink, isexec=isexec
            )
        else:
            raise error.Abort(_("Unknown file change {}").format(change))

    time, tz = snapshot["time"], snapshot["tz"]
    if time or tz:
        date = (time, tz)
    else:
        date = None

    ctx = memctx(
        repo,
        parents,
        text="",
        files=list(path2filechange.keys()),
        filectxfn=getfile,
        user=snapshot["author"] or None,
        date=date,
    )
    return ctx


def show(ui, repo, csid=None, **opts):
    if csid is None:
        raise error.CommandError("snapshot show", _("missing snapshot id"))
    try:
        snapshot = repo.edenapi.fetchsnapshot(
            {
                "cs_id": bytes.fromhex(csid),
            },
        )
    except Exception:
        raise error.Abort(_("snapshot doesn't exist"))
    else:
        ctx = _snapshot2ctx(repo, snapshot)
        match = scmutil.matchall(repo)
        printeropt = {"patch": not opts["stat"], "stat": opts["stat"]}
        buffered = False
        if opts["json"] is True:
            displayer = jsonchangeset(ui, repo, match, printeropt, buffered)
        else:
            ui.status(_("snapshot: {}\n").format(csid))
            displayer = changeset_printer(ui, repo, match, printeropt, buffered)
        displayer.show(ctx)
        displayer.close()
