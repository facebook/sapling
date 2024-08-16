# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json

from .. import cmdutil, context, error, hg, pathutil, scmutil
from ..cmdutil import commitopts, commitopts2
from ..i18n import _
from .cmdtable import command


@command(
    "subtree",
    [],
    _("<copy>"),
)
def subtree(ui, repo, *pats, **opts) -> None:
    """subtree (directory or file) branching in monorepo"""
    raise error.Abort(
        _(
            "you need to specify a subcommand (run with --help to see a list of subcommands)"
        )
    )


subtree_subcmd = subtree.subcommand(
    categories=[
        (
            "Create subtree branching",
            ["copy"],
        ),
    ]
)


@subtree_subcmd(
    "copy|cp",
    [
        (
            "r",
            "rev",
            "",
            _("the commit to copy from"),
            _("REV"),
        ),
        (
            "",
            "from-path",
            [],
            _("the path of source directory or file"),
            _("PATH"),
        ),
        (
            "",
            "to-path",
            [],
            _("the path of dest directory or file"),
            _("PATH"),
        ),
    ]
    + commitopts
    + commitopts2,
    _("[-r REV] --from-path PATH --to-path PATH ..."),
)
def subtree_copy(ui, repo, *args, **opts):
    """create a directory or file branching"""
    copy(ui, repo, *args, **opts)


def copy(ui, repo, *args, **opts):
    with repo.wlock(), repo.lock():
        return _docopy(ui, repo, *args, **opts)


def _docopy(ui, repo, *args, **opts):
    cmdutil.bailifchanged(repo)
    cwd = repo.getcwd()

    from_paths = [pathutil.canonpath(repo.root, cwd, f) for f in opts.get("from_path")]
    to_paths = [pathutil.canonpath(repo.root, cwd, f) for f in opts.get("to_path")]
    if len(from_paths) != len(to_paths):
        raise error.Abort(_("must provide same number of --from-path and --to-path"))

    user = opts.get("user")
    date = opts.get("date")
    text = opts.get("message")

    # if 'rev' is not specificed, copy from the working copy parent
    from_rev = opts.get("rev") or "."
    from_ctx = scmutil.revsingle(repo, from_rev)
    to_ctx = repo["."]

    extra = {}
    extra.update(_gen_branch_info(from_ctx.hex(), from_paths, to_paths))

    summaryfooter = _gen_prepopulated_commit_msg(from_ctx, from_paths, to_paths)
    editform = cmdutil.mergeeditform(repo[None], "subtree.copy")
    editor = cmdutil.getcommiteditor(
        editform=editform, summaryfooter=summaryfooter, **opts
    )

    newctx = context.subtreecopyctx(
        repo,
        from_ctx,
        to_ctx,
        from_paths,
        to_paths,
        text=text,
        user=user,
        date=date,
        extra=extra,
        editor=editor,
    )

    newid = repo.commitctx(newctx)
    hg.update(repo, newid)


def _gen_branch_info(from_commit, from_paths, to_paths):
    # todo: remove the 'test_' prefix when this feature is stable
    key = "test_branch_info"
    value = {
        "v": 1,
        "branches": [
            {
                "from_path": from_path,
                "to_path": to_path,
                "from_commit": from_commit,
            }
            for from_path, to_path in zip(from_paths, to_paths)
        ],
    }
    # compact JSON representation
    str_val = json.dumps(value, separators=(",", ":"))
    return {key: str_val}


def _gen_prepopulated_commit_msg(from_commit, from_paths, to_paths):
    full_commit_hash = from_commit.hex()
    msgs = [f"Subtree copy from {full_commit_hash}"]
    for from_path, to_path in zip(from_paths, to_paths):
        msgs.append(f"  Copied path {from_path} to {to_path}")
    return "\n".join(msgs)
