import re

import ghstack.git
import ghstack.github
import ghstack.github_utils
import ghstack.sapling_shell
import ghstack.shell
from ghstack.ghs_types import GitCommitHash


def main(pull_request: str,
         remote_name: str,
         github: ghstack.github.GitHubEndpoint,
         sh: ghstack.shell.Shell,
         github_url: str) -> None:
    import ghstack
    if isinstance(sh, ghstack.sapling_shell.SaplingShell):
        import ghstack.sapling_land
        return ghstack.sapling_land.main(pull_request, remote_name, github, sh, github_url)

    # We land the entire stack pointed to by a URL.
    # Local state is ignored; PR is source of truth
    # Furthermore, the parent commits of PR are ignored: we always
    # take the canonical version of the patch from any given pr

    params = ghstack.github_utils.parse_pull_request(pull_request)
    default_branch = ghstack.github_utils.get_github_repo_info(
        github=github,
        sh=sh,
        repo_owner=params["owner"],
        repo_name=params["name"],
        github_url=github_url,
        remote_name=remote_name,
    )["default_branch"]
    orig_ref = ghstack.github_utils.lookup_pr_to_orig_ref(
        github,
        owner=params["owner"],
        name=params["name"],
        number=params["number"],
    )

    # Get up-to-date
    sh.git("fetch", "--prune", remote_name)
    remote_orig_ref = remote_name + "/" + orig_ref
    base = GitCommitHash(sh.git("merge-base", f"{remote_name}/{default_branch}", remote_orig_ref))

    # compute the stack of commits in chronological order (does not
    # include base)
    stack = ghstack.git.parse_header(
        sh.git("rev-list", "--reverse", "--header", "^" + base, remote_orig_ref),
        github_url=github_url,
    )

    # Switch working copy
    try:
        prev_ref = sh.git("symbolic-ref", "--short", "HEAD")
    except RuntimeError:
        prev_ref = sh.git("rev-parse", "HEAD")

    # If this fails, we don't have to reset
    sh.git("checkout", f"{remote_name}/{default_branch}")

    try:
        # Compute the metadata for each commit
        stack_orig_refs = []
        for s in stack:
            pr_resolved = s.pull_request_resolved
            # We got this from GitHub, this better not be corrupted
            assert pr_resolved is not None

            stack_orig_refs.append(ghstack.github_utils.lookup_pr_to_orig_ref(
                github,
                owner=pr_resolved.owner,
                name=pr_resolved.repo,
                number=pr_resolved.number))

        # OK, actually do the land now
        for orig_ref in stack_orig_refs:
            try:
                sh.git("cherry-pick", f"{remote_name}/{orig_ref}")
            except BaseException:
                sh.git("cherry-pick", "--abort")
                raise

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

        for orig_ref in stack_orig_refs:
            # TODO: regex here so janky
            base_ref = re.sub(r'/orig$', '/base', orig_ref)
            head_ref = re.sub(r'/orig$', '/head', orig_ref)
            sh.git("push", remote_name, f"{remote_name}/{head_ref}:{base_ref}")

        # All good! Push!
        sh.git("push", remote_name, f"HEAD:refs/heads/{default_branch}")

        # Delete the branches
        for orig_ref in stack_orig_refs:
            # TODO: regex here so janky
            base_ref = re.sub(r'/orig$', '/base', orig_ref)
            head_ref = re.sub(r'/orig$', '/head', orig_ref)
            sh.git("push", remote_name, "--delete", orig_ref, base_ref, head_ref)

    finally:
        sh.git("checkout", prev_ref)
