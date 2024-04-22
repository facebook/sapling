# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
mark commits as "Landed" on pull
"""
import typing as t
from collections import defaultdict

from ghstack.github_cli_endpoint import GitHubCLIEndpoint

from sapling import mutation, util, visibility
from sapling.ext.github.github_repo_util import find_github_repo
from sapling.ext.github.pr_parser import get_pull_request_for_context
from sapling.ext.github.pullrequest import get_pr_state, PullRequestId
from sapling.ext.github.pullrequeststore import PullRequestStore
from sapling.i18n import _, _n
from sapling.node import bin


Node = bytes


def cleanup_landed_pr_hook(ui, repo, **kwargs) -> None:
    """hook for cleanup_landed_pr

    This is used as a post hook of `pull` command.
    """
    cleanup_landed_pr(ui, repo)


def cleanup_landed_pr(ui, repo, dry_run: bool = False) -> None:
    """cleanup landed GitHub PRs

    If the repo is not a valid GitHub repo, just return.
    """
    # Don't run in tests by default since it requires more stuff to be mocked out.
    if not ui.configbool("github", "hide-landed-commits", not util.istest()):
        return

    github_repo = find_github_repo(repo).ok()
    if github_repo is None:
        # not a GitHubRepo, just return
        return

    try:
        pr_to_draft = _get_draft_commits(repo)
    except Exception:
        ui.warn(
            _(
                "warning: pr_marker was not able to mark commits as landed (try '@prog@ debugprmarker')\n"
            )
        )
        return

    try:
        to_hide, mutation_entries = _get_landed_commits(
            repo, pr_to_draft, github_repo.hostname
        )
    except Exception as e:
        ui.warn(
            _(
                "warning: failed to read from Github for landed commits (%r), not marking commits as landed\n"
            )
            % e
        )
        return

    if to_hide:
        _hide_commits(repo, to_hide, mutation_entries, dry_run)
        ui.status(
            _n(
                "marked %d commit as landed\n",
                "marked %d commits as landed\n",
                len(to_hide),
            )
            % len(to_hide)
        )


def _get_draft_commits(repo) -> t.Dict[PullRequestId, t.Set[Node]]:
    pr_to_draft = defaultdict(set)
    for ctx in repo.set("sort(draft() - obsolete(), -rev)"):
        pr = _get_pr_for_context(repo, ctx)
        if pr:
            pr_to_draft[pr].add(ctx.node())
    return pr_to_draft


def _get_pr_for_context(repo, ctx) -> t.Optional[PullRequestId]:
    store = PullRequestStore(repo)
    return get_pull_request_for_context(store, repo, ctx)


def _get_landed_commits(
    repo, pr_to_draft, hostname: str
) -> t.Tuple[t.Set[Node], t.List]:
    github = GitHubCLIEndpoint(hostname)
    to_hide = set()
    mutation_entries = []
    for pr, draft_nodes in pr_to_draft.items():
        pr_state = get_pr_state(github, pr)
        if pr_state["merged"]:
            public_node = bin(pr_state["merge_commit"])
            for draft_node in draft_nodes:
                to_hide.add(draft_node)
                mutation_entries.append(
                    mutation.createsyntheticentry(
                        repo, [draft_node], public_node, "land"
                    )
                )
    return to_hide, mutation_entries


def _hide_commits(repo, to_hide, mutation_entries, dry_run) -> None:
    if dry_run or not to_hide:
        return
    with repo.lock(), repo.transaction("pr_marker"):
        if mutation.enabled(repo):
            mutation.recordentries(repo, mutation_entries, skipexisting=False)
        if visibility.tracking(repo):
            visibility.remove(repo, to_hide)
