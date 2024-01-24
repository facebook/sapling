# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Optional

from . import git


def determine_git_uri(git_flag: Optional[bool], source: str) -> Optional[str]:
    """Based on the git flag passed to clone and the source arg, determines the
    Git URI to use for cloning, if appropriate.

    git_flag is:
    - True if --git was specified as an option to clone.
    - False if --no-git was specified as an option to clone.
    - None if neither --git nor --no-git were specified.

    >>> determine_git_uri(True, "ssh://example.com/my/repo.git")
    'ssh://example.com/my/repo.git'
    >>> determine_git_uri(None, "git+ssh://example.com/my/repo.git")
    'ssh://example.com/my/repo.git'
    >>> determine_git_uri(False, "ssh://example.com/my/repo.git") is None
    True
    """
    if git_flag is False:
        return None
    elif git_flag is True:
        return source
    else:
        return git.maybegiturl(source)
