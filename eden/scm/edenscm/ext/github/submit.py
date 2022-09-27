# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import error
from edenscm.i18n import _
from edenscm.node import hex, nullid

from . import github_repo_util, pullrequeststore


def submit(ui, repo, *args, **opts):
    """Create or update GitHub pull requests."""
    if not github_repo_util.is_github_repo(repo):
        raise error.Abort(_("not a Git repo"))

    find_commits_on_branch(ui, repo)
    return 0


def find_commits_on_branch(ui, repo):
    parents = repo.dirstate.parents()
    if parents[0] == nullid:
        # Root commit?
        return

    store = pullrequeststore.PullRequestStore(repo)
    commits_to_process = [
        (node, github_repo_util.get_pull_request_for_node(node, store, repo[node]))
        for node in repo.nodes("sort(. %% public(), -rev)")
    ]

    # TODO: Take the list of commits_to_process and create/update pull requests,
    # as appropriate.
    for node, pr in commits_to_process:
        url = pr.as_url() if pr else "None"
        ui.status(f"{hex(node)}: {url}\n")
