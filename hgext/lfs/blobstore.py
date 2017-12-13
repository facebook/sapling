# blobstore.py - local and remote (speaking Git-LFS protocol) blob storages
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import json
import os
import re

from mercurial.i18n import _

from mercurial import (
    error,
    pathutil,
    url as urlmod,
    util,
    vfs as vfsmod,
)

from ..largefiles import lfutil

# 64 bytes for SHA256
_lfsre = re.compile(r'\A[a-f0-9]{64}\Z')

class lfsvfs(vfsmod.vfs):
    def join(self, path):
        """split the path at first two characters, like: XX/XXXXX..."""
        if not _lfsre.match(path):
            raise error.ProgrammingError('unexpected lfs path: %s' % path)
        return super(lfsvfs, self).join(path[0:2], path[2:])

    def walk(self, path=None, onerror=None):
        """Yield (dirpath, [], oids) tuple for blobs under path

        Oids only exist in the root of this vfs, so dirpath is always ''.
        """
        root = os.path.normpath(self.base)
        # when dirpath == root, dirpath[prefixlen:] becomes empty
        # because len(dirpath) < prefixlen.
        prefixlen = len(pathutil.normasprefix(root))
        oids = []

        for dirpath, dirs, files in os.walk(self.reljoin(self.base, path or ''),
                                            onerror=onerror):
            dirpath = dirpath[prefixlen:]

            # Silently skip unexpected files and directories
            if len(dirpath) == 2:
                oids.extend([dirpath + f for f in files
                             if _lfsre.match(dirpath + f)])

        yield ('', [], oids)

class filewithprogress(object):
    """a file-like object that supports __len__ and read.

    Useful to provide progress information for how many bytes are read.
    """

    def __init__(self, fp, callback):
        self._fp = fp
        self._callback = callback # func(readsize)
        fp.seek(0, os.SEEK_END)
        self._len = fp.tell()
        fp.seek(0)

    def __len__(self):
        return self._len

    def read(self, size):
        if self._fp is None:
            return b''
        data = self._fp.read(size)
        if data:
            if self._callback:
                self._callback(len(data))
        else:
            self._fp.close()
            self._fp = None
        return data

class local(object):
    """Local blobstore for large file contents.

    This blobstore is used both as a cache and as a staging area for large blobs
    to be uploaded to the remote blobstore.
    """

    def __init__(self, repo):
        fullpath = repo.svfs.join('lfs/objects')
        self.vfs = lfsvfs(fullpath)
        usercache = lfutil._usercachedir(repo.ui, 'lfs')
        self.cachevfs = lfsvfs(usercache)

    def write(self, oid, data):
        """Write blob to local blobstore."""
        with self.vfs(oid, 'wb', atomictemp=True) as fp:
            fp.write(data)

        # XXX: should we verify the content of the cache, and hardlink back to
        # the local store on success, but truncate, write and link on failure?
        if not self.cachevfs.exists(oid):
            lfutil.link(self.vfs.join(oid), self.cachevfs.join(oid))

    def read(self, oid):
        """Read blob from local blobstore."""
        if not self.vfs.exists(oid):
            lfutil.link(self.cachevfs.join(oid), self.vfs.join(oid))
        return self.vfs.read(oid)

    def has(self, oid):
        """Returns True if the local blobstore contains the requested blob,
        False otherwise."""
        return self.cachevfs.exists(oid) or self.vfs.exists(oid)

class _gitlfsremote(object):

    def __init__(self, repo, url):
        ui = repo.ui
        self.ui = ui
        baseurl, authinfo = url.authinfo()
        self.baseurl = baseurl.rstrip('/')
        self.urlopener = urlmod.opener(ui, authinfo)
        self.retry = ui.configint('lfs', 'retry')

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

    def _checkforservererror(self, pointers, responses):
        """Scans errors from objects

        Returns LfsRemoteError if any objects has an error"""
        for response in responses:
            error = response.get('error')
            if error:
                ptrmap = {p.oid(): p for p in pointers}
                p = ptrmap.get(response['oid'], None)
                if error['code'] == 404 and p:
                    filename = getattr(p, 'filename', 'unknown')
                    raise LfsRemoteError(
                        _(('LFS server error. Remote object '
                          'for file %s not found: %r')) % (filename, response))
                raise LfsRemoteError(_('LFS server error: %r') % response)

    def _extractobjects(self, response, pointers, action):
        """extract objects from response of the batch API

        response: parsed JSON object returned by batch API
        return response['objects'] filtered by action
        raise if any object has an error
        """
        # Scan errors from objects - fail early
        objects = response.get('objects', [])
        self._checkforservererror(pointers, objects)

        # Filter objects with given action. Practically, this skips uploading
        # objects which exist in the server.
        filteredobjects = [o for o in objects if action in o.get('actions', [])]
        # But for downloading, we want all objects. Therefore missing objects
        # should be considered an error.
        if action == 'download':
            if len(filteredobjects) < len(objects):
                missing = [o.get('oid', '?')
                           for o in objects
                           if action not in o.get('actions', [])]
                raise LfsRemoteError(
                    _('LFS server claims required objects do not exist:\n%s')
                    % '\n'.join(missing))

        return filteredobjects

    def _basictransfer(self, obj, action, localstore, progress=None):
        """Download or upload a single object using basic transfer protocol

        obj: dict, an object description returned by batch API
        action: string, one of ['upload', 'download']
        localstore: blobstore.local

        See https://github.com/git-lfs/git-lfs/blob/master/docs/api/\
        basic-transfers.md
        """
        oid = str(obj['oid'])

        href = str(obj['actions'][action].get('href'))
        headers = obj['actions'][action].get('header', {}).items()

        request = util.urlreq.request(href)
        if action == 'upload':
            # If uploading blobs, read data from local blobstore.
            request.data = filewithprogress(localstore.vfs(oid), progress)
            request.get_method = lambda: 'PUT'

        for k, v in headers:
            request.add_header(k, v)

        response = b''
        try:
            req = self.urlopener.open(request)
            while True:
                data = req.read(1048576)
                if not data:
                    break
                if action == 'download' and progress:
                    progress(len(data))
                response += data
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
        prunningsize = [0]
        objects = self._extractobjects(response, pointers, action)
        total = sum(x.get('size', 0) for x in objects)
        topic = {'upload': _('lfs uploading'),
                 'download': _('lfs downloading')}[action]
        if self.ui.verbose and len(objects) > 1:
            self.ui.write(_('lfs: need to transfer %d objects (%s)\n')
                          % (len(objects), util.bytecount(total)))
        self.ui.progress(topic, 0, total=total)
        def progress(size):
            # advance progress bar by "size" bytes
            prunningsize[0] += size
            self.ui.progress(topic, prunningsize[0], total=total)
        for obj in sorted(objects, key=lambda o: o.get('oid')):
            objsize = obj.get('size', 0)
            if self.ui.verbose:
                if action == 'download':
                    msg = _('lfs: downloading %s (%s)\n')
                elif action == 'upload':
                    msg = _('lfs: uploading %s (%s)\n')
                self.ui.write(msg % (obj.get('oid'), util.bytecount(objsize)))
            origrunningsize = prunningsize[0]
            retry = self.retry
            while True:
                prunningsize[0] = origrunningsize
                try:
                    self._basictransfer(obj, action, localstore,
                                        progress=progress)
                    break
                except Exception as ex:
                    if retry > 0:
                        if self.ui.verbose:
                            self.ui.write(
                                _('lfs: failed: %r (remaining retry %d)\n')
                                % (ex, retry))
                        retry -= 1
                        continue
                    raise

        self.ui.progress(topic, pos=None, total=total)

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
        self.vfs = lfsvfs(fullpath)

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
