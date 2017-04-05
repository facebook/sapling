from __future__ import absolute_import

import errno
import functools
import json
import os

from mercurial import (
    error,
    util,
)
from mercurial.i18n import _

from . import (
    util as lfsutil,
)

class StoreID(object):
    def __init__(self, oid, size):
        self.oid = oid
        self.size = size

class local(object):
    """Local blobstore for large file contents.

    This blobstore is used both as a cache and as a staging area for large blobs
    to be uploaded to the remote blobstore.
    """

    def __init__(self, repo):
        storepath = repo.ui.config('lfs', 'blobstore', 'cache/localblobstore')
        fullpath = repo.vfs.join(storepath)
        self.vfs = lfsutil.lfsvfs(fullpath)

    def write(self, storeid, data):
        """Write blob to local blobstore."""
        with self.vfs(storeid.oid, 'wb', atomictemp=True) as fp:
            fp.write(data)

    def read(self, storeid):
        """Read blob from local blobstore."""
        return self.vfs.read(storeid.oid)

    def has(self, storeid):
        """Returns True if the local blobstore contains the requested blob,
        False otherwise."""
        return self.vfs.exists(storeid.oid)

class _gitlfsremote(object):

    def __init__(self, repo):
        ui = repo.ui
        url = ui.config('lfs', 'remoteurl', None)
        user = ui.config('lfs', 'remoteuser', None)
        password = ui.config('lfs', 'remotepassword', None)
        assert url is not None
        self.ui = ui
        self.baseurl = url
        if user is not None and password is not None:
            urlreq = util.urlreq
            passwdmanager = urlreq.httppasswordmgrwithdefaultrealm()
            passwdmanager.add_password(None, url, user, password)
            authenticator = urlreq.httpbasicauthhandler(passwdmanager)
            opener = urlreq.buildopener(authenticator)
            urlreq.installopener(opener)

    def writebatch(self, storeids, fromstore, total=None):
        """Batch upload from local to remote blobstore."""
        self._batch(storeids, fromstore, 'upload', total=total)

    def readbatch(self, storeids, tostore, total=None):
        """Batch download from remote to local blostore."""
        self._batch(storeids, tostore, 'download', total=total)

    def _batch(self, storeids, localstore, action, total=None):
        if action not in ['upload', 'download']:
            # FIXME: we should not have that error raise too high
            raise UnavailableBatchOperationError(None, action)

        # Create the batch data for git-lfs.
        urlreq = util.urlreq
        objects = []
        storeidmap = {}
        for storeid in storeids:
            oid = storeid.oid[:40]  # Limitation in Dewey, hashes max 40 char
            size = storeid.size
            objects.append({
                'oid': oid,
                'size': size,
            })
            storeidmap[oid] = storeid

        requestdata = json.dumps({
            'objects': objects,
            'operation': action,
        })

        # Batch upload the blobs to git-lfs.
        if self.ui.verbose:
            self.ui.write(_('lfs: mapping blobs to %s URLs\n') % action)
        batchreq = urlreq.request(self.baseurl + 'objects/batch',
                                  data=requestdata)
        batchreq.add_header('Accept', 'application/vnd.git-lfs+json')
        batchreq.add_header('Content-Type', 'application/vnd.git-lfs+json')
        raw_response = urlreq.urlopen(batchreq)
        response = json.loads(raw_response.read())

        topic = {'upload': _('lfs uploading'),
                 'download': _('lfs downloading')}[action]
        runningsize = 0
        if total is None:
            alttotal = functools.reduce(
                lambda acc, x: acc + long(x.get('size', 0)),
                response.get('objects'), 0)
            if alttotal > 0:
                total = alttotal
        if self.ui:
            self.ui.progress(topic, 0, total=total)
        for obj in response.get('objects'):
            oid = str(obj['oid'])
            try:
                # The action we're trying to perform should be available for the
                # current blob.
                if action not in obj.get('actions'):
                    raise UnavailableBatchOperationError(oid, action)

                size = long(obj.get('size'))
                href = str(obj['actions'][action].get('href'))
                headers = obj['actions'][action].get('header', {}).items()

                if self.ui:
                    self.ui.progress(topic, runningsize, total=total)

                if action == 'upload':
                    # If uploading blobs, read data from local blobstore.
                    filedata = localstore.read(storeidmap[oid])
                    request = urlreq.request(href, data=filedata)
                    request.get_method = lambda: 'PUT'
                else:
                    request = urlreq.request(href)

                for k, v in headers:
                    request.add_header(k, v)

                response = urlreq.urlopen(request)

                if action == 'download':
                    # If downloading blobs, store downloaded data to local
                    # blobstore
                    localstore.write(storeidmap[oid], response.read())

                runningsize += size
            except util.urlerr.httperror:
                raise RequestFailedError(oid, action)
            except UnavailableBatchOperationError:
                if action == 'upload':
                    # The blob is already known by the remote blobstore.
                    continue
                else:
                    raise RequestFailedError(oid, action)

        self.ui.progress(topic, pos=None, total=total)
        if self.ui.verbose:
            self.ui.write(_('lfs: %s completed\n') % action)

class _dummyremote(object):
    """Dummy store storing blobs to temp directory."""

    def __init__(self, repo):
        path = repo.ui.config('lfs', 'remotepath', None)
        if path is None:
            raise error.ProgrammingError('dummystore: must set "remotepath"')
        try:
            os.makedirs(path)
        except OSError as exc:
            if exc.errno == errno.EEXIST:
                pass
            else:
                raise
        self._storepath = path

    def write(self, storeid, data):
        fname = self.filename(storeid)
        try:
            os.makedirs(os.path.dirname(fname))
        except OSError as exc:
            if exc.errno == errno.EEXIST:
                pass
            else:
                raise
        with open(self.filename(storeid), 'w+') as fp:
            fp.write(data)

    def read(self, storeid):
        with open(self.filename(storeid), 'r+') as fp:
            return fp.read()

    def writebatch(self, storeids, fromstore, ui=None, total=None):
        for id in storeids:
            content = fromstore.read(id)
            self.write(id, content)

    def readbatch(self, storeids, tostore, ui=None, total=None):
        for id in storeids:
            content = self.read(id)
            tostore.write(id, content)

    def filename(self, storeid):
        filename = os.path.join(self._storepath, storeid.oid)
        return filename

_storemap = {
    'git-lfs': _gitlfsremote,
    'dummy': _dummyremote,
}

def remote(repo):
    """remotestore factory. return a store in _storemap depending on config"""
    storename = repo.ui.config('lfs', 'remotestore', 'git-lfs')
    if storename not in _storemap:
        raise error.Abort(_('lfs: unknown remotestore: %s') % storename)
    return _storemap[storename](repo)

class RequestFailedError(error.RevlogError):
    def __init__(self, oid, action):
        message = _('the requested file could be %sed: %s') % (action, oid)
        super(RequestFailedError, self).__init__(message)

class UnavailableBatchOperationError(error.RevlogError):
    def __init__(self, oid, action):
        self.oid = oid
        self.action = action

        message = (_('unknown batch operation "%s" for blob "%s"')
                   % (self.action, self.oid or 'unknown'))
        super(UnavailableBatchOperationError, self).__init__(message)
