# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# megarepo.py - support for cross repo commit resolution

"""provides support for cross repo commit resolution

Allows for queries such as `@prog@ log -r deadbeef` to find the commit in the
local repo which corresponds to commit "deadbeef" in a mirror repo.
"""

import re
from typing import Optional

from sapling import (
    autopull,
    commands,
    error,
    extensions,
    localrepo,
    mutation,
    namespaces,
    registrar,
    util,
)
from sapling.autopull import deferredpullattempt, pullattempt
from sapling.i18n import _
from sapling.namespaces import namespace
from sapling.node import bin, hex

configtable = {}
configitem = registrar.configitem(configtable)

# The other repo to translate commit hashes from.
configitem("megarepo", "transparent-lookup", default=None)

namespacepredicate = registrar.namespacepredicate()
autopullpredicate = registrar.autopullpredicate()


@namespacepredicate("megarepo", priority=100)
def _megareponamespace(_repo) -> namespace:
    """Namespace to map another repo's commits to corresponding commit in local repo.

    For performance reasons, this namespace only uses already cached
    translations. The cached translations are populated by the
    autopull predicate.

    """

    def cachedname(repo, commithash):
        if localnode := getattr(repo, "_xrepo_lookup_cache", {}).get(commithash):
            return [localnode]

        return []

    return namespaces.namespace(
        listnames=lambda _repo: [],
        namemap=cachedname,
        nodemap=lambda _repo, _node: [],
        user_only=True,
    )


@autopullpredicate("megarepo", priority=100)
def _xrepopull(repo, name) -> deferredpullattempt:
    """Autopull a commit from another repo.

    First the xrepo commit is translated to the coresponding commit of
    the local repo. Then the local commit is pulled.

    We defer our autopull work so we can avoid all xrepo queries in
    the case "name" was resolved as a local commit by a higher
    priority autopull predicate.

    """

    def generateattempt() -> Optional[pullattempt]:
        localnode = _xrepotranslate(repo, name)
        if not localnode:
            return None
        return autopull.pullattempt(headnodes=[localnode])

    return deferredpullattempt(generate=generateattempt)


_commithashre = re.compile(r"\A[0-9a-f]{6,40}\Z")


def _xrepotranslate(repo, commithash):
    if not _commithashre.match(commithash):
        return None

    if not repo.nullableedenapi:
        return None

    # Avoid xrepo query if commithash is now known to be of this repo.
    # This would be the case if a previous autopull already found it.
    if commithash in repo:
        return None

    cache = getattr(repo, "_xrepo_lookup_cache", None)
    if cache is None:
        return None

    if commithash in cache:
        return cache[commithash]

    commit_ids = {commithash}

    localnode = None
    for xrepo in repo.ui.configlist("megarepo", "transparent-lookup"):
        if xrepo == repo.ui.config("remotefilelog", "reponame"):
            continue

        if len(commithash) == 40:
            xnode = bin(commithash)
        else:
            try:
                repo.ui.note_err(
                    _("looking up prefix %s in repo %s\n") % (commithash, xrepo)
                )
                xnode = next(repo._http_prefix_lookup([commithash], reponame=xrepo))
            except error.RepoLookupError:
                continue

        if xnode in cache:
            return cache[xnode]

        commit_ids.add(xnode)

        repo.ui.note_err(_("translating %s from repo %s\n") % (hex(xnode), xrepo))
        translated = list(
            repo.edenapi.committranslateids([{"Hg": xnode}], "Hg", fromrepo=xrepo)
        )
        if len(translated) == 1:
            localnode = translated[0]["translated"]["Hg"]
            repo.ui.status_err(
                _("translated %s@%s to %s\n") % (hex(xnode), xrepo, hex(localnode))
            )

        if localnode:
            break

    for commit_id in commit_ids:
        # Cache negative value (i.e. localnode=None) to avoid repeated queries.
        cache[commit_id] = localnode

    return localnode


def reposetup(_ui, repo) -> None:
    repo._xrepo_lookup_cache = util.lrucachedict(100)


# If commit is marked as a lossy commit, abort if abort, else warn.
def _check_for_lossy_commit_usage(repo, commit, abort):
    if not commit or not commit in repo:
        return

    ctx = repo[commit]
    if "created_by_lossy_conversion" in ctx.extra():
        if abort:
            raise error.Abort(
                _("operating on lossily synced commit %s disallowed by default")
                % ctx.hex(),
                hint=_(
                    "perform operation in source-of-truth repo, or specify '--config megarepo.lossy-commit-action=ignore' to bypass"
                ),
            )
        else:
            repo.ui.warn(
                _("warning: operating on lossily synced commit %s\n") % ctx.hex()
            )


def extsetup(ui) -> None:
    action = ui.config("megarepo", "lossy-commit-action")
    should_abort = action == "abort"

    def _wrap_commit_ctx(orig, repo, ctx, **opts):
        to_check = set()

        # Check mutation info. Some commands like "metaedit" only set this.
        if mutinfo := ctx.mutinfo():
            to_check.update(mutation.nodesfrominfo(mutinfo.get("mutpred")) or [])

        # Check ad-hoc "source" extras. Some commands like "graft" only set this.
        if not to_check:
            to_check.update(v for (k, v) in ctx.extra().items() if k.endswith("source"))

        for c in to_check:
            _check_for_lossy_commit_usage(repo, c, should_abort)

        return orig(repo, ctx, **opts)

    # Wrap backout separately since it doesn't set any commit extras.
    def _wrap_backout(orig, ui, repo, node=None, rev=None, **opts):
        _check_for_lossy_commit_usage(repo, node or rev, should_abort)
        return orig(ui, repo, node, rev, **opts)

    if action in {"warn", "abort"}:
        extensions.wrapfunction(
            localrepo.localrepository, "commitctx", _wrap_commit_ctx
        )
        extensions.wrapcommand(commands.table, "backout", _wrap_backout)
    elif action and not action == "ignore":
        ui.warn(_("invalid megarepo.lossy-commit-action '%s'\n") % action)
