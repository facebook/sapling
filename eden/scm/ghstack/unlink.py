import logging
import re
import textwrap
from dataclasses import dataclass
from typing import List, Optional, Set

import ghstack.diff
import ghstack.git
import ghstack.github
import ghstack.github_utils
import ghstack.shell
from ghstack.ghs_types import GitCommitHash, GitTreeHash


RE_GHSTACK_SOURCE_ID = re.compile(r'^ghstack-source-id: (.+)\n?', re.MULTILINE)


@dataclass
class SimpleCommitHeader:
    """Reduced version of CommitHeader that is a simple dataclass so the fields
    are amenable to being rewritten."""
    commit_id: GitCommitHash
    commit_msg: str
    tree: GitTreeHash


def main(*,
         commits: Optional[List[str]] = None,
         github: ghstack.github.GitHubEndpoint,
         sh: ghstack.shell.Shell,
         repo_owner: Optional[str] = None,
         repo_name: Optional[str] = None,
         github_url: str,
         remote_name: str) -> GitCommitHash:
    # If commits is empty, we unlink the entire stack
    #
    # For now, we only process commits on our current
    # stack, because we have no way of knowing how to
    # "restack" for other commits.

    default_branch = ghstack.github_utils.get_github_repo_info(
        github=github,
        sh=sh,
        repo_owner=repo_owner,
        repo_name=repo_name,
        github_url=github_url,
        remote_name=remote_name,
    )["default_branch"]

    # Parse the commits
    parsed_commits: Optional[Set[GitCommitHash]] = None
    if commits:
        parsed_commits = set()
        for c in commits:
            parsed_commits.add(GitCommitHash(sh.git("rev-parse", c)))

    base = GitCommitHash(sh.git("merge-base", f"{remote_name}/{default_branch}", "HEAD"))

    # compute the stack of commits in chronological order (does not
    # include base)
    stack = ghstack.git.split_header(
        sh.git("rev-list", "--reverse", "--header", "^" + base, "HEAD"))
    stack = [SimpleCommitHeader(commit_id=GitCommitHash(s.commit_id()), commit_msg=s.commit_msg(), tree=s.tree()) for s in stack]

    # sanity check the parsed_commits
    if parsed_commits is not None:
        stack_commits = set()
        for s in stack:
            stack_commits.add(s.commit_id)
        invalid_commits = parsed_commits - stack_commits
        if invalid_commits:
            raise RuntimeError(
                "unlink can only process commits which are on the "
                "current stack; these commits are not:\n{}"
                .format("\n".join(invalid_commits)))

    # Run the interactive rebase.  Don't start rewriting until we
    # hit the first commit that needs it.
    head = base
    rewriting = False

    for index, s in enumerate(stack):
        commit_id = s.commit_id
        should_unlink = parsed_commits is None or commit_id in parsed_commits
        if not rewriting and not should_unlink:
            # Advance HEAD without reconstructing commit
            head = commit_id
            continue

        rewriting = True
        commit_msg = s.commit_msg
        logging.debug("-- commit_msg:\n{}".format(textwrap.indent(commit_msg, '   ')))
        if should_unlink:
            commit_msg = RE_GHSTACK_SOURCE_ID.sub(
                '',
                ghstack.diff.re_pull_request_resolved_w_sp(github_url).sub('', commit_msg)
            )
            logging.debug("-- edited commit_msg:\n{}".format(
                textwrap.indent(commit_msg, '   ')))

        if isinstance(sh, ghstack.sapling_shell.SaplingShell):
            # After rewriting the commit message via metaedit, update the
            # hashes for the desecendant commits.
            mappings = sh.rewrite_commit_message(commit_id, commit_msg)
            for rewritten_commit in stack[index:]:
                new_id = mappings[rewritten_commit.commit_id]
                # Note that we do not update the .tree field of rewritten_commit,
                # but the Eden codepath never reads it.
                rewritten_commit.commit_id = GitCommitHash(new_id)

            # It is not safe to update a Set in-place, so we must determine the
            # mutations and then apply them.
            if parsed_commits is not None:
                to_add = []
                to_remove = []
                for p in parsed_commits:
                    # p may not be in mappings if it was processed earlier in
                    # the stack.
                    new_id = mappings.get(p)
                    if new_id:
                        to_add.append(GitCommitHash(new_id))
                        to_remove.append(p)
                if to_add:
                    parsed_commits = (parsed_commits - set(to_remove)).union(set(to_add))

            # `head` is not really used in the Eden codepath, but we maintain
            # it so the return value is consistent with the Git codepath.
            head = GitCommitHash(stack[index].commit_id)
        else:
            head = sh.git_commit_tree(
                s.tree,
                "-p", head,
                input=commit_msg)

    if sh.is_git():
        sh.git('reset', '--soft', head)

        logging.info("""
Diffs successfully unlinked!

To undo this operation, run:

    git reset --soft {}
""".format(s.commit_id))

    return head
