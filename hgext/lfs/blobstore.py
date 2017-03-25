from __future__ import absolute_import

import errno
import json
import os
import re

from mercurial import (
    i18n,
    revlog,
    util,
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

    def __init__(self, path, opener):
        self._opener = opener
        self._storepath = path

    @staticmethod
    def get(opener):
        """Get the stored local blobstore instance."""
        if util.safehasattr(opener, 'lfslocalblobstore'):
            return opener.lfslocalblobstore
        raise UnknownBlobstoreError()

    def write(self, storeid, data):
        """Write blob to local blobstore."""
        assert re.match('[a-f0-9]{40}', storeid.oid)
        fp = self._opener(self.filename(storeid), 'w+', atomictemp=True)
        try:
            fp.write(data)
        finally:
            fp.close()

    def read(self, storeid):
        """Read blob from local blobstore."""
        assert re.match('[a-f0-9]{40}', storeid.oid)
        fp = self._opener(self.filename(storeid), 'r')
        try:
            return fp.read()
        finally:
            fp.close()

    def has(self, storeid):
        """Returns True if the local blobstore contains the requested blob,
        False otherwise."""
        return self._opener.exists(self.filename(storeid))

    def filename(self, storeid):
        """Generates filename for a blob in the local blob store. Defaults to
        .hg/cache/blobstore/XX/XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"""
        return os.path.join(self._storepath, storeid.oid[0:2], storeid.oid[2:])

class remote(object):

    def __init__(self, ui):
        url = ui.config('lfs', 'remoteurl', None)
        user = ui.config('lfs', 'remoteuser', None)
        password = ui.config('lfs', 'remotepassword', None)
        assert url is not None
        self.ui=ui
        self.baseurl = url
        if user is not None and password is not None:
            urlreq = util.urlreq
            passwdmanager = urlreq.httppasswordmgrwithdefaultrealm()
            passwdmanager.add_password(None, url, user, password)
            authenticator = urlreq.httpbasicauthhandler(passwdmanager)
            opener = urlreq.buildopener(authenticator)
            urlreq.installopener(opener)

    @staticmethod
    def get(opener):
        """Get the stored remote blobstore instance."""
        if util.safehasattr(opener, 'lfsremoteblobstore'):
            return opener.lfsremoteblobstore
        raise UnknownBlobstoreError()

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
        if self.ui:
            self.ui.write('lfs: mapping blobs to %s URLs\n' % action)
        batchreq = urlreq.request(self.baseurl + 'objects/batch', data=requestdata)
        batchreq.add_header('Accept', 'application/vnd.git-lfs+json')
        batchreq.add_header('Content-Type', 'application/vnd.git-lfs+json')
        raw_response = urlreq.urlopen(batchreq)
        response = json.loads(raw_response.read())

        topic = 'lfs: ' + action + 'ing blobs'
        runningsize = 0
        if total is None:
            alttotal = reduce(lambda acc, x: acc + long(x.get('size', 0)), response.get('objects'), 0)
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

        if self.ui:
            self.ui.progress(topic, pos=None, total=total)
            self.ui.write('lfs: %s completed\n' % action)

class dummy(object):
    """Dummy store storing blobs to temp directory."""

    def __init__(self, ui):
        path = ui.config('lfs', 'remotepath', None)
        if path is None:
            raise Exception('Dummy remotestore: must set "remotepath"')
        try:
            os.makedirs(path)
        except OSError as exc:
            if exc.errno == errno.EEXIST:
                pass
            else:
                raise
        self._storepath = path

    @staticmethod
    def get(opener):
        """Get the stored remote blobstore instance."""
        if util.safehasattr(opener, 'lfsremoteblobstore'):
            return opener.lfsremoteblobstore
        raise UnknownBlobstoreError()

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

class UnknownBlobstoreError(revlog.RevlogError):
    def __init__(self):
        message = 'attempt to access unknown blobstore'
        revlog.RevlogError.__init__(self, i18n._(message))

class RequestFailedError(revlog.RevlogError):
    def __init__(self, oid, action):
        message = 'the requested file could be %sed: %s' % (action, oid)
        revlog.RevlogError.__init__(self, i18n._(message))

class UnavailableBatchOperationError(revlog.RevlogError):
    def __init__(self, oid, action):
        self.oid = oid
        self.action = action

        message = 'unknown batch operation "%s"' % self.action
        if self.oid:
            message += ' for blob "%s"' % self.oid
        revlog.RevlogError.__init__(self, i18n._(message))

