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
from sapling.utils import pathutil, subtreeutil


cmdtable = {}
command = registrar.command(cmdtable)


def _copied_directory(repo, ctx, path):
    ui = repo.ui
    files = [
        file
        for file in ctx.files()
        if file in ctx and pathutil.path_starts_with(file, path)
    ]
    if not files:
        return None

    matcher = matchmod.match(repo.root, "", [f"path:{path}"])
    ui.debug(
        f"inspecting {path!r} at {short(ctx.node())} ({len(files)} destination files)\n"
    )

    candidates = []
    for parent in ctx.parents():
        file_copies = copies.pathcopies(parent, ctx, matcher)
        ui.debug(
            f"parent {short(parent.node())} provides "
            f"{len(file_copies)} copy mappings under {path!r}\n"
        )
        source_dir = None
        for destination in files:
            source = file_copies.get(destination)
            suffix = destination[len(path) :]
            if (
                source is None
                or not suffix.startswith("/")
                or not source.endswith(suffix)
            ):
                source_dir = None
                break

            candidate = source[: -len(suffix)]
            if not candidate or (source_dir is not None and candidate != source_dir):
                source_dir = None
                break
            source_dir = candidate

        if source_dir is not None:
            ui.debug(
                f"candidate {source_dir!r} maps "
                f"{len(files)}/{len(files)} destination files\n"
            )
            candidates.append((parent.node(), source_dir))

    if len(candidates) != 1:
        ui.debug(
            f"found {len(candidates)} viable copy sources for "
            f"{path!r} at {short(ctx.node())}\n"
        )
        return None

    return candidates[0]


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
