# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json

from .. import cmdutil, context, error, hg, scmutil

from ..cmdutil import commitopts, commitopts2, diffgraftopts, dryrunopts, mergetoolopts
from ..i18n import _
from .cmdtable import command


# todo: remove the 'test_' prefix when this feature is stable
SUBTREE_BRANCH_INFO_KEY = "test_branch_info"
SUBTREE_MERGE_INFO_KEY = "test_subtree_merge_info"


@command(
    "subtree",
    [],
    _("<copy|graft>"),
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


@subtree_subcmd(
    "graft",
    [
        ("r", "rev", [], _("revisions to graft"), _("REV")),
        ("c", "continue", False, _("resume interrupted graft")),
        ("", "abort", False, _("abort an interrupted graft")),
        ("e", "edit", False, _("invoke editor on commit messages")),
        ("", "log", None, _("append graft info to log message")),
        ("f", "force", False, _("force graft")),
        ("D", "currentdate", False, _("record the current date as commit date")),
        (
            "U",
            "currentuser",
            False,
            _("record the current user as committer"),
        ),
    ]
    + commitopts2
    + mergetoolopts
    + dryrunopts
    + diffgraftopts,
    _("[OPTION]... --from-path PATH --to-path PATH ..."),
)
def subtree_graft(ui, repo, **opts):
    """move commits from one path to another"""
    from sapling.commands import _dograft

    from_paths = opts.get("from_path")
    to_paths = opts.get("to_path")
    if not (opts.get("continue") or opts.get("abort")):
        if not (from_paths and to_paths):
            raise error.Abort(_("must provide --from-path and --to-path"))

    with repo.wlock():
        return _dograft(ui, repo, **opts)


def copy(ui, repo, *args, **opts):
    with repo.wlock(), repo.lock():
        return _docopy(ui, repo, *args, **opts)


def _docopy(ui, repo, *args, **opts):
    cmdutil.bailifchanged(repo)

    # if 'rev' is not specificed, copy from the working copy parent
    from_rev = opts.get("rev") or "."
    from_ctx = scmutil.revsingle(repo, from_rev)
    to_ctx = repo["."]

    from_paths = scmutil.rootrelpaths(from_ctx, opts.get("from_path"))
    to_paths = scmutil.rootrelpaths(from_ctx, opts.get("to_path"))
    scmutil.validate_path_size(from_paths, to_paths, abort_on_empty=True)
    scmutil.validate_path_exist(ui, from_ctx, from_paths, abort_on_missing=True)
    scmutil.validate_path_overlap(to_paths)

    user = opts.get("user")
    date = opts.get("date")
    text = opts.get("message")

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
    return {SUBTREE_BRANCH_INFO_KEY: str_val}


def _gen_prepopulated_commit_msg(from_commit, from_paths, to_paths):
    full_commit_hash = from_commit.hex()
    msgs = [f"Subtree copy from {full_commit_hash}"]
    for from_path, to_path in zip(from_paths, to_paths):
        msgs.append(f"- Copied path {from_path} to {to_path}")
    return "\n".join(msgs)


def get_branch_info(repo, node):
    return _get_subtree_metadata(repo, node, SUBTREE_BRANCH_INFO_KEY)


def get_merge_info(repo, node):
    return _get_subtree_metadata(repo, node, SUBTREE_MERGE_INFO_KEY)


def _get_subtree_metadata(repo, node, key):
    extra = repo[node].extra()
    try:
        val_str = extra[key]
    except KeyError:
        return None
    try:
        return json.loads(val_str)
    except json.JSONDecodeError:
        raise error.Abort(f"invalid {key} metadata: {val_str}")
