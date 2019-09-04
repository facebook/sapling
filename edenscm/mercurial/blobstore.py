# blobstore.py - local blob storage
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib

from . import error
from .i18n import _


class localblobstore(object):
    """A local blobstore.

    This blobstore is used both as a cache and as a staging area for large blobs
    to be uploaded to the remote blobstore.
    """

    def __init__(self, vfs, cachevfs):
        self.vfs = vfs
        self.cachevfs = cachevfs

    def write(self, oid, data):
        """Write blob to local blobstore."""
        contentsha256 = hashlib.sha256(data).hexdigest()
        if contentsha256 != oid:
            raise error.Abort(
                _("blobstore: sha256 mismatch (oid: %s, content: %s)")
                % (oid, contentsha256)
            )
        with self.vfs(oid, "wb", atomictemp=True) as fp:
            fp.write(data)

        # XXX: should we verify the content of the cache, and hardlink back to
        # the local store on success, but truncate, write and link on failure?
        if self.cachevfs and not self.cachevfs.exists(oid):
            self.vfs.linktovfs(oid, self.cachevfs)

    def read(self, oid):
        """Read blob from local blobstore."""
        if self.cachevfs and not self.vfs.exists(oid):
            self.cachevfs.linktovfs(oid, self.vfs)
        return self.vfs.read(oid)

    def has(self, oid):
        """Returns True if the local blobstore contains the requested blob,
        False otherwise."""
        return (self.cachevfs and self.cachevfs.exists(oid)) or self.vfs.exists(oid)
