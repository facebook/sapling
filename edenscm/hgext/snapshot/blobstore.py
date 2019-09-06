# blobstore.py - local blob storage for snapshot metadata
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import blobstore, vfs as vfsmod


class local(blobstore.localblobstore):
    """Local blobstore for snapshot metadata contents.
    """

    def __init__(self, repo):
        fullpath = repo.svfs.join("snapshots/objects")
        vfs = vfsmod.blobvfs(fullpath)
        cachevfs = None
        usercachepath = repo.ui.config("snapshot", "usercache")
        if usercachepath:
            self.cachevfs = vfsmod.blobvfs(usercachepath)
        super(local, self).__init__(vfs, cachevfs)
