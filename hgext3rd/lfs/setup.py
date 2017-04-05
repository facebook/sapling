# coding=UTF-8

from __future__ import absolute_import

from . import (
    blobstore,
)

def threshold(ui, repo):
    """Configure threshold for a file to be handled by LFS"""
    threshold = ui.configbytes('lfs', 'threshold', None)
    repo.svfs.options['lfsthreshold'] = threshold

def localblobstore(ui, repo):
    """Configure local blobstore"""
    repo.svfs.lfslocalblobstore = blobstore.local(repo)

def chunking(ui, repo):
    """Configure chunking for massive blobs to be split into smaller chunks."""
    chunksize = ui.configbytes('lfs', 'chunksize', None)
    repo.svfs.options['lfschunksize'] =  chunksize

def remoteblobstore(ui, repo):
    """Configure remote blobstore."""
    repo.svfs.lfsremoteblobstore = blobstore.remote(repo)
