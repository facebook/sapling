# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import git


def is_github_repo(repo) -> bool:
    """Create or update GitHub pull requests."""
    if not git.isgitpeer(repo):
        return False

    try:
        return repo.ui.paths.get("default", "default-push").url.host == "github.com"
    except AttributeError:  # ex. paths.default is not set
        return False
