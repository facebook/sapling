# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from sapling import (
    error,
    filemerge,
    json,
    match as matchmod,
    merge as mergemod,
    simplemerge,
)
from sapling.commands.cmdtable import command
from sapling.i18n import _
from sapling.node import bin


@command(
    "debugconflictcontext",
    [
        (
            "",
            "commit-search-limit",
            10,
            _(
                "max number of commits touching conflicted file to search when looking for conflicting commit"
            ),
        ),
        ("", "max-diff-size", 10240, _("max byte size of diffs")),
    ],
)
def debugconflictcontext(ui, repo, **opts):
    """dump useful context about the currently conflicting file(s)

    This command expects the working copy to be in an unresolved merge state.

    On success, prints JSON information like::

        [{
            "file": <conflicting file path>
            # If we can guess the commit that introduced the conflicting content on the "local" side (or "dest" when rebasing):
            "conflicting_local": {"description": <commit message>, "diff": <diff string>, "hash": <commit hash>},
            # Info about the "other" (or "source" when rebasing) commit:
            "conflicting_other": {"description": <commit message>, "diff": <diff string>, "hash": <commit hash>},
        }]
    """

    ms = mergemod.mergestate.read(repo)

    if not ms.active():
        raise error.Abort(_("no active merge state"))

    unresolved = ms.unresolved()
    if not unresolved:
        raise error.Abort(_("no unresolved files"))

    contexts = []
    for f in unresolved:
        context = {"file": f}
        contexts.append(context)

        conflicting_local = _guess_conflicting_commit(ui, opts, repo, ms, f)
        if conflicting_local:
            context["conflicting_local"] = {
                "hash": conflicting_local.hex(),
                "description": conflicting_local.description(),
                "diff": _limited_diff(repo, opts, conflicting_local, f),
            }

        # Assumes rebase where otherctx will be the conflicting commit on that side.
        context["conflicting_other"] = {
            "hash": ms.otherctx.hex(),
            "description": ms.otherctx.description(),
            "diff": _limited_diff(repo, opts, ms.otherctx, f),
        }

    ui.write("%s\n" % json.dumps(contexts))


def _guess_conflicting_commit(ui, opts, repo, ms, f):
    _state, _hexdnode, _lfile, afile, hexanode, ofile, _hexonode, _flags = (
        ms._rust_ms.get(f)
    )

    wctx = repo[None]

    if f not in wctx:
        ui.note_err(_("local file %s not in wctx?\n") % (f,))
        return None

    fcd = wctx[f]

    if acommitnode := ms.extras(f).get("ancestorlinknode"):
        fca = repo.filectx(afile, fileid=bin(hexanode), changeid=repo[acommitnode])
    else:
        ui.note_err(_("file %s has no ancestorlinknode\n") % (f,))
        return None

    if ofile not in ms.otherctx:
        ui.note_err(_("other file %s not in otherctx %s\n") % (ofile, ms.otherctx))
        return None

    fco = ms.otherctx[ofile]

    # Use pathhistory to loop through commits that touched our file. We try to find the commit that
    # first introduced a conflict as our most likely culprit.

    prev_conflicting = None
    candidates, max_hit = filemerge._findconflictingcommits(
        repo, fcd, fca, maxcommits=opts.get("commit_search_limit")
    )

    for maybe_conflicting in candidates:
        if f not in maybe_conflicting:
            ui.note_err(
                _("local file %s not in ancestor %s\n") % (f, maybe_conflicting)
            )
            return None

        _content, conflicts_count = simplemerge.render_minimized(
            simplemerge.Merge3Text(
                fca.data(),
                maybe_conflicting[f].data(),
                fco.data(),
            )
        )

        if conflicts_count > 0:
            prev_conflicting = maybe_conflicting
        else:
            # We no longer have conflicting content - culprit must be the previous conflicter.
            return prev_conflicting

    if max_hit:
        # If there were more candidates then we aren't sure who the culprit is.
        return None
    else:
        # This was the last candidate - it probably introduced the conflict.
        return prev_conflicting


def _limited_diff(repo, opts, ctx, f):
    # Try full diff of ctx. If too large, try diff limited to just f.
    limit = opts.get("max_diff_size")

    diff = b""
    for item in ctx.diff():
        diff += item
        if len(diff) > limit:
            diff = b"".join(
                list(ctx.diff(match=matchmod.match(repo.root, "", [f], default="path")))
            )
            if len(diff) > limit:
                diff = b""
            break

    return diff.decode(errors="backslashreplace")
