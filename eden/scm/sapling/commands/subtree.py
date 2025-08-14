# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import json
import os
from collections import defaultdict

from .. import (
    cloneuri,
    cmdutil,
    context,
    error,
    hg,
    match as matchmod,
    merge as mergemod,
    node,
    pathlog,
    pathutil,
    progress,
    registrar,
    scmutil,
)
from ..cmdutil import (
    commitopts,
    commitopts2,
    diffopts,
    diffopts2,
    dryrunopts,
    mergetoolopts,
    subtree_path_opts,
    templatekw,
    walkopts,
)
from ..i18n import _
from ..utils import subtreeutil
from ..utils.subtreeutil import (
    BranchType,
    gen_branch_info,
    get_or_clone_git_repo,
    get_subtree_branches,
    get_subtree_imports,
    get_subtree_merges,
)
from .cmdtable import command

MAX_SUBTREE_COPY_FILE_COUNT = 10_000
COPY_REUSE_TREE = False

MERGE_BASE_STRATEGIES = [
    # Walk only the from‑path’s history when searching for a merge base.
    "only-from",
    # Walk only the to‑path’s history when searching for a merge base.
    "only-to",
]

readonly = registrar.command.readonly


@templatekw.templatekeyword("subtree_copies")
def subtree_copies(repo, ctx, **args):
    copies = get_subtree_branches(repo, ctx.node())
    if copies:
        copies = [c.to_full_dict() for c in copies]
        return json.dumps(copies)


@templatekw.templatekeyword("subtree_merges")
def subtree_merges(repo, ctx, **args):
    merges = get_subtree_merges(repo, ctx.node())
    if merges:
        merges = [c.to_full_dict() for c in merges]
        return json.dumps(merges)


@command(
    "subtree",
    [],
    _("<copy|graft|merge|diff>"),
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
    """create a directory or file branching

    To create a branch from the directory "my-project" at commit A into the
    directory "my-branch", use::

      @prog@ subtree cp -r A --from-path my-project --to-path my-branch

    Subtree copy also supports multiple "--from-path"/"--to-path" mappings::

      @prog@ subtree cp \\
        --from-path my-project --to-path my-branch1 \\
        --from-path my-project --to-path my-branch2
    """
    with repo.wlock(), repo.lock():
        return _docopy(ui, repo, *args, **opts)


@subtree_subcmd(
    "import",
    [
        (
            "",
            "url",
            "",
            _("external repository url"),
            _("URL"),
        ),
        (
            "r",
            "rev",
            "",
            _("external repository commit hash"),
            _("REV"),
        ),
        ("f", "force", None, _("overwrite existing path")),
    ]
    + subtree_path_opts
    + commitopts
    + commitopts2,
    _("-r REV [--from-path PATH] --to-path PATH ..."),
)
def subtree_import(ui, repo, *args, **opts):
    """import an external repository into current repository at the specified path

    Import an external (Git) repository into the current @Product@ repository at
    a specified destination path. This preserves the external repository's metadata.

    The command requires specifying the external repository URL (HTTPS or SSH),
    a specific commit hash to import, and the destination path within the current
    repository.

    Optionally, a source path within the external repository can be specified to
    import only a subdirectory.

    Examples:

    - Import entire repository::

        @prog@ subtree import --url https://github.com/owner/foo.git \\
            --to-path vendor/foo --rev abc123

    - Import specific subdirectory::

        @prog@ subtree import --url git@github.com:owner/foo.git \\
            --from-path src --to-path vendor/foo_src --rev def456

    Use --force to overwrite existing destination paths.
    """
    with repo.wlock(), repo.lock():
        return _do_import(ui, repo, *args, **opts)


@subtree_subcmd(
    "prefetch",
    [
        (
            "",
            "url",
            "",
            _("external repository url"),
            _("URL"),
        ),
        (
            "r",
            "rev",
            "",
            _("external repository commit hash"),
            _("REV"),
        ),
        ("f", "force", None, _("overwrite existing path")),
    ],
    _("-r REV [--from-path PATH] --to-path PATH ..."),
)
def subtree_prefetch(ui, repo, *args, **opts):
    """prefetch commits from the external repository

    Prefetches commits from the external repository and stores them in the
    local cache.
    """
    from_rev = opts.get("rev")
    if not from_rev:
        raise error.Abort(_("must specify the external repository commit hash"))
    url = opts.get("url")
    if not url:
        raise error.Abort(_("must specify the external repository url"))
    giturl = cloneuri.determine_git_uri(None, url)

    get_or_clone_git_repo(ui, giturl, from_rev)


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
        (
            "",
            "url",
            "",
            _("external repository url (EXPERIMENTAL)"),
            _("URL"),
        ),
    ]
    + commitopts
    + commitopts2
    + cmdutil.messagefieldopts
    + mergetoolopts
    + dryrunopts
    + subtree_path_opts,
    _("[OPTION]... --from-path PATH --to-path PATH ..."),
)
def subtree_graft(ui, repo, **opts):
    """move commits from one path to another"""
    from sapling.commands import _dograft

    url = opts.get("url")
    from_paths = opts.get("from_path")
    to_paths = opts.get("to_path")
    if not (opts.get("continue") or opts.get("abort")):
        if url and not from_paths:
            from_paths = [""]
            opts["from_path"] = from_paths
        if not (from_paths and to_paths):
            raise error.Abort(
                _("must provide --from-path and --to-path for same-repo grafts")
            )

    if url:
        giturl = cloneuri.determine_git_uri(None, url)
        if giturl is None:
            raise error.Abort(_("unable to determine git url from '%s'") % url)
        from_repo = get_or_clone_git_repo(ui, giturl)
    else:
        from_repo = repo

    with repo.wlock():
        return _dograft(ui, repo, from_repo=from_repo, **opts)


@subtree_subcmd(
    "merge",
    [
        ("r", "rev", "", _("revisions to merge"), _("REV")),
        (
            "",
            "merge-base-strategy",
            "",
            _("strategy for searching merge‑base: %s")
            % ", ".join(MERGE_BASE_STRATEGIES),
        ),
    ]
    + mergetoolopts
    + subtree_path_opts,
    _("[OPTION]... --from-path PATH --to-path PATH"),
)
def subtree_merge(ui, repo, **opts):
    """merge a path of the specified commit into a different path of the current commit

    To merge the directory branch "my-branch" at commit A into "my-project", run::

      @prog@ subtree merge -r A --from-path my-branch --to-path my-project

    If "-r/--rev" option is not specified, "-r ." is used as the default value.

    By default @Product@ scans the history of both "--from-path" and "to-path" to
    find the best merge base. If one side’s history is massive, that can get slow.
    Narrow the search with "--merge-base-strategy":

    - only-from: walk only the from-path's history
    - only-to: walk only the to-path's history
    """
    ctx = repo["."]
    from_ctx = scmutil.revsingle(repo, opts.get("rev"))
    from_paths = scmutil.rootrelpaths(ctx, opts.get("from_path"))
    to_paths = scmutil.rootrelpaths(ctx, opts.get("to_path"))

    if len(from_paths) != 1 or len(to_paths) != 1:
        raise error.Abort(_("must provide exactly one --from-path and --to-path"))
    subtreeutil.validate_path_overlap(from_paths, to_paths)
    subtreeutil.validate_path_exist(ui, from_ctx, from_paths, abort_on_missing=True)
    subtreeutil.validate_path_exist(ui, ctx, to_paths, abort_on_missing=True)
    subtreeutil.validate_path_depth(ui, from_paths + to_paths)
    subtreeutil.validate_source_commit(repo.ui, from_ctx, "merge")
    subtreeutil.validate_file_count(repo, from_ctx, from_paths)

    merge_base_ctx = _subtree_merge_base(
        repo, ctx, to_paths[0], from_ctx, from_paths[0], opts
    )
    ui.status("merge base: %s\n" % merge_base_ctx)
    cmdutil.registerdiffgrafts(from_paths, to_paths, from_ctx)

    with ui.configoverride(
        {
            ("ui", "forcemerge"): opts.get("tool", ""),
            ("subtree", "allow-merge-subtree-copy-commit"): True,
        },
        "subtree_merge",
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


@subtree_subcmd(
    "diff",
    [
        ("r", "rev", [], _("revision"), _("REV")),
    ]
    + diffopts
    + diffopts2
    + walkopts
    + subtree_path_opts,
    _("[OPTION]... ([-r REV1 [-r REV2]])"),
    inferrepo=True,
    cmdtype=readonly,
)
def subtree_diff(ui, repo, *args, **opts):
    """show differences between directory branches"""
    from sapling.commands import do_diff

    return do_diff(ui, repo, *args, **opts)


@subtree_subcmd(
    "inspect",
    [
        ("r", "rev", "", _("revision"), _("REV")),
    ],
    _("[OPTION]... [-r REV]"),
    inferrepo=True,
    cmdtype=readonly,
)
def subtree_inspect(ui, repo, *args, **opts):
    """inspect the internal subtree metadata of commits"""
    ctx = scmutil.revsingle(repo, opts.get("rev"))
    copies = get_subtree_branches(repo, ctx.node())
    merges = get_subtree_merges(repo, ctx.node())
    imports = get_subtree_imports(repo, ctx.node())

    if not copies and not merges and not imports:
        ui.warn(_("no subtree metadata found for commit %s\n") % ctx)
        return

    result = {}
    if copies:
        result["copies"] = [c.to_full_dict() for c in copies]
    if merges:
        result["merges"] = [c.to_full_dict() for c in merges]
    if imports:
        result["imports"] = [c.to_full_dict() for c in imports]

    result_json = json.dumps(
        result,
        indent=2,
        separators=(",", ": "),
    )
    ui.write(f"{result_json}\n")


def _subtree_merge_base(repo, to_ctx, to_path, from_ctx, from_path, opts):
    """get the best merge base for subtree merge

    There are two major use cases for subtree merge:
    1. merge a dev branch (original copy-to directory) to main branch
    2. merge a main branch to release branch (original copy-to directory)

    High level idea of the algorithm:
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
            curr_from_path = paths[1]
            cmdutil.registerdiffgrafts([curr_from_path], [to_path], merge_base_ctx)
        else:
            curr_to_path = paths[0]
            cmdutil.registerdiffgrafts([curr_to_path], [to_path], merge_base_ctx)
        return merge_base_ctx

    def get_p1(dag, node):
        try:
            return dag.parentnames(node)[0]
        except IndexError:
            return None

    def get_to_path_history(repo, node, path, strategy):
        if strategy == "only-from":
            return iter([])
        return pathlog.pathlog(repo, node, path, is_prefetch_commit_text=True)

    def get_from_path_history(repo, node, path, strategy):
        if strategy == "only-to":
            return iter([])
        return pathlog.pathlog(repo, node, path, is_prefetch_commit_text=True)

    strategy = opts.get("merge_base_strategy")
    if strategy and strategy not in MERGE_BASE_STRATEGIES:
        raise error.Abort(
            _("invalid merge base strategy: %s") % strategy,
            hint=_("valid strategies: %s") % ", ".join(MERGE_BASE_STRATEGIES),
        )

    paths = [to_path, from_path]
    dag = repo.changelog.dag
    if from_path == to_path:
        nodes = [from_ctx.node(), to_ctx.node()]
        gca = dag.gcaone(nodes)
        return registerdiffgrafts(repo[gca], 0)

    ui = repo.ui
    ui.status(_("searching for merge base ...\n"))
    isancestor = dag.isancestor
    to_hist = get_to_path_history(repo, to_ctx.node(), to_path, strategy)
    from_hist = get_from_path_history(repo, from_ctx.node(), from_path, strategy)
    # `iter`'s order should match the order of `paths`
    iters = [to_hist, from_hist]

    # we ensure that 'from_path' and 'to_path' exist, so it should be safe to call
    # next() on both iterators.
    heads = [
        # heads[0] is the head of the to_path history
        None if strategy == "only-from" else next(iters[0]),
        # heads[1] is the head of the from_path history
        None if strategy == "only-to" else next(iters[1]),
    ]
    i = 1
    with progress.bar(
        ui,
        _("searching commit history"),
        _("commits"),
    ) as p:
        while True:
            p.value += 1
            if strategy == "only-to":
                i = 0
            elif strategy == "only-from":
                i = 1
            else:
                # check the other one by default
                i = 1 - i
                # if they have direct ancestor relationship, then selects the newer one
                if isancestor(heads[0], heads[1]):
                    i = 1
                elif isancestor(heads[1], heads[0]):
                    i = 0

            # check merge info
            curr_node = heads[i]
            ui.debug("checking commit %s\n" % node.short(curr_node))
            for merge in get_subtree_merges(repo, curr_node):
                if merge.to_path == paths[i] and merge.from_path == paths[1 - i]:
                    ui.status(
                        _("found the last subtree merge commit %s\n")
                        % node.short(curr_node)
                    )
                    merge_base_ctx = repo[merge.from_commit]
                    return registerdiffgrafts(merge_base_ctx, i)

            # check branch info
            for branch in get_subtree_branches(repo, curr_node):
                if branch.to_path == paths[i] and branch.from_path == paths[1 - i]:
                    ui.status(
                        _("found the last subtree copy commit %s\n")
                        % node.short(curr_node)
                    )
                    merge_base_ctx = repo[branch.from_commit]
                    return registerdiffgrafts(merge_base_ctx, i)
                elif branch.to_path == paths[i]:
                    # subtree copy overwrite, follow the source commit
                    source_node = node.bin(branch.from_commit)
                    paths[i] = branch.from_path
                    hist = pathlog.pathlog(
                        repo, source_node, paths[i], is_prefetch_commit_text=True
                    )
                    # XXX: handle normal merge base
                    iters[i] = hist

            try:
                # add next node to the list
                heads[i] = next(iters[i])
            except StopIteration:
                p1 = get_p1(dag, curr_node) or curr_node
                return registerdiffgrafts(repo[p1], i)


def _docopy(ui, repo, *args, **opts):
    cmdutil.bailifchanged(repo)

    # if 'rev' is not specified, copy from the working copy parent
    from_rev = opts.get("rev") or "."
    from_ctx = scmutil.revsingle(repo, from_rev)
    to_ctx = repo["."]

    from_paths = scmutil.rootrelpaths(from_ctx, opts.get("from_path"))
    to_paths = scmutil.rootrelpaths(from_ctx, opts.get("to_path"))
    subtreeutil.validate_path_size(from_paths, to_paths, abort_on_empty=True)
    subtreeutil.validate_path_exist(ui, from_ctx, from_paths, abort_on_missing=True)
    subtreeutil.validate_path_overlap(from_paths, to_paths)
    subtreeutil.validate_source_commit(ui, from_ctx, "copy")

    if COPY_REUSE_TREE:
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
    ui = repo.ui
    abort_or_remove_paths(ui, repo, to_paths, "copy", opts)
    copy_files(ui, repo, repo, from_ctx, from_paths, to_paths, "copy")

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


def _do_import(ui, repo, *args, **opts):
    cmdutil.bailifchanged(repo)

    from_rev = opts.get("rev")
    if not from_rev:
        raise error.Abort(_("must specify the external repository commit hash"))
    url = opts.get("url")
    if not url:
        raise error.Abort(_("must specify the external repository url"))
    giturl = cloneuri.determine_git_uri(None, url)
    if giturl is None:
        raise error.Abort(_("unable to determine git url from '%s'") % url)

    ctx = repo["."]
    # default to root ("") of the repo
    from_paths = opts.get("from_path") or [""]
    to_paths = scmutil.rootrelpaths(ctx, opts.get("to_path"))
    if not to_paths:
        raise error.Abort(_("must specify the to-path"))
    subtreeutil.validate_path_size(from_paths, to_paths, abort_on_empty=True)
    subtreeutil.validate_path_overlap([], to_paths)
    subtreeutil.validate_path_depth(ui, to_paths)

    abort_or_remove_paths(ui, repo, to_paths, "import", opts)

    git_repo = get_or_clone_git_repo(ui, giturl, from_rev)
    from_ctx = git_repo[from_rev]
    subtreeutil.validate_path_exist(ui, from_ctx, from_paths, abort_on_missing=True)

    copy_files(ui, git_repo, repo, from_ctx, from_paths, to_paths, "import")

    from_commit = from_ctx.hex()
    # use the original `url` in the metadata, as the `giturl` may lost information
    # e.g.: "git+" prefix
    extra = subtreeutil.gen_import_info(ui, url, from_commit, from_paths, to_paths)
    summaryfooter = _gen_import_commit_msg(url, from_commit, from_paths, to_paths)

    editform = cmdutil.mergeeditform(repo[None], "subtree.import")
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


def _gen_copy_commit_msg(from_ctx, from_paths, to_paths):
    full_commit = from_ctx.hex()
    msgs = [f"Subtree copy from {full_commit}"]
    for from_path, to_path in zip(from_paths, to_paths):
        msgs.append(f"- Copied path {from_path} to {to_path}")
    return "\n".join(msgs)


def _gen_import_commit_msg(url, from_commit, from_paths, to_paths):
    msgs = [f"Subtree import from {url} at {from_commit} "]
    for from_path, to_path in zip(from_paths, to_paths):
        from_path = os.path.join("/", from_path)
        msgs.append(f"- Imported path {from_path} to {to_path}")
    return "\n".join(msgs)


def gen_merge_commit_msg(subtree_merges):
    groups = defaultdict(list)
    for m in subtree_merges:
        groups[m["from_commit"]].append((m["from_path"], m["to_path"]))

    msgs = []
    for from_node, paths in groups.items():
        from_commit = node.hex(from_node)
        msgs.append(f"Subtree merge from {from_commit}")
        for from_path, to_path in paths:
            msgs.append(f"- Merged path {from_path} to {to_path}")
    return "\n".join(msgs)


def copy_files(ui, from_repo, to_repo, from_ctx, from_paths, to_paths, subcmd):
    """copy files from `from_repo` to `to_repo`.

    `from_repo` can be an external git repo in the `subtree import` case.
    """

    def prefetch(repo, path, fileids):
        # fileservice is defined in shallowrepo.py
        if fileservice := getattr(repo, "fileservice", None):
            with progress.spinner(repo.ui, _("prefetching files in %s") % path):
                fileservice.prefetch(fileids, fetchhistory=False)

    limit = ui.configint("subtree", "max-file-count")
    path_to_fileids = {}
    for path in from_paths:
        matcher = matchmod.match(from_repo.root, "", [f"path:{path}"])
        fileids = scmutil.walkfiles(from_repo, from_ctx, matcher)
        file_count = len(fileids)
        if limit and file_count > limit:
            support = ui.config("ui", "supportcontact")
            help_hint = _("contact %s for help") % support if support else None
            raise error.Abort(
                _("path '%s' includes too many files: %d (max: %d)")
                % (path, file_count, limit),
                hint=help_hint,
            )
        path_to_fileids[path] = fileids

    new_files = []
    for from_path, to_path in zip(from_paths, to_paths):
        ui.status(_("copying %s to %s\n") % (from_path or "/", to_path))
        fileids = path_to_fileids[from_path]
        prefetch(from_repo, from_path, fileids)
        with progress.bar(
            ui,
            _("subtree %s from %s to %s") % (subcmd, from_path or "/", to_path),
            _("files"),
            len(fileids),
        ) as p:
            for src, _node in fileids:
                p.value += 1
                tail = src[len(from_path) :]
                dest = to_path + ("/" if from_path == "" else "") + tail
                fctx = from_ctx[src]
                to_repo.wwrite(dest, fctx.data(), fctx.flags())
                new_files.append(dest)

    wctx = to_repo[None]
    wctx.add(new_files)


def abort_or_remove_paths(ui, repo, paths, subcmd, opts):
    """Abort if the path exists and `force` is not set, otherwise remove the path"""
    force = opts.get("force")
    auditor = pathutil.pathauditor(repo.root)

    for path in paths:
        auditor(path)
        if repo.wvfs.lexists(path):
            if not force:
                raise error.Abort(
                    _("cannot %s to an existing path: %s") % (subcmd, path),
                    hint=_("use --force to overwrite"),
                )
            matcher = matchmod.match(repo.root, "", [f"path:{path}"])
            cmdutil.remove(ui, repo, matcher, mark=False, force=True)
            if repo.wvfs.lexists(path):
                repo.wvfs.rmtree(path)
