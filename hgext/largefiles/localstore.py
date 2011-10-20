# Copyright 2009-2010 Gregory P. Ward
# Copyright 2009-2010 Intelerad Medical Systems Incorporated
# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''store class for local filesystem'''

import os

from mercurial import util
from mercurial.i18n import _

import lfutil
import basestore

class localstore(basestore.basestore):
    '''Because there is a system-wide cache, the local store always
    uses that cache. Since the cache is updated elsewhere, we can
    just read from it here as if it were the store.'''

    def __init__(self, ui, repo, remote):
        url = os.path.join(remote.path, '.hg', lfutil.longname)
        super(localstore, self).__init__(ui, repo, util.expandpath(url))

    def put(self, source, filename, hash):
        '''Any file that is put must already be in the system-wide
        cache so do nothing.'''
        return

    def exists(self, hash):
        return lfutil.inusercache(self.repo.ui, hash)

    def _getfile(self, tmpfile, filename, hash):
        if lfutil.inusercache(self.ui, hash):
            return lfutil.usercachepath(self.ui, hash)
        raise basestore.StoreError(filename, hash, '',
            _("Can't get file locally"))

    def _verifyfile(self, cctx, cset, contents, standin, verified):
        filename = lfutil.splitstandin(standin)
        if not filename:
            return False
        fctx = cctx[standin]
        key = (filename, fctx.filenode())
        if key in verified:
            return False

        expecthash = fctx.data()[0:40]
        verified.add(key)
        if not lfutil.inusercache(self.ui, expecthash):
            self.ui.warn(
                _('changeset %s: %s missing\n'
                  '  (looked for hash %s)\n')
                % (cset, filename, expecthash))
            return True                 # failed

        if contents:
            storepath = lfutil.usercachepath(self.ui, expecthash)
            actualhash = lfutil.hashfile(storepath)
            if actualhash != expecthash:
                self.ui.warn(
                    _('changeset %s: %s: contents differ\n'
                      '  (%s:\n'
                      '  expected hash %s,\n'
                      '  but got %s)\n')
                    % (cset, filename, storepath, expecthash, actualhash))
                return True             # failed
        return False
