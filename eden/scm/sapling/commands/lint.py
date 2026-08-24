# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import bindings

from .. import cmdutil, error, filelint, hg, phases, rewriteutil, scmutil
from ..i18n import _
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
    cmdutil.bailifchanged(repo)

    # All draft commits connected to the working copy.
    defaultrevs = "descendants(roots(draft() & ::.)) & draft()"
    revs = scmutil.revrange(repo, opts.get("rev") or [defaultrevs])
    if not revs:
        raise error.Abort(_("empty revision set"))
    revs.sort()

    if opts.get("clear_cache"):
        # The cache only skips work, so a failed clear (ex. another process
        # holds the cache files open on Windows) warns instead of aborting.
        failure = bindings.filelint.clearcache(repo._rsrepo)
        if failure:
            ui.warn(
                _("warning: failed to clear lint clean-content cache: %s\n") % failure
            )

    with repo.wlock(), repo.lock():
        possibleaffectedrevs = sorted(repo.revs("(%ld::) - public()", revs))
        checkrevs = sorted(set(revs).union(possibleaffectedrevs))
        _prefetchcommittexts(repo, repo.changelog.tonodes(checkrevs))

        rewriteutil.precheck(repo, checkrevs, "lint")
        _bailifmerges(repo, checkrevs)
        with filelint.lintctxs(repo, [repo[rev] for rev in revs]) as fixes:
            changedrevs = [
                rev
                for rev, replacements in zip(revs, fixes, strict=True)
                if replacements
            ]
            if not changedrevs:
                ui.status(_("nothing changed\n"))
                return 0

            fixedfiles = sum(len(paths) for paths in fixes)
            if not opts.get("fix"):
                ui.status(_("%d file(s) need fixes\n") % fixedfiles)
                return 1

            affectedrevs = sorted(repo.revs("(%ld::) - public()", changedrevs))
            replacementsbyrev = dict(zip(revs, fixes, strict=True))
            oldwdirparent = repo["."].node()

            with repo.transaction("lint") as tr:
                replacements = _rewrite(repo, tr, affectedrevs, replacementsbyrev)
                moves = scmutil.cleanupnodes(repo, replacements, "lint")

            newwdirparent = moves.get(oldwdirparent)
            if newwdirparent is not None:
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
