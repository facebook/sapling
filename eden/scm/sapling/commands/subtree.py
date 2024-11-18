# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from collections import defaultdict

from .. import (
    cmdutil,
    context,
    error,
    hg,
    match as matchmod,
    merge as mergemod,
    node,
    pathutil,
    progress,
    scmutil,
)
from ..cmdutil import (
    commitopts,
    commitopts2,
    dryrunopts,
    mergetoolopts,
    subtree_path_opts,
)
from ..i18n import _
from ..utils import subtreeutil
from ..utils.subtreeutil import (
    BranchType,
    gen_branch_info,
    get_subtree_branches,
    get_subtree_merges,
)
from .cmdtable import command


MAX_SUBTREE_COPY_FILE_COUNT = 10_000


@command(
    "subtree",
    [],
    _("<copy|graft|merge>"),
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
        ("f", "force", None, _("forcibly copy over an existing file")),
    ]
    + subtree_path_opts
    + commitopts
    + commitopts2,
    _("[-r REV] --from-path PATH --to-path PATH ..."),
)
def subtree_copy(ui, repo, *args, **opts):
    """create a directory or file branching"""
    with repo.wlock(), repo.lock():
        return _docopy(ui, repo, *args, **opts)


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
    + commitopts
    + commitopts2
    + mergetoolopts
    + dryrunopts
    + subtree_path_opts,
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


@subtree_subcmd(
    "merge",
    [
        ("r", "rev", "", _("revisions to merge"), _("REV")),
    ]
    + mergetoolopts
    + subtree_path_opts,
    _("[OPTION]... --from-path PATH --to-path PATH"),
)
def subtree_merge(ui, repo, **opts):
    """merge a path of the specified commit into a different path of the current commit"""
    ctx = repo["."]
    from_ctx = scmutil.revsingle(repo, opts.get("rev"))
    from_paths = scmutil.rootrelpaths(ctx, opts.get("from_path"))
    to_paths = scmutil.rootrelpaths(ctx, opts.get("to_path"))

    if len(from_paths) != 1 or len(to_paths) != 1:
        raise error.Abort(_("must provide exactly one --from-path and --to-path"))
    subtreeutil.validate_path_overlap(from_paths, to_paths)
    subtreeutil.validate_path_exist(ui, from_ctx, from_paths, abort_on_missing=True)
    subtreeutil.validate_path_exist(ui, ctx, to_paths, abort_on_missing=True)
    subtreeutil.validate_source_commit(repo.ui, from_ctx, "merge")

    merge_base_ctx = _subtree_merge_base(
        repo, ctx, to_paths[0], from_ctx, from_paths[0]
    )
    ui.status("merge base: %s\n" % merge_base_ctx)
    cmdutil.registerdiffgrafts(from_paths, to_paths, ctx, from_ctx)

    with ui.configoverride(
        {("ui", "forcemerge"): opts.get("tool", "")}, "subtree_merge"
    ):
        labels = ["working copy", "merge rev"]
        stats = mergemod.merge(
            repo,
            from_ctx,
            force=False,
            ancestor=merge_base_ctx,
            mergeancestor=False,
            labels=labels,
        )
        hg.showstats(repo, stats)
        if stats[3]:
            repo.ui.status(
                _(
                    "use '@prog@ resolve' to retry unresolved file merges "
                    "or '@prog@ goto -C .' to abandon\n"
                )
            )
        else:
            repo.ui.status(_("(subtree merge, don't forget to commit)\n"))
        return stats[3] > 0


def _subtree_merge_base(repo, to_ctx, to_path, from_ctx, from_path):
    """get the best merge base for subtree merge

    There are two major use cases for subtree merge:
    1. merge a dev branch (original copy-to directory) to main branch
    2. merge a main branch to release branch (original copy-to directory)

    High level idea of the aglorithm:
    1. try to find the last subtree merge point
    2. try to find the original subtree copy info
    3. otherwise, fallback to the parent commit of the creation commit

    The return value is the context of merge base commit with registered
    path mapping.
    """

    def registerdiffgrafts(merge_base_ctx, heads_index):
        # if the head index is 0, then it points to to_paths, which means
        # the merge direction matches the original copy direction, otherwise
        # it is a reverse merge
        if heads_index == 0:
            cmdutil.registerdiffgrafts([from_path], [to_path], merge_base_ctx)
        else:
            cmdutil.registerdiffgrafts([to_path], [from_path], merge_base_ctx)
        return merge_base_ctx

    dag = repo.changelog.dag
    if from_path == to_path:
        nodes = [from_ctx.node(), to_ctx.node()]
        gca = dag.gcaone(nodes)
        return registerdiffgrafts(repo[gca], 0)

    isancestor = dag.isancestor
    to_hist = repo.pathhistory([to_path], dag.ancestors([to_ctx.node()]))
    from_hist = repo.pathhistory([from_path], dag.ancestors([from_ctx.node()]))

    iters = [to_hist, from_hist]
    paths = [to_path, from_path]

    # we ensure that 'from_path' and 'to_path' exist, so it should be safe to call
    # next() on both iterators.
    heads = [next(iters[0]), next(iters[1])]
    has_ancestor_relation = dag.gcaone(heads) in heads
    i = 1
    while True:
        # check the other one by default
        i = 1 - i
        # if they have direct ancestor relationship, then selects the newer one
        if has_ancestor_relation:
            if isancestor(heads[0], heads[1]):
                i = 1
            elif isancestor(heads[1], heads[0]):
                i = 0

        # check merge info
        for merge in get_subtree_merges(repo, heads[i]):
            if merge.to_path == paths[i] and merge.from_path == paths[1 - i]:
                merge_base_ctx = repo[merge.from_commit]
                return registerdiffgrafts(merge_base_ctx, i)

        # check branch info
        for branch in get_subtree_branches(repo, heads[i]):
            if branch.to_path == paths[i] and branch.from_path == paths[1 - i]:
                merge_base_ctx = repo[branch.from_commit]
                return registerdiffgrafts(merge_base_ctx, i)

        try:
            # add next node to the list
            heads[i] = next(iters[i])
        except StopIteration:
            try:
                # no branch info, use the first parent
                p1 = dag.parentnames(heads[i])[0]
                return registerdiffgrafts(repo[p1], i)
            except IndexError:
                return repo[node.nullid]

    # should never reach here
    raise error.Abort("cannot find a merge base")


def _docopy(ui, repo, *args, **opts):
    cmdutil.bailifchanged(repo)

    # if 'rev' is not specificed, copy from the working copy parent
    from_rev = opts.get("rev") or "."
    from_ctx = scmutil.revsingle(repo, from_rev)
    to_ctx = repo["."]

    from_paths = scmutil.rootrelpaths(from_ctx, opts.get("from_path"))
    to_paths = scmutil.rootrelpaths(from_ctx, opts.get("to_path"))
    subtreeutil.validate_path_size(from_paths, to_paths, abort_on_empty=True)
    subtreeutil.validate_path_exist(ui, from_ctx, from_paths, abort_on_missing=True)
    subtreeutil.validate_path_overlap(from_paths, to_paths)
    subtreeutil.validate_source_commit(ui, from_ctx, "copy")

    if ui.configbool("subtree", "copy-reuse-tree"):
        _do_cheap_copy(repo, from_ctx, to_ctx, from_paths, to_paths, opts)
    else:
        _do_normal_copy(repo, from_ctx, to_ctx, from_paths, to_paths, opts)


def _do_cheap_copy(repo, from_ctx, to_ctx, from_paths, to_paths, opts):
    user = opts.get("user")
    date = opts.get("date")
    text = opts.get("message")

    extra = {}
    extra.update(
        gen_branch_info(
            repo, from_ctx.hex(), from_paths, to_paths, BranchType.SHALLOW_COPY
        )
    )

    summaryfooter = _gen_copy_commit_msg(from_ctx, from_paths, to_paths)
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


def _do_normal_copy(repo, from_ctx, to_ctx, from_paths, to_paths, opts):
    def prefetch(repo, path, fileids):
        # fileservice is defined in shallowrepo.py
        if fileservice := getattr(repo, "fileservice", None):
            with progress.spinner(repo.ui, _("prefetching files in %s") % path):
                fileservice.prefetch(fileids, fetchhistory=False)

    ui = repo.ui
    force = opts.get("force")
    auditor = pathutil.pathauditor(repo.root)

    for to_path in to_paths:
        auditor(to_path)
        if repo.wvfs.lexists(to_path):
            if not force:
                raise error.Abort(
                    _("cannot copy to an existing path: %s") % to_path,
                    hint=_("use --force to overwrite (recursively remove %s)")
                    % to_path,
                )
            matcher = matchmod.match(repo.root, "", [f"path:{to_path}"])
            cmdutil.remove(ui, repo, matcher, mark=False, force=True)

    path_to_fileids = {}
    limit = ui.configint("subtree", "copy-max-file-count", MAX_SUBTREE_COPY_FILE_COUNT)
    file_count = 0
    for path in from_paths:
        matcher = matchmod.match(repo.root, "", [f"path:{path}"])
        fileids = scmutil.walkfiles(repo, from_ctx, matcher)
        file_count += len(fileids)
        if file_count > limit:
            support = ui.config("ui", "supportcontact")
            hint = _("contact %s for help") % support if support else ""
            raise error.Abort(_("subtree copy includes too many files"), hint=hint)
        path_to_fileids[path] = fileids

    new_files = []
    for from_path, to_path in zip(from_paths, to_paths):
        ui.status(_("copying %s to %s\n") % (from_path, to_path))
        fileids = path_to_fileids[from_path]
        prefetch(repo, from_path, fileids)

        with progress.bar(
            repo.ui,
            _("subtree copy from %s to %s") % (from_path, to_path),
            _("files"),
            len(fileids),
        ) as p:
            for src, _node in fileids:
                p.value += 1
                tail = src[len(from_path) :]
                dest = to_path + tail
                os_abs_dest = repo.wjoin(dest)
                os_abs_dest_dir = os.path.dirname(os_abs_dest)
                if not os.path.isdir(os_abs_dest_dir):
                    os.makedirs(os_abs_dest_dir)

                new_files.append(dest)
                fctx = from_ctx[src]
                if fctx.islink():
                    os.symlink(fctx.data(), os_abs_dest)
                else:
                    with open(os_abs_dest, "wb") as f:
                        f.write(fctx.data())

    wctx = repo[None]
    wctx.add(new_files)

    extra = {}
    extra.update(
        gen_branch_info(
            repo, from_ctx.hex(), from_paths, to_paths, BranchType.DEEP_COPY
        )
    )

    summaryfooter = _gen_copy_commit_msg(from_ctx, from_paths, to_paths)
    editform = cmdutil.mergeeditform(repo[None], "subtree.copy")
    editor = cmdutil.getcommiteditor(
        editform=editform, summaryfooter=summaryfooter, **opts
    )

    def commitfunc(ui, repo, message, match, opts):
        return repo.commit(
            message,
            opts.get("user"),
            opts.get("date"),
            match,
            editor=editor,
            extra=extra,
        )

    cmdutil.commit(ui, repo, commitfunc, [], opts)


def _gen_copy_commit_msg(from_commit, from_paths, to_paths):
    full_commit_hash = from_commit.hex()
    msgs = [f"Subtree copy from {full_commit_hash}"]
    for from_path, to_path in zip(from_paths, to_paths):
        msgs.append(f"- Copied path {from_path} to {to_path}")
    return "\n".join(msgs)


def gen_merge_commit_msg(subtree_merges):
    groups = defaultdict(list)
    for from_node, from_path, to_path in subtree_merges:
        groups[from_node].append((from_path, to_path))

    msgs = []
    for from_node, paths in groups.items():
        from_commit = node.hex(from_node)
        msgs.append(f"Subtree merge from {from_commit}")
        for from_path, to_path in paths:
            msgs.append(f"- Merged path {from_path} to {to_path}")
    return "\n".join(msgs)
