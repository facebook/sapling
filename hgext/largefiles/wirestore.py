# Copyright 2010-2011 Fog Creek Software
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''largefile store working over Mercurial's wire protocol'''

import lfutil
import remotestore

class wirestore(remotestore.remotestore):
    def __init__(self, ui, repo, remote):
        cap = remote.capable('largefiles')
        if not cap:
            raise lfutil.storeprotonotcapable([])
        storetypes = cap.split(',')
        if not 'serve' in storetypes:
            raise lfutil.storeprotonotcapable(storetypes)
        self.remote = remote
        super(wirestore, self).__init__(ui, repo, remote.url())

    def _put(self, hash, fd):
        return self.remote.putlfile(hash, fd)

    def _get(self, hash):
        return self.remote.getlfile(hash)

    def _stat(self, hash):
        return self.remote.statlfile(hash)
