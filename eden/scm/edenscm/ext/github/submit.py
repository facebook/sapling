# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex, nullid

from . import github_repo, pullrequeststore


def submit(ui, repo, *args, **opts):
    """Create or update GitHub pull requests."""
    if not github_repo.is_github_repo(repo):
        raise error.Abort(_("not a Git repo"))

    find_commits_on_branch(ui, repo)
    return 0


def find_commits_on_branch(ui, repo):
    parents = repo.dirstate.parents()
    if parents[0] == nullid:
        # Root commit?
        return

    pr_store = pullrequeststore.PullRequestStore(repo)
    for node in repo.nodes("sort(. %% public(), -rev)"):
        # TODO(mbolin): Partition the nodes in this iterator into subgraphs
        # of commits that belong to a pull request. There can be one subgraph
        # (starting from the tip) that has no associated pull request:
        # - If there is a subgraph with no PR, decide whether it should be
        #   merged into an existing PR or if it should be the basis for a new
        #   PR.
        # - For all other subgraphs, update the PRs on GitHub, if appropriate.
        #   Note that the PR commit data and/or the PR body may need updating.
        pr = pr_store.find_pull_request(node)
        url = (
            f"https://github.com/{pr.owner}/{pr.name}/pull/{pr.number}"
            if pr
            else "None"
        )
        ui.status(f"{hex(node)}: {url}\n")
