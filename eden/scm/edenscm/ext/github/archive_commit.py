# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import Callable, Optional

from edenscm import error, gituser, gpg
from edenscm.i18n import _

from . import gh_submit
from .gh_submit import Repository
from .run_git_command import run_git_command


async def add_commit_to_archives(
    *,
    oid_to_archive: str,
    ui,
    origin: str,
    repository: Repository,
    get_gitdir: Callable[[], str],
):
    """Takes the specified commit (oid_to_archive) and merges it into the
    appropriate archive branch for the (repo, username). GitHub will
    periodically garbage collect commits that are no longer part of a public
    branch, but we want to prevent this to ensure previous version of a PR can
    be viewed later, even after it has been updated via a force-push.

    oid_to_archive is the hex version of the commit hash to be merged into the
    archive branch.
    """
    username = await get_username(hostname=repository.hostname)
    if not username:
        raise error.Abort(_("could not determine GitHub username"))

    branch_name = f"sapling-pr-archive-{username}"
    # Try to merge the commit directly, though this may fail if oid_to_archive
    # has already been merged or if the branch has not been created before. We
    # try to merge without checking for the existence of the branch to try to
    # avoid a TOCTOU error.
    result = await gh_submit.merge_into_branch(
        hostname=repository.hostname,
        repo_id=repository.id,
        oid_to_merge=oid_to_archive,
        branch_name=branch_name,
    )
    if result.is_ok():
        return

    import json

    # TODO: Store Result.error as Dict so we don't have to parse it again.
    err = result.unwrap_err()
    response = None
    try:
        response = json.loads(err)
    except json.JSONDecodeError:
        # response is not guaranteed to be valid JSON.
        pass

    if response and _is_already_merged_error(response):
        # Nothing to do!
        return
    elif response and _is_branch_does_not_exist_error(response):
        # Archive branch does not exist yet, so initialize it with the current
        # oid_to_archive.
        result = await gh_submit.create_branch(
            hostname=repository.hostname,
            repo_id=repository.id,
            branch_name=branch_name,
            oid=oid_to_archive,
        )
        if result.is_err():
            raise error.Abort(
                _("unexpected error when trying to create branch %s with commit %s: %s")
                % (branch_name, oid_to_archive, result.unwrap_err())
            )
    elif response and _is_merge_conflict(response):
        # Git cannot do the merge on its own, so we need to generate our own
        # commit that merges the existing archive with the contents of
        # `oid_to_archive` to use as the new head for the archive branch.
        gitdir = get_gitdir()

        # We must fetch the archive branch because we need to have the commit
        # object locally in order to use it with commit-tree.
        run_git_command(["fetch", origin, branch_name], gitdir)
        # `git fetch --verbose` does not appear to include the hash, so we must
        # use `git ls-remote` to get it.
        ls_remote_args = ["ls-remote", origin, branch_name]
        ls_remote_output = (
            run_git_command(ls_remote_args, gitdir=gitdir).decode().rstrip()
        )
        # oid and ref name should be separated by a tab character, but we use
        # '\s+' just to be safe.
        match = re.match(r"^([0-9a-f]+)\s+.*$", ls_remote_output)
        if not match:
            raise error.Abort(
                _("unexpected output from `%s`: %s")
                % (" ".join(ls_remote_args), ls_remote_output)
            )

        branch_name_oid = match[1]

        # This will be the tree to use for the merge commit. We could use the
        # tree for either `oid_to_archive` or `branch_name_oid`, but since
        # `oid_to_archive` appears to be "newer," we prefer it as it seems less
        # likely to cause a merge conflict the next time we update the archive
        # branch.
        tree_oid = (
            run_git_command(
                ["log", "--max-count=1", "--format=%T", oid_to_archive], gitdir=gitdir
            )
            .decode()
            .rstrip()
        )

        # Synthetically create a new commit that has `oid_to_archive` and the
        # old branch head as parents and force-push it as the new branch head.
        user_name, user_email = gituser.get_identity_or_raise(ui)
        keyid = gpg.get_gpg_keyid(ui)
        gpg_args = [f"-S{keyid}"] if keyid else []
        commit_tree_args = (
            [
                "-c",
                f"user.name={user_name}",
                "-c",
                f"user.email={user_email}",
                "commit-tree",
            ]
            + gpg_args
            + [
                "-m",
                "merge commit for archive created by Sapling",
                "-p",
                oid_to_archive,
                "-p",
                branch_name_oid,
                tree_oid,
            ]
        )
        merge_commit = (
            run_git_command(
                commit_tree_args,
                gitdir,
            )
            .decode()
            .rstrip()
        )
        refspec = f"{merge_commit}:refs/heads/{branch_name}"
        git_push_args = [
            "push",
            "--force",
            origin,
            refspec,
        ]
        ui.status_err(_("force-pushing %s to %s\n") % (refspec, origin))
        run_git_command(git_push_args, gitdir)
    else:
        raise error.Abort(
            _("unexpected error when trying to merge %s into %s: %s")
            % (oid_to_archive, branch_name, err)
        )


def _is_already_merged_error(response) -> bool:
    r"""
    >>> response = {
    ...   "data": {
    ...     "mergeBranch": None
    ...   },
    ...   "errors": [
    ...     {
    ...       "type": "UNPROCESSABLE",
    ...       "path": [
    ...         "mergeBranch"
    ...       ],
    ...       "locations": [
    ...         {
    ...           "line": 2,
    ...           "column": 3
    ...         }
    ...       ],
    ...       "message": "Failed to merge: \"Already merged\""
    ...     }
    ...   ]
    ... }
    >>> _is_already_merged_error(response)
    True
    """
    errors = response.get("errors")
    if not errors or not isinstance(errors, list):
        return False
    for err in errors:
        if err.get("type") != "UNPROCESSABLE":
            continue
        message = err.get("message")
        if isinstance(message, str) and "Already merged" in message:
            return True
    return False


def _is_merge_conflict(response) -> bool:
    r"""
    >>> response = {
    ...   "data": {
    ...     "mergeBranch": None
    ...   },
    ...   "errors": [
    ...     {
    ...       "type": "UNPROCESSABLE",
    ...       "path": [
    ...         "mergeBranch"
    ...       ],
    ...       "locations": [
    ...         {
    ...           "line": 3,
    ...           "column": 3
    ...         }
    ...       ],
    ...       "message": "Failed to merge: \"Merge conflict\""
    ...     }
    ...   ]
    ... }
    >>> _is_merge_conflict(response)
    True
    """
    errors = response.get("errors")
    if not errors or not isinstance(errors, list):
        return False
    for err in errors:
        if err.get("type") != "UNPROCESSABLE":
            continue
        message = err.get("message")
        if isinstance(message, str) and "Merge conflict" in message:
            return True
    return False


def _is_branch_does_not_exist_error(response) -> bool:
    r"""
    >>> response = {
    ...   "data": {
    ...     "mergeBranch": None
    ...   },
    ...   "errors": [
    ...     {
    ...       "type": "NOT_FOUND",
    ...       "path": [
    ...         "mergeBranch"
    ...       ],
    ...       "locations": [
    ...         {
    ...           "line": 2,
    ...           "column": 3
    ...         }
    ...       ],
    ...       "message": "No such base."
    ...     }
    ...   ]
    ... }
    >>> _is_branch_does_not_exist_error(response)
    True
    """
    errors = response.get("errors")
    if not errors or not isinstance(errors, list):
        return False
    for err in errors:
        if err.get("type") != "NOT_FOUND":
            continue
        message = err.get("message")
        if isinstance(message, str) and "No such base." in message:
            return True
    return False


async def get_username(hostname: str) -> Optional[str]:
    """Returns the username for the user authenticated with the GitHub CLI."""
    result = await gh_submit.get_username(hostname=hostname)
    return result.ok()
