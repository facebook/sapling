# coding=UTF-8

from __future__ import absolute_import

from mercurial import (
    error,
)
from mercurial.i18n import _

from . import (
    blobstore,
)

def threshold(ui, repo):
    """Configure threshold for a file to be handled by LFS"""
    threshold = ui.configbytes('lfs', 'threshold', None)
    repo.svfs.options['lfsthreshold'] = threshold

def localblobstore(ui, repo):
    """Configure local blobstore"""
    storepath = ui.config('lfs', 'blobstore', 'cache/localblobstore')
    localblobstore = blobstore.local(repo.vfs.join(storepath))
    repo.svfs.lfslocalblobstore = localblobstore

def chunking(ui, repo):
    """Configure chunking for massive blobs to be split into smaller chunks."""
    chunksize = ui.configbytes('lfs', 'chunksize', None)
    repo.svfs.options['lfschunksize'] =  chunksize

def remoteblobstore(ui, repo):
    """Configure remote blobstore."""
    knownblobstores = {
        'git-lfs': blobstore.remote,
        'dummy': blobstore.dummy,
    }
    remotestore = ui.config('lfs', 'remotestore', 'git-lfs')
    if not remotestore in knownblobstores:
        message = _("Unknown remote store %s") % (remotestore)
        raise error.ProgrammingError(message)
    repo.svfs.lfsremoteblobstore = knownblobstores[remotestore](ui)
