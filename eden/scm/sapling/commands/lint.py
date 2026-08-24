# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import bindings

from .. import (
    cmdutil,
    error,
    filelint,
    hg,
    phases,
    revset,
    rewriteutil,
    scmutil,
    smartset,
)
from ..i18n import _
from ..node import wdirrev
from .cmdtable import command


@command(
    "lint",
    [
        ("r", "rev", [], _("revisions to lint"), _("REV")),
        ("", "fix", True, _("apply fixes and rewrite commits")),
        (
            "",
            "clear-cache",
            False,
            _("clear the lint clean-content cache before linting (ADVANCED)"),
        ),
    ],
    _("[-r REV]..."),
)
def lint(ui, repo, **opts):
    """lint files changed by revisions and apply fixes"""
    cmdutil.checkunfinished(repo)

    fix = opts.get("fix")
    # All draft commits connected to the working copy.
    defaultrevs = "(draft() & ::.):: & draft()"
    revspecs = opts.get("rev") or [defaultrevs, "wdir()"]
    selectedrevs = list(scmutil.revrange(repo, revspecs))
    wdirrevs = {wdirrev, scmutil.revf64encode(wdirrev)}
    # revrange() drops the virtual wdir() revision, so detect it by evaluating
    # each spec against a wdir()-only subset.
    lintwdir = _includeswdir(repo, revspecs)
    revs = sorted(rev for rev in selectedrevs if rev not in wdirrevs)
    if not revs and not lintwdir:
        raise error.Abort(_("empty revision set"))

    if opts.get("clear_cache"):
        # The cache only skips work, so a failed clear (ex. another process
        # holds the cache files open on Windows) warns instead of aborting.
        failure = bindings.filelint.clearcache(repo._rsrepo)
        if failure:
            ui.warn(
                _("warning: failed to clear lint clean-content cache: %s\n") % failure
            )

    with repo.wlock(), repo.lock():
        dirtypaths = set(repo[None].files())
        ctxs = []
        if revs:
            possibleaffectedrevs = sorted(repo.revs("(%ld::) - public()", revs))
            checkrevs = sorted(set(revs).union(possibleaffectedrevs))
            _prefetchcommittexts(repo, repo.changelog.tonodes(checkrevs))

            rewriteutil.precheck(repo, checkrevs, "lint")
            _bailifmerges(repo, checkrevs)
            ctxs = [repo[rev] for rev in revs]

        with filelint.lintctxs(repo, ctxs) as fixes:
            changedrevs = [
                rev
                for rev, replacements in zip(revs, fixes, strict=True)
                if replacements
            ]
            if not changedrevs:
                fixedwc = 0
                if lintwdir and fix:
                    fixedwc = _lintworkingcopy(repo, dirtypaths)
                if fixedwc:
                    # The linters rewrite dirty files in place and report
                    # their own output, so this does not claim anything
                    # actually changed.
                    ui.status(_("linted working copy files; no commits rewritten\n"))
                else:
                    ui.status(_("nothing changed\n"))
                return 0

            fixedfiles = sum(len(paths) for paths in fixes)
            if not fix:
                ui.status(_("%d file(s) need fixes\n") % fixedfiles)
                return 1

            affectedrevs = sorted(repo.revs("(%ld::) - public()", changedrevs))
            replacementsbyrev = dict(zip(revs, fixes, strict=True))
            oldwdirparent = repo["."].node()

            with repo.transaction("lint") as tr:
                replacements = _rewrite(repo, tr, affectedrevs, replacementsbyrev)
                moves = scmutil.cleanupnodes(repo, replacements, "lint")

            if dirtypaths:
                # Now that the rewrite is durable, make clean inherited paths
                # match the new parent so its identity can move without
                # checking out over local edits.
                propagatedpaths = _propagatedpaths(
                    repo, affectedrevs, replacementsbyrev, oldwdirparent
                )
                workingpaths = propagatedpaths - dirtypaths
                if lintwdir:
                    workingpaths.update(dirtypaths)
                _lintworkingcopy(repo, workingpaths)

            newwdirparent = moves.get(oldwdirparent)
            if newwdirparent is not None:
                if dirtypaths:
                    # Fixed local content already matches the new parent,
                    # so only the working-copy parent identity moves.
                    repo.setparents(newwdirparent)
                else:
                    hg.updaterepo(repo, newwdirparent, overwrite=True)

    ui.status(
        _("fixed %d files and rewrote %d commits\n") % (fixedfiles, len(replacements))
    )
    return 0


def _prefetchcommittexts(repo, nodes):
    """Batch commit-text reads for revisions and their parents."""
    nodes = bindings.dag.nameset(nodes)
    tofetch = bindings.dag.nameset(nodes)
    tofetch += repo.changelog.dag.parents(nodes)
    if not tofetch:
        return
    repo.changelog.filternodes(tofetch)
    repo.changelog.inner.getcommitrawtextlist(tofetch)


def _bailifmerges(repo, revs):
    """Reject revisions that require the single-parent replay path to merge."""
    if any(len(repo[rev].parents()) > 1 for rev in revs):
        raise error.Abort(_("cannot lint merge commits"))


def _includeswdir(repo, revspecs):
    subset = smartset.baseset([wdirrev], repo=repo)
    with repo.names.included_user():
        return any(
            wdirrev in revset.match(repo.ui, spec, repo=repo)(repo, subset=subset)
            for spec in revspecs
        )


def _lintworkingcopy(repo, paths):
    wctx = repo[None]
    paths = sorted(path for path in paths if path in wctx and repo.wvfs.lexists(path))
    filelint.lintworkingcopy(repo, paths)
    return len(paths)


def _propagatedpaths(repo, revs, replacementsbyrev, targetnode):
    """Track fixed paths inherited through commits that do not touch them.

    Relies on ascending revision numbers being a topological order and on
    merge commits having been rejected earlier, so following p1 suffices.
    """
    pathsbynode = {}
    for rev in revs:
        ctx = repo[rev]
        paths = set(pathsbynode.get(ctx.p1().node(), ()))
        paths.difference_update(ctx.files())
        paths.update(replacementsbyrev.get(rev, ()))
        pathsbynode[ctx.node()] = paths
    return pathsbynode.get(targetnode, set())


def _rewrite(repo, tr, revs, replacementsbyrev):
    rewritten = {}
    replacements = {}
    for rev in revs:
        ctx = repo[rev]
        parents = [
            repo[rewritten.get(parent.node(), parent.node())]
            for parent in ctx.parents()
        ]
        mctx = filelint.overlayctx(
            ctx, parents, replacementsbyrev.get(rev, {}), operation="lint"
        )
        newnode = repo.commitctx(mctx)
        phases.retractboundary(repo, tr, ctx.phase(), [newnode])
        rewritten[ctx.node()] = newnode
        replacements[ctx.node()] = [newnode]
    return replacements
