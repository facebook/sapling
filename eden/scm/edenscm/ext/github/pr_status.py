# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import smartset, util
from edenscm.ext.github import graphql
from edenscm.ext.github.github_repo_util import get_pull_request_for_node
from edenscm.ext.github.pullrequeststore import PullRequestStore


def setup_smartset_prefetch():
    smartset.prefetchtemplatekw.update(
        {
            "github_pull_request_state": ["pr_status"],
        }
    )
    smartset.prefetchtable["pr_status"] = _prefetch


def _prefetch(repo, ctx_iter):
    for ctx in ctx_iter:
        store = PullRequestStore(repo)
        pr = get_pull_request_for_node(ctx.node(), store, ctx)
        if pr:
            repo.ui.debug("prefetch GitHub for pr%r\n" % pr.number)
            get_pr_data(repo, pr)
        yield ctx


def _memoize(f):
    def helper(repo, pr):
        if not util.safehasattr(repo, "_pr_status"):
            repo._pr_status = {}
        key = (repo, pr)
        if key not in repo._pr_status:
            val = f(repo, pr)
            repo._pr_status[key] = val
        return repo._pr_status[key]

    return helper


@_memoize
def get_pr_data(_repo, pr):
    return graphql.get_pull_request_data(pr)
