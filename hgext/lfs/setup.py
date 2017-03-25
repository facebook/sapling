# coding=UTF-8

from __future__ import absolute_import

from mercurial import (
    error,
    i18n,
)

from . import (
    blobstore,
)

def threshold(ui, repo):
    """Configure threshold for a file to be handled by LFS"""
    threshold = ui.configbytes('lfs', 'threshold', None)
    if threshold:
        repo.lfsthreshold = threshold
        repo.svfs.options['lfsthreshold'] = threshold

def localblobstore(ui, repo):
    """Configure local blobstore"""
    storepath = ui.config('lfs', 'blobstore', 'cache/localblobstore')
    localblobstore = blobstore.local(storepath, repo.opener)
    repo.lfslocalblobstore = localblobstore
    repo.svfs.lfslocalblobstore = localblobstore

def chunking(ui, repo):
    """Configure chunking for massive blobs to be split into smaller chunks."""
    chunksize = ui.configbytes('lfs', 'chunksize', None)
    if chunksize:
        repo.lfschunksize = chunksize
        repo.svfs.options['lfschunksize'] =  chunksize

def remoteblobstore(ui, repo):
    """Configure remote blobstore."""
    knownblobstores = {
        'git-lfs': blobstore.remote,
        'dummy': blobstore.dummy,
    }
    remotestore = ui.config('lfs', 'remotestore', 'git-lfs')
    if not remotestore in knownblobstores:
        message = i18n._("Unknown remote store %s") % (remotestore)
        raise error.ProgrammingError(message)
    repo.lfsremoteblobstore = knownblobstores[remotestore](ui)
    repo.svfs.lfsremoteblobstore = repo.lfsremoteblobstore


