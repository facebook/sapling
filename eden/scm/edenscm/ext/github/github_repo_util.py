# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
import subprocess
from dataclasses import dataclass
from functools import lru_cache
from typing import Optional

from edenscm import error, git
from edenscm.i18n import _
from edenscm.result import Err, Ok, Result


class NotGitHubRepoError:
    # we can add a 'kind' enum attribute to differentiate 'Not Git' and
    # 'Git but not GitHub' cases later if needed
    def __init__(self, message: str):
        self.message = message


@dataclass(eq=True, frozen=True)
class GitHubRepo:
    # If GitHub Enterprise, this is the Enterprise hostname; otherwise, it is
    # "github.com".
    hostname: str
    # In GitHub, a "RepositoryOwner" is either an "Organization" or a "User":
    # https://docs.github.com/en/graphql/reference/interfaces#repositoryowner
    owner: str
    # Name of the GitHub repo within the organization.
    name: str

    def to_url(self) -> str:
        return f"https://{self.hostname}/{self.owner}/{self.name}"

    def as_gh_repo_arg(self) -> str:
        """the value to use with --repo for the GitHub CLI"""
        if self.hostname == "github.com":
            return f"{self.owner}/{self.name}"
        else:
            return f"{self.hostname}/{self.owner}/{self.name}"


def is_github_repo(repo) -> bool:
    """Returns True if it's a GitHub repo"""
    return find_github_repo(repo).is_ok()


def check_github_repo(repo) -> GitHubRepo:
    """Returns GitHubRepo if the URI for the upstream repo appears to be an
    identifier for a consumer GitHub or GitHub Enterprise repository; otherwise,
    raises error.Abort() with an appropriate message.
    """
    result = find_github_repo(repo)
    if result.is_ok():
        return result.unwrap()
    else:
        raise error.Abort(result.unwrap_err().message)


def find_github_repo(repo) -> Result[GitHubRepo, NotGitHubRepoError]:
    """Returns a Rust like Result[GitHubRepo, NotGitHubRepoError].

    Checks if the URI for the upstream repo appears to be an identifier for a consumer
    GitHub or GitHub Enterprise repository.
    """
    if not git.isgitpeer(repo):
        return Err(NotGitHubRepoError(message=_("not a Git repo")))

    url = None
    try:
        url = repo.ui.paths.get("default", "default-push").url
    except AttributeError:  # ex. paths.default is not set
        return Err(NotGitHubRepoError(message=_("could not read paths.default")))

    hostname = url.host
    if hostname == "github.com" or is_github_enterprise_hostname(hostname):
        url_arg = str(url)
        github_repo = parse_github_repo_from_github_url(url_arg)
        if github_repo:
            return Ok(github_repo)
        else:
            return Err(
                NotGitHubRepoError(
                    message=_("could not parse GitHub URI: %s") % url_arg
                )
            )

    err_msg = _(
        (
            "either %s is not a GitHub (Enterprise) hostname or you are not logged in.\n"
            + "Authenticate using the GitHub CLI: `gh auth login --git-protocol https --hostname %s`"
        )
        % (hostname, hostname)
    )
    return Err(NotGitHubRepoError(message=err_msg))


@lru_cache
def is_github_enterprise_hostname(hostname: str) -> bool:
    """Returns True if the user is authenticated (via gh, the GitHub CLI)
    to the GitHub Enterprise instance for the specified hostname. Note that
    if this returns False, that does not mean that hostname is *not* part of a
    GitHub Enterprise account, only that Sapling does not know about it because
    the user is not authenticated.
    """
    try:
        subprocess.check_call(
            ["gh", "auth", "status", "--hostname", hostname],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        # The user may not be authenticated or may not even have `gh` installed.
        return False
    return True


def parse_github_repo_from_github_url(url: str) -> Optional[GitHubRepo]:
    """Assumes the caller has already verified that `url` is a "GitHub URL",
    i.e., it refers to a repo hosted on consumer github.com or a GitHub
    Enterprise instance.

    Parses the following URL formats:

    https://github.com/bolinfest/escoria-demo-game
    https://github.com/bolinfest/escoria-demo-game.git
    git@github.com:bolinfest/escoria-demo-game.git
    git+ssh://git@github.com:bolinfest/escoria-demo-game.git
    ssh://git@github.com/bolinfest/escoria-demo-game.git

    and returns:

    https://github.com/bolinfest/escoria-demo-game

    which is suitable for constructing URLs to pull requests.

    >>> parse_github_repo_from_github_url("https://github.com/bolinfest/escoria-demo-game").to_url()
    'https://github.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("https://github.com/bolinfest/escoria-demo-game.git").to_url()
    'https://github.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("git@github.com:bolinfest/escoria-demo-game.git").to_url()
    'https://github.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("git+ssh://git@github.com:bolinfest/escoria-demo-game.git").to_url()
    'https://github.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("ssh://git@github.com/bolinfest/escoria-demo-game.git").to_url()
    'https://github.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("ssh://git@github.com:bolinfest/escoria-demo-game.git").to_url()
    'https://github.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("ssh://git@foo.bar.com/bolinfest/escoria-demo-game.git").to_url()
    'https://foo.bar.com/bolinfest/escoria-demo-game'
    >>> parse_github_repo_from_github_url("git+ssh://git@foo.bar.com:bolinfest/escoria-demo-game.git").to_url()
    'https://foo.bar.com/bolinfest/escoria-demo-game'
    """
    pattern = r"(?:https://([^/]+)|(?:git\+ssh://|ssh://)?git@([^:/]+))[:/]([^/]+)\/(.+?)(?:\.git)?$"
    match = re.match(pattern, url)
    if match:
        hostname1, hostname2, owner, repo = match.groups()
        return GitHubRepo(hostname1 or hostname2, owner, repo)
    else:
        return None
