# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""find the creation commit of a tracked path

Enable this extension with::

    [extensions]
    debugpathcreation=
"""

from sapling import cmdutil, copies, error, match as matchmod, registrar, scmutil
from sapling.i18n import _
from sapling.node import hex, short
from sapling.utils import subtreeutil


cmdtable = {}
command = registrar.command(cmdtable)

_DEFAULT_SIMILARITY_PERCENT = 90


def _counts_are_similar(left, right, similarity_percent):
    return (
        max(left, right) > 0
        and min(left, right) * 100 >= max(left, right) * similarity_percent
    )


def _count_files(ctx, matcher):
    return ctx.manifest().countfiles(matcher)


def _copied_directory(repo, ctx, path, similarity_percent):
    ui = repo.ui
    matcher = matchmod.match(repo.root, "", [f"path:{path}"])
    destination_count = _count_files(ctx, matcher)
    ui.debug(
        f"inspecting {path!r} at {short(ctx.node())} "
        f"({destination_count} destination files)\n"
    )
    if not destination_count:
        return None

    candidates = []
    for parent in ctx.parents():
        file_copies = copies.pathcopies(parent, ctx, matcher)
        ui.debug(
            f"parent {short(parent.node())} provides "
            f"{len(file_copies)} copy mappings under {path!r}\n"
        )
        source_counts = {}
        for destination, source in file_copies.items():
            suffix = destination[len(path) :]
            if not suffix.startswith("/") or not source.endswith(suffix):
                continue

            source_dir = source[: -len(suffix)]
            if source_dir:
                source_counts[source_dir] = source_counts.get(source_dir, 0) + 1

        if source_counts:
            source_dir, copied_count = max(
                source_counts.items(), key=lambda item: item[1]
            )
            ui.debug(
                f"candidate {source_dir!r} maps "
                f"{copied_count}/{destination_count} destination files\n"
            )
            if not _counts_are_similar(
                copied_count, destination_count, similarity_percent
            ):
                ui.debug(
                    f"rejecting {source_dir!r}; copy coverage is "
                    f"{copied_count * 100 / destination_count:.1f}%, below "
                    f"configured {similarity_percent}%; use '--config "
                    "debugpathcreation.similarity-percent=N' to adjust "
                    "the threshold, where 50 < N <= 100\n"
                )
                continue

            candidates.append((parent, source_dir))

    if len(candidates) != 1:
        ui.debug(
            f"found {len(candidates)} viable copy sources for "
            f"{path!r} at {short(ctx.node())}\n"
        )
        return None

    source_ctx, source_dir = candidates[0]
    source_matcher = matchmod.match(repo.root, "", [f"path:{source_dir}"])
    source_count = _count_files(source_ctx, source_matcher)
    ui.debug(f"candidate {source_dir!r} contains {source_count} source files\n")
    warning = None
    if not _counts_are_similar(source_count, destination_count, similarity_percent):
        warning = _(
            "warning: inferred directory copy from '%s' to '%s' despite "
            "dissimilar file counts (%d source, %d destination)\n"
        ) % (source_dir, path, source_count, destination_count)
    return source_ctx.node(), source_dir, warning


def _copied_file(ctx, path):
    renamed = ctx[path].renamed()
    parents = ctx.parents()
    if renamed is None or len(parents) != 1:
        return None

    return parents[0].node(), renamed[0]


def _find_path_creation(repo, head, path, is_directory, similarity_percent):
    """
    return the origin commit for ``path``, appending each traced copy/rename
    step to ``trace`` as a structured dict.
    """
    trace = []
    dag = repo.changelog.dag
    while creation := repo.pathcreation(path, dag.ancestors([head])):
        creation_ctx = repo[creation]
        source = subtreeutil.find_subtree_copy(repo, creation, path)
        is_subtree_copy = source is not None
        warning = None
        if source is None:
            if is_directory:
                found = _copied_directory(repo, creation_ctx, path, similarity_percent)
                if found is not None:
                    source_node, source_dir, warning = found
                    source = (source_node, source_dir)
            else:
                source = _copied_file(creation_ctx, path)
        if source is None:
            repo.ui.debug(
                f"no copy source found; {short(creation_ctx.node())} is the origin\n"
            )
            return creation, trace

        source_commit, source_path = source
        source_ctx = repo[source_commit]
        step = {
            "commit": creation_ctx.hex(),
            "source": source_path,
            "destination": path,
            "subtree": is_subtree_copy,
        }
        if warning is not None:
            step["warning"] = warning.rstrip("\n")
            repo.ui.warn(warning)
        message = (
            _("tracing backward: %s subtree copied '%s' to '%s'\n")
            if is_subtree_copy
            else _("tracing backward: %s copied '%s' to '%s'\n")
        )
        repo.ui.status_err(message % (short(creation_ctx.node()), source_path, path))
        trace.append(step)
        head, path = source_ctx.node(), source_path

    raise error.Abort(
        _("cannot find the origin of path '%s'") % path,
        hint=_("run '@prog@ log %s' to inspect its history") % path,
    )


@command(
    "debugpathcreation",
    [("r", "rev", "", _("start tracing at revision (default: .)"), _("REV"))]
    + cmdutil.formatteropts,
    _("[-r REV] PATH"),
)
def debugpathcreation(ui, repo, path, **opts) -> None:
    """print the oldest commit in a path's copy and rename history

    Directory copy/rename history based on ``sl copy``/``sl rename`` metadata
    is inferred heuristically and may be inaccurate.

    Use ``-T json`` to emit machine-readable output. The JSON object includes
    the ``origin`` commit along with a ``trace`` of each copy/rename step
    (the information otherwise printed to stderr), so it can be consumed by
    other tooling.
    """

    similarity_percent = ui.configint(
        "debugpathcreation", "similarity-percent", _DEFAULT_SIMILARITY_PERCENT
    )
    # More than half guarantees a unique source directory within each parent.
    if not 50 < similarity_percent <= 100:
        raise error.Abort(
            _(
                "debugpathcreation.similarity-percent must be greater than 50 "
                "and at most 100"
            )
        )

    ctx = scmutil.revsingle(repo, opts.get("rev"))
    path = scmutil.rootrelpath(ctx, path)
    if not path:
        raise error.Abort(_("repository root is not supported"))
    is_directory = ctx.hasdir(path)
    if not is_directory and path not in ctx:
        raise error.Abort(_("path '%s' does not exist in commit %s") % (path, ctx))

    with ui.formatter("debugpathcreation", opts) as fm:
        origin, trace = _find_path_creation(
            repo,
            ctx.node(),
            path,
            is_directory,
            similarity_percent,
        )
        fm.startitem()
        fm.data(path=path, trace=trace)
        fm.write("origin", "%s\n", hex(origin))
