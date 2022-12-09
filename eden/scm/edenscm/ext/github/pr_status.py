# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List, Optional

from edenscm import smartset, util
from ghstack.github import get_github_endpoint

from . import graphql
from .github_repo_util import is_github_repo
from .pr_parser import get_pull_request_for_context
from .pullrequest import GraphQLPullRequest, PullRequestId
from .pullrequeststore import PullRequestStore


_PR_STATUS_PEEK_AHEAD = 30
_PR_STATUS_CACHE = "_pr_status_cache"


def setup_smartset_prefetch():
    smartset.prefetchtemplatekw.update(
        {
            "github_pull_request_state": ["pr_status"],
        }
    )
    smartset.prefetchtable["pr_status"] = _prefetch


def get_pull_request_data(repo, pr: PullRequestId) -> Optional[GraphQLPullRequest]:
    return _get_pull_request_data_list(repo, pr)[0]


def _prefetch(repo, ctx_iter):
    if not is_github_repo(repo):
        for ctx in ctx_iter:
            yield ctx
        return

    ui = repo.ui
    peek_ahead = ui.configint("githubprstatus", "peekahead", _PR_STATUS_PEEK_AHEAD)
    pr_store = PullRequestStore(repo)
    for batch in util.eachslice(ctx_iter, peek_ahead):
        cached = getattr(repo, _PR_STATUS_CACHE, {})
        pr_list = {get_pull_request_for_context(pr_store, ctx) for ctx in batch}

        pr_list = [pr for pr in pr_list if pr and pr not in cached]
        if pr_list:
            if ui.debugflag:
                ui.debug(
                    "prefetch GitHub PR status for %r\n"
                    % sorted([pr.number for pr in pr_list])
                )
            _get_pull_request_data_list(repo, *pr_list)

        # this is needed by smartset's iterctx method
        for ctx in batch:
            yield ctx


def _memoize(f):
    """Cache the result of the PR list and the result of each PR."""

    def helper(repo, *pr_list: PullRequestId) -> List[Optional[GraphQLPullRequest]]:
        pr_status_cache = getattr(repo, _PR_STATUS_CACHE, None)
        if pr_status_cache is None:
            repo._pr_status_cache = pr_status_cache = {}
        key = (repo, *pr_list)
        val_list = pr_status_cache.get(key)
        if val_list is None:
            val_list = f(repo, *pr_list)
            pr_status_cache[key] = val_list
            for pr, val in zip(pr_list, val_list):
                pr_status_cache[(repo, pr)] = [val]
        return val_list

    return helper


@_memoize
def _get_pull_request_data_list(
    _repo, *pr_list: PullRequestId
) -> List[Optional[GraphQLPullRequest]]:
    if pr_list:
        github = get_github_endpoint(pr_list[0].get_hostname())
        return graphql.get_pull_request_data_list(github, pr_list)
    else:
        return []
