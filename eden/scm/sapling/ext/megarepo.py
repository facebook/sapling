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

import bindings

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
from sapling.ext import fbcodereview
from sapling.i18n import _
from sapling.namespaces import namespace
from sapling.node import bin, hex


# The other repo to translate commit hashes from.


namespacepredicate = registrar.namespacepredicate()
autopullpredicate = registrar.autopullpredicate()


@namespacepredicate("megarepo", priority=100)
def _megareponamespace(_repo) -> namespace:
    """Namespace to map another repo's commits to corresponding commit in local repo.

    For performance reasons, this namespace only uses already cached
    translations. The cached translations are populated by the
    autopull predicate.

    """

    def cachedname(repo, commitid):
        if localnode := getattr(repo, "_xrepo_lookup_cache", {}).get(commitid):
            return [localnode]

        return []

    return namespaces.namespace(
        listnames=lambda _repo: [],
        namemap=cachedname,
        nodemap=lambda _repo, _node: [],
        user_only=True,
    )


@autopullpredicate("megarepo", priority=100, rewritepullrev=True)
def _xrepopull(repo, name, rewritepullrev=False) -> Optional[pullattempt]:
    """Autopull a commit from another repo.

    First the xrepo commit is translated to the corresponding commit of
    the local repo. Then the local commit is pulled.

    We defer our autopull work so we can avoid all xrepo queries in
    the case "name" was resolved as a local commit by a higher
    priority autopull predicate.

    """

    def generateattempt() -> Optional[pullattempt]:
        if not may_need_xrepotranslate(repo, name):
            return None

        localnode = xrepotranslate(repo, name)
        if not localnode or localnode in repo:
            return None

        if not repo.ui.configbool("ui", "autopullcommits"):
            return None

        return autopull.pullattempt(headnodes=[localnode])

    if rewritepullrev:
        if repo.ui.configbool("megarepo", "rewrite-pull-rev", True):
            return generateattempt()
    elif may_need_xrepotranslate(repo, name):
        # deferredpullattempt disables "titles" namespace!
        return deferredpullattempt(generate=generateattempt)

    # Returning None allows "titles" namespace lookup.
    return None


_commithashre = re.compile(r"\A[0-9a-f]{6,40}\Z")
_diffidre = re.compile(r"\AD\d+\Z")


def may_need_xrepotranslate(repo, commitid) -> bool:
    """Test if 'commitid' may trigger xrepo lookup without asking remote servers.
    Returns True if the commitid might trigger xrepo lookup.
    Returns False if the commitid will NOT trigger xrepo lookup.
    This is a subset of `_xrepotranslate` but avoids remote lookups.
    """
    if (
        not _diffidre.match(commitid)
        and not _commithashre.match(commitid)
        and "/" not in commitid
    ):
        return False
    if not repo.nullableedenapi or commitid in repo:
        return False
    return True


def _diff_to_commit(repo, commitid):
    try:
        # First try looking up the diff using our local repo name. This can work xrepo
        # since Phabricator has knowledge of a commit landing to multiple different repos
        # (one of which might be our repo).
        if resolved := fbcodereview.diffidtonode(
            repo,
            commitid[1:],
        ):
            return hex(resolved)
    except Exception as ex:
        repo.ui.note_err(_("error resolving diff %s to commit: %s\n") % (commitid, ex))

    for xrepo in repo.ui.configlist("megarepo", "transparent-lookup"):
        if xrepo == repo.ui.config("remotefilelog", "reponame"):
            continue

        try:
            # Now try using the other repo's name. If Phabricator hasn't noticed the
            # commit appearing in our local repo yet, we need to resolve the diff number
            # using the "native" repo.
            if resolved := fbcodereview.diffidtonode(
                repo,
                commitid[1:],
                localreponame=xrepo,
            ):
                return hex(resolved)
        except Exception as ex:
            repo.ui.note_err(
                _("error resolving diff %s to commit in %s: %s\n")
                % (commitid, xrepo, ex)
            )

    return None


def xrepotranslate(repo, commitid):
    commit_ids = {commitid}

    if _diffidre.match(commitid):
        # If it looks like a phabricator diff, first resolve the diff ID to a commit hash.
        if resolved := _diff_to_commit(repo, commitid):
            commitid = resolved
            commit_ids.add(commitid)

    if not _commithashre.match(commitid) and "/" not in commitid:
        return None

    if not repo.nullableedenapi:
        return None

    cache = getattr(repo, "_xrepo_lookup_cache", None)
    if cache is None:
        return None

    # Avoid xrepo query if commithash is now known to be of this repo.
    # This would be the case if a previous autopull already found it.
    if commitid in repo:
        node = repo[commitid].node()
        for id in commit_ids:
            cache[id] = node
        return node

    if commitid in cache:
        return cache[commitid]

    localnode = None
    for xrepo in repo.ui.configlist("megarepo", "transparent-lookup"):
        if xrepo == repo.ui.config("remotefilelog", "reponame"):
            continue

        xnode = None

        if "/" in commitid:
            bm_name = commitid.removeprefix(xrepo + "/")
            if bm_name != commitid:
                repo.ui.note_err(
                    _("looking up bookmark %s in repo %s\n") % (bm_name, xrepo)
                )
                xrepo_edenapi = bindings.edenapi.client(repo.ui._rcfg, reponame=xrepo)
                if xrepo_hash := xrepo_edenapi.bookmarks([bm_name]).get(bm_name):
                    commit_ids.add(xrepo_hash)
                    xnode = bin(xrepo_hash)
        elif len(commitid) == 40:
            xnode = bin(commitid)
        else:
            try:
                repo.ui.note_err(
                    _("looking up prefix %s in repo %s\n") % (commitid, xrepo)
                )
                xnode = next(repo._http_prefix_lookup([commitid], reponame=xrepo))
            except error.RepoLookupError:
                pass

        if xnode is None:
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

    def _wrap_commit_ctx(orig, repo, ctx, *args, **opts):
        to_check = set()

        # Check mutation info. Some commands like "metaedit" only set this.
        if mutinfo := ctx.mutinfo():
            to_check.update(mutation.nodesfrominfo(mutinfo.get("mutpred")) or [])

        # Check ad-hoc "source" extras. Some commands like "graft" only set this.
        if not to_check:
            to_check.update(v for (k, v) in ctx.extra().items() if k.endswith("source"))

        for c in to_check:
            _check_for_lossy_commit_usage(repo, c, should_abort)

        return orig(repo, ctx, *args, **opts)

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
