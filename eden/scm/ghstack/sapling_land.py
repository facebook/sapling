import json
import re

import ghstack.github
import ghstack.github_utils
import ghstack.sapling_shell

from sapling import error
from sapling.i18n import _


def main(
    pull_request: str,
    remote_name: str,
    github: ghstack.github.GitHubEndpoint,
    sh: ghstack.sapling_shell.SaplingShell,
    github_url: str,
) -> None:
    """The general approach to land is:

    - Find the /orig commit that corresponds to the PR.
    - Take all of the commits in the stack that correspond to /orig
      and rebase them on the tip of the default branch.
    - Advance /base to /head for each PR in the stack to "close" each PR.
    - Do `hg push` to push the rebased orig commits upstream.
    """
    # Ensure the pull_request argument (which is a URL) parses.
    params = ghstack.github_utils.parse_pull_request(pull_request)
    repo = ghstack.github_utils.get_github_repo_info(
        github=github,
        sh=sh,
        repo_owner=params["owner"],
        repo_name=params["name"],
        github_url=github_url,
        remote_name=remote_name,
    )
    default_branch = repo["default_branch"]
    repo_id = repo["id"]

    orig_ref = ghstack.github_utils.lookup_pr_to_orig_ref(
        github,
        owner=params["owner"],
        name=params["name"],
        number=params["number"],
    )

    orig_oid = ghstack.github_utils.get_commit_and_tree_for_ref(
        github=github, repo_id=repo_id, ref=orig_ref
    )["commit"]

    # Do a `pull` so we have the latest commit for the default branch locally.
    sh.run_sapling_command("pull")
    default_branch_oid = sh.run_sapling_command(
        "log", "-T", "{node}", "-r", default_branch, "--limit", "1"
    )
    base = sh.run_sapling_command(
        "log", "-T", "{node}", "-r", f"ancestor({orig_oid}, {default_branch_oid})"
    )

    stack = ghstack.git.parse_header(
        # pyre-ignore[6]
        sh.git("rev-list", "--reverse", "--header", "^" + base, orig_oid),
        github_url=github_url,
    )
    if not stack:
        raise error.Abort(
            _("No stack commits found between base %r and head %r.") % (base, orig_oid)
        )

    starting_parent = sh.run_sapling_command("whereami")

    try:
        # Compute the metadata for each commit
        stack_orig_refs = []
        for s in stack:
            pr_resolved = s.pull_request_resolved
            # We got this from GitHub, this better not be corrupted
            if pr_resolved is None:
                raise error.Abort(
                    _("Couldn't find PR info in commit header for %r.") % s.oid
                )

            assert pr_resolved is not None

            stack_orig_refs.append(
                ghstack.github_utils.lookup_pr_to_orig_ref(
                    github,
                    owner=pr_resolved.owner,
                    name=pr_resolved.repo,
                    number=pr_resolved.number,
                )
            )

        # Rebase each commit we are landing onto the default branch. Be careful
        # not to rebase further descendants since they may conflict.
        stdout = sh.run_sapling_command(
            "rebase",
            *(x for s in stack for x in ("-r", s.oid)),
            "-d",
            default_branch_oid,
            "-q",
            "-T",
            "{nodechanges|json}",
        )

        mappings = json.loads(stdout) if stdout else None
        stack_top_oid = stack[-1].oid
        if mappings:
            if starting_parent in mappings:
                # Move back to our starting position in case rebased moved us (i.e.
                # the user has the stack checked out). The user may have further
                # descendant commits we didn't rebase, so we don't want to leave
                # them on a rebased commit. Also, if the "push" fails, we want to
                # leave them in their starting position.
                sh.run_sapling_command("goto", "-q", starting_parent)

            # Hide the rebased commits we created. They are internal to "land",
            # and we don't want them to be visible if the "push" fails.
            sh.run_sapling_command(
                "hide", "-q", *(x for v in mappings.values() for x in ("-r", v[0]))
            )

            # If only a subset of commits were rebased, that means some commits
            # were "rebased out". There is a higher likelihood of a semantic
            # change, so make the user update their stack.
            if len(mappings) != len(stack):
                raise error.Abort(
                    _(
                        "One or more commits in stack already exists on %s.\nPlease manually rebase and update stack."
                    )
                    % (default_branch)
                )

            push_rev = mappings[stack_top_oid][0]
        else:
            # No mappings means the stack is already based on the branch head.
            push_rev = stack_top_oid

        # Advance base to head to "close" the PR for all PRs.
        # This has to happen before the push because the push
        # will trigger a closure, but we want a *merge*.  This should
        # happen after the cherry-pick, because the cherry-picks can
        # fail
        # TODO: It might be helpful to advance orig to reflect the true
        # state of upstream at the time we are doing the land, and then
        # directly *merge* head into base, so that the PR accurately
        # reflects what we ACTUALLY merged to master, as opposed to
        # this synthetic thing I'm doing right now just to make it look
        # like the PR got closed

        # Note there is an experimental batch API, UpdateRefsInput, that might
        # be more efficient once it is stable:
        # https://docs.github.com/en/graphql/reference/input-objects#updaterefsinput
        for orig_ref in stack_orig_refs:
            # TODO: regex here so janky
            base_ref = re.sub(r"/orig$", "/base", orig_ref)
            head_ref = re.sub(r"/orig$", "/head", orig_ref)
            ghstack.github_utils.update_ref(
                github=github, repo_id=repo_id, ref=base_ref, target_ref=head_ref
            )

        # All good! Push!
        sh.run_sapling_command("push", "--rev", push_rev, "--to", default_branch)

        # Delete the branches
        for orig_ref in stack_orig_refs:
            # TODO: regex here so janky
            base_ref = re.sub(r"/orig$", "/base", orig_ref)
            head_ref = re.sub(r"/orig$", "/head", orig_ref)
            sh.git("push", remote_name, "--delete", orig_ref, base_ref, head_ref)

    finally:
        # Need tighter try block to make this meaningful?
        pass
