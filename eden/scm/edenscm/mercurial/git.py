# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for git support
"""

import bindings

GIT_DIR_FILE = "gitdir"
GIT_REQUIREMENT = "git"


def isgit(repo):
    """Test if repo is backe by git"""
    return GIT_REQUIREMENT in repo.storerequirements


def readgitdir(repo):
    """Return the path of the GIT_DIR, if the repo is backed by git"""
    if isgit(repo):
        return repo.svfs.readutf8(GIT_DIR_FILE)
    else:
        return None


def openstore(repo):
    """Obtain a gitstore object to access git odb"""
    gitdir = readgitdir(repo)
    if gitdir:
        return bindings.gitstore.gitstore(gitdir)
