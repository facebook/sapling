# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
utilities for git support
"""

import hashlib

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


class gitfilelog(object):
    """filelog-like interface for git"""

    def __init__(self, repo):
        self.store = repo.fileslog.contentstore

    def lookup(self, node):
        assert len(node) == 20
        return node

    def read(self, node):
        return self.store.readobj(node, "blob")

    def size(self, node):
        return self.store.readobjsize(node, "blob")

    def rev(self, node):
        # same trick as remotefilelog
        return node

    def cmp(self, node, text):
        """returns True if blob hash is different from text"""
        # compare without reading `node`
        return node != hashobj(b"blob", text)


def hashobj(kind, text):
    """(bytes, bytes) -> bytes. obtain git SHA1 hash"""
    # git blob format: kind + " " + str(size) + "\0" + text
    return hashlib.sha1(b"%s %d\0%s" % (kind, len(text), text)).digest()
