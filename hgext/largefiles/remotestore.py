# Copyright 2010-2011 Fog Creek Software
# Copyright 2010-2011 Unity Technologies
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''Remote largefile store; the base class for servestore'''

import urllib2
import HTTPError

from mercurial import util
from mercurial.i18n import _

import lfutil
import basestore

class remotestore(basestore.basestore):
    """A largefile store accessed over a network"""
    def __init__(self, ui, repo, url):
        super(remotestore, self).__init__(ui, repo, url)

    def put(self, source, hash):
        if self._verify(hash):
            return
        if self.sendfile(source, hash):
            raise util.Abort(
                _('remotestore: could not put %s to remote store %s')
                % (source, self.url))
        self.ui.debug(
            _('remotestore: put %s to remote store %s') % (source, self.url))

    def exists(self, hash):
        return self._verify(hash)

    def sendfile(self, filename, hash):
        self.ui.debug('remotestore: sendfile(%s, %s)\n' % (filename, hash))
        fd = None
        try:
            try:
                fd = lfutil.httpsendfile(self.ui, filename)
            except IOError, e:
                raise util.Abort(
                    _('remotestore: could not open file %s: %s')
                    % (filename, str(e)))
            return self._put(hash, fd)
        finally:
            if fd:
                fd.close()

    def _getfile(self, tmpfile, filename, hash):
        # quit if the largefile isn't there
        stat = self._stat(hash)
        if stat:
            raise util.Abort(_('remotestore: largefile %s is %s') %
                             (hash, stat == 1 and 'invalid' or 'missing'))

        try:
            length, infile = self._get(hash)
        except HTTPError, e:
            # 401s get converted to util.Aborts; everything else is fine being
            # turned into a StoreError
            raise basestore.StoreError(filename, hash, self.url, str(e))
        except urllib2.URLError, e:
            # This usually indicates a connection problem, so don't
            # keep trying with the other files... they will probably
            # all fail too.
            raise util.Abort('%s: %s' % (self.url, str(e.reason)))
        except IOError, e:
            raise basestore.StoreError(filename, hash, self.url, str(e))

        # Mercurial does not close its SSH connections after writing a stream
        if length is not None:
            infile = lfutil.limitreader(infile, length)
        return lfutil.copyandhash(lfutil.blockstream(infile), tmpfile)

    def _verify(self, hash):
        return not self._stat(hash)

    def _verifyfile(self, cctx, cset, contents, standin, verified):
        filename = lfutil.splitstandin(standin)
        if not filename:
            return False
        fctx = cctx[standin]
        key = (filename, fctx.filenode())
        if key in verified:
            return False

        verified.add(key)

        stat = self._stat(hash)
        if not stat:
            return False
        elif stat == 1:
            self.ui.warn(
                _('changeset %s: %s: contents differ\n')
                % (cset, filename))
            return True # failed
        elif stat == 2:
            self.ui.warn(
                _('changeset %s: %s missing\n')
                % (cset, filename))
            return True # failed
        else:
            raise util.Abort(_('check failed, unexpected response'
                               'statlfile: %d') % stat)
