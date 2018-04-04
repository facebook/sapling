# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import hashlib
import json

# Mercurial
from mercurial.i18n import _

from .. import shareutil
from . import (
    commitcloudcommon,
    commitcloudutil,
)

class SyncState(object):
    """
    Stores the local record of what state was stored in the cloud at the
    last sync.
    """

    def __init__(self, repo):
        repo = shareutil.getsrcrepo(repo)
        self.repo = repo

        # get current workspace
        workspace = commitcloudutil.WorkspaceManager(repo).workspace
        if not workspace:
            raise commitcloudcommon.WorkspaceError(
                repo.ui, _('undefined workspace'))

        # make a valid filename
        self.filename = 'commitcloudstate.' + ''.join(
            x for x in workspace if x.isalnum()) + '.%s' % (
                hashlib.sha256(workspace).hexdigest()[0:5])

        if repo.vfs.exists(self.filename):
            with repo.vfs.open(self.filename, 'r') as f:
                data = json.load(f)
                self.version = data['version']
                self.heads = [h.encode() for h in data['heads']]
                self.bookmarks = {n.encode('utf-8'): v.encode()
                                  for n, v in data['bookmarks'].items()}
        else:
            self.version = 0
            self.heads = []
            self.bookmarks = {}

    def update(self, newversion, newheads, newbookmarks):
        data = {
            'version': newversion,
            'heads': newheads,
            'bookmarks': newbookmarks,
        }
        with self.repo.wlock():
            with self.repo.vfs.open(self.filename, 'w', atomictemp=True) as f:
                json.dump(data, f)
        self.version = newversion
        self.heads = newheads
        self.bookmarks = newbookmarks
