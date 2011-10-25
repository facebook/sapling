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
    '''localstore first attempts to grab files out of the store in the remote
    Mercurial repository.  Failling that, it attempts to grab the files from
    the user cache.'''

    def __init__(self, ui, repo, remote):
        url = os.path.join(remote.path, '.hg', lfutil.longname)
        super(localstore, self).__init__(ui, repo, util.expandpath(url))
        self.remote = remote

    def put(self, source, hash):
        util.makedirs(os.path.dirname(lfutil.storepath(self.remote, hash)))
        if lfutil.instore(self.remote, hash):
            return
        lfutil.link(lfutil.storepath(self.repo, hash),
                lfutil.storepath(self.remote, hash))

    def exists(self, hash):
        return lfutil.instore(self.remote, hash)

    def _getfile(self, tmpfile, filename, hash):
        if lfutil.instore(self.remote, hash):
            path = lfutil.storepath(self.remote, hash)
        elif lfutil.inusercache(self.ui, hash):
            path = lfutil.usercachepath(self.ui, hash)
        else:
            raise basestore.StoreError(filename, hash, '',
                _("Can't get file locally"))
        fd = open(path, 'rb')
        try:
            return lfutil.copyandhash(fd, tmpfile)
        finally:
            fd.close()

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
        if not lfutil.instore(self.remote, expecthash):
            self.ui.warn(
                _('changeset %s: %s missing\n'
                  '  (looked for hash %s)\n')
                % (cset, filename, expecthash))
            return True                 # failed

        if contents:
            storepath = lfutil.storepath(self.remote, expecthash)
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
