# shareutil.py - useful utility methods for accessing shared repos
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from mercurial import hg


def getsrcrepo(repo):
    """returns main repo in case of shared woking copy
    """
    if repo.sharedpath == repo.path:
        return repo

    # the sharedpath always ends in the .hg; we want the path to the repo
    source = repo.vfs.split(repo.sharedpath)[0]
    srcurl, branches = hg.parseurl(source)
    srcrepo = hg.repository(repo.ui, srcurl)
    if srcrepo.local():
        return srcrepo
    else:
        return repo
