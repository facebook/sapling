from __future__ import absolute_import

import json

from mercurial import (
    error,
    url as urlmod,
    util,
)
from mercurial.i18n import _

from . import (
    util as lfsutil,
)

class local(object):
    """Local blobstore for large file contents.

    This blobstore is used both as a cache and as a staging area for large blobs
    to be uploaded to the remote blobstore.
    """

    def __init__(self, repo):
        fullpath = repo.svfs.join('lfs/objects')
        self.vfs = lfsutil.lfsvfs(fullpath)

    def write(self, oid, data):
        """Write blob to local blobstore."""
        with self.vfs(oid, 'wb', atomictemp=True) as fp:
            fp.write(data)

    def read(self, oid):
        """Read blob from local blobstore."""
        return self.vfs.read(oid)

    def has(self, oid):
        """Returns True if the local blobstore contains the requested blob,
        False otherwise."""
        return self.vfs.exists(oid)

class _gitlfsremote(object):

    def __init__(self, repo, url):
        ui = repo.ui
        self.ui = ui
        baseurl, authinfo = url.authinfo()
        self.baseurl = baseurl.rstrip('/')
        self.urlopener = urlmod.opener(ui, authinfo)

    def writebatch(self, pointers, fromstore):
        """Batch upload from local to remote blobstore."""
        self._batch(pointers, fromstore, 'upload')

    def readbatch(self, pointers, tostore):
        """Batch download from remote to local blostore."""
        self._batch(pointers, tostore, 'download')

    def _batchrequest(self, pointers, action):
        """Get metadata about objects pointed by pointers for given action

        Return decoded JSON object like {'objects': [{'oid': '', 'size': 1}]}
        See https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md
        """
        objects = [{'oid': p.oid(), 'size': p.size()} for p in pointers]
        requestdata = json.dumps({
            'objects': objects,
            'operation': action,
        })
        batchreq = util.urlreq.request('%s/objects/batch' % self.baseurl,
                                       data=requestdata)
        batchreq.add_header('Accept', 'application/vnd.git-lfs+json')
        batchreq.add_header('Content-Type', 'application/vnd.git-lfs+json')
        try:
            rawjson = self.urlopener.open(batchreq).read()
        except util.urlerr.httperror as ex:
            raise LfsRemoteError(_('LFS HTTP error: %s (action=%s)')
                                 % (ex, action))
        try:
            response = json.loads(rawjson)
        except ValueError:
            raise LfsRemoteError(_('LFS server returns invalid JSON: %s')
                                 % rawjson)
        return response

    def _basictransfer(self, obj, action, localstore):
        """Download or upload a single object using basic transfer protocol

        obj: dict, an object description returned by batch API
        action: string, one of ['upload', 'download']
        localstore: blobstore.local

        See https://github.com/git-lfs/git-lfs/blob/master/docs/api/\
        basic-transfers.md
        """
        oid = str(obj['oid'])

        # The action we're trying to perform should be available for the
        # current blob. If upload is unavailable, it means the server
        # has the object already, which is not an error.
        if action not in obj.get('actions', []):
            if action == 'upload':
                return
            m = obj.get('error', {}).get(
                'message', _('(server did not provide error message)'))
            raise LfsRemoteError(_('cannot download LFS object %s: %s')
                                 % (oid, m))

        href = str(obj['actions'][action].get('href'))
        headers = obj['actions'][action].get('header', {}).items()

        request = util.urlreq.request(href)
        if action == 'upload':
            # If uploading blobs, read data from local blobstore.
            request.data = localstore.read(oid)
            request.get_method = lambda: 'PUT'

        for k, v in headers:
            request.add_header(k, v)

        try:
            response = self.urlopener.open(request).read()
        except util.urlerr.httperror as ex:
            raise LfsRemoteError(_('HTTP error: %s (oid=%s, action=%s)')
                                 % (ex, oid, action))

        if action == 'download':
            # If downloading blobs, store downloaded data to local blobstore
            localstore.write(oid, response)

    def _batch(self, pointers, localstore, action):
        if action not in ['upload', 'download']:
            raise error.ProgrammingError('invalid Git-LFS action: %s' % action)

        response = self._batchrequest(pointers, action)
        runningsize = 0
        objects = response.get('objects', [])
        total = sum(x.get('size', 0) for x in objects)
        topic = {'upload': _('lfs uploading'),
                 'download': _('lfs downloading')}[action]
        self.ui.progress(topic, 0, total=total)
        for obj in objects:
            self._basictransfer(obj, action, localstore)
            runningsize += obj.get('size', 0)
            self.ui.progress(topic, runningsize, total=total)

        self.ui.progress(topic, pos=None, total=total)
        if self.ui.verbose:
            self.ui.write(_('lfs: %s completed\n') % action)

    def __del__(self):
        # copied from mercurial/httppeer.py
        urlopener = getattr(self, 'urlopener', None)
        if urlopener:
            for h in urlopener.handlers:
                h.close()
                getattr(h, "close_all", lambda : None)()

class _dummyremote(object):
    """Dummy store storing blobs to temp directory."""

    def __init__(self, repo, url):
        fullpath = repo.vfs.join('lfs', url.path)
        self.vfs = lfsutil.lfsvfs(fullpath)

    def writebatch(self, pointers, fromstore):
        for p in pointers:
            content = fromstore.read(p.oid())
            with self.vfs(p.oid(), 'wb', atomictemp=True) as fp:
                fp.write(content)

    def readbatch(self, pointers, tostore):
        for p in pointers:
            content = self.vfs.read(p.oid())
            tostore.write(p.oid(), content)

class _nullremote(object):
    """Null store storing blobs to /dev/null."""

    def __init__(self, repo, url):
        pass

    def writebatch(self, pointers, fromstore):
        pass

    def readbatch(self, pointers, tostore):
        pass

class _promptremote(object):
    """Prompt user to set lfs.url when accessed."""

    def __init__(self, repo, url):
        pass

    def writebatch(self, pointers, fromstore, ui=None):
        self._prompt()

    def readbatch(self, pointers, tostore, ui=None):
        self._prompt()

    def _prompt(self):
        raise error.Abort(_('lfs.url needs to be configured'))

_storemap = {
    'https': _gitlfsremote,
    'http': _gitlfsremote,
    'file': _dummyremote,
    'null': _nullremote,
    None: _promptremote,
}

def remote(repo):
    """remotestore factory. return a store in _storemap depending on config"""
    defaulturl = ''

    # convert deprecated configs to the new url. TODO: remove this if other
    # places are migrated to the new url config.
    # deprecated config: lfs.remotestore
    deprecatedstore = repo.ui.config('lfs', 'remotestore')
    if deprecatedstore == 'dummy':
        # deprecated config: lfs.remotepath
        defaulturl = 'file://' + repo.ui.config('lfs', 'remotepath')
    elif deprecatedstore == 'git-lfs':
        # deprecated config: lfs.remoteurl
        defaulturl = repo.ui.config('lfs', 'remoteurl')
    elif deprecatedstore == 'null':
        defaulturl = 'null://'

    url = util.url(repo.ui.config('lfs', 'url', defaulturl))
    scheme = url.scheme
    if scheme not in _storemap:
        raise error.Abort(_('lfs: unknown url scheme: %s') % scheme)
    return _storemap[scheme](repo, url)

class LfsRemoteError(error.RevlogError):
    pass
