# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""find the creation commit of a tracked directory

Enable this extension with::

    [extensions]
    debugpathcreation=
"""

from sapling import copies, error, match as matchmod, registrar, scmutil
from sapling.i18n import _
from sapling.node import hex, short
from sapling.utils import subtreeutil


cmdtable = {}
command = registrar.command(cmdtable)

_MIN_DIRECTORY_SIMILARITY_PERCENT = 90


def _counts_are_similar(left, right):
    return (
        max(left, right) > 0
        and min(left, right) * 100
        >= max(left, right) * _MIN_DIRECTORY_SIMILARITY_PERCENT
    )


def _count_files(ctx, matcher):
    return ctx.manifest().countfiles(matcher)


def _copied_directory(repo, ctx, path):
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
            if not _counts_are_similar(copied_count, destination_count):
                ui.debug(
                    f"rejecting {source_dir!r}; copy coverage is below "
                    f"{_MIN_DIRECTORY_SIMILARITY_PERCENT}%\n"
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
    if not _counts_are_similar(source_count, destination_count):
        ui.warn(
            _(
                "warning: inferred directory copy from '%s' to '%s' despite "
                "dissimilar file counts (%d source, %d destination)\n"
            )
            % (source_dir, path, source_count, destination_count)
        )
    return source_ctx.node(), source_dir


def _find_path_creation(repo, head, path):
    dag = repo.changelog.dag
    while creation := repo.pathcreation(path, dag.ancestors([head])):
        creation_ctx = repo[creation]
        source = subtreeutil.find_subtree_copy(repo, creation, path)
        is_subtree_copy = source is not None
        if source is None:
            source = _copied_directory(repo, creation_ctx, path)
        if source is None:
            repo.ui.debug(
                f"no copy source found; {short(creation_ctx.node())} is the origin\n"
            )
            return creation

        source_commit, source_path = source
        source_ctx = repo[source_commit]
        message = (
            _("tracing backward: %s subtree copied '%s' to '%s'\n")
            if is_subtree_copy
            else _("tracing backward: %s copied '%s' to '%s'\n")
        )
        repo.ui.status_err(message % (short(creation_ctx.node()), source_path, path))
        head, path = source_ctx.node(), source_path

    raise error.Abort(
        _("cannot find the origin of directory '%s'") % path,
        hint=_("run '@prog@ log %s' to inspect its history") % path,
    )


@command("debugpathcreation", [], _("FOLDER"))
def debugpathcreation(ui, repo, folder, **opts) -> None:
    """print the oldest commit in the history of a tracked directory"""

    ctx = repo["."]
    path = scmutil.rootrelpath(ctx, folder)
    if not path:
        raise error.Abort(_("repository root is not supported"))
    if not ctx.hasdir(path):
        raise error.Abort(_("path '%s' is not a directory in commit %s") % (path, ctx))

    ui.write("%s\n" % hex(_find_path_creation(repo, ctx.node(), path)))
