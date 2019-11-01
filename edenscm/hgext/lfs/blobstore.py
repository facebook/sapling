# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# blobstore.py - local and remote (speaking Git-LFS protocol) blob storages

from __future__ import absolute_import

import json
import os

from edenscm.mercurial import (
    blobstore,
    error,
    extensions,
    pathutil,
    perftrace,
    progress,
    url as urlmod,
    util,
    vfs as vfsmod,
    worker,
)
from edenscm.mercurial.i18n import _


class filewithprogress(object):
    """a file-like object that supports __len__ and read.

    Useful to provide progress information for how many bytes are read.
    """

    def __init__(self, fp, callback):
        self._fp = fp
        self._callback = callback  # func(readsize)
        fp.seek(0, os.SEEK_END)
        self._len = fp.tell()
        fp.seek(0)

    def __len__(self):
        return self._len

    def read(self, size):
        if self._fp is None:
            return b""
        data = self._fp.read(size)
        if data:
            if self._callback:
                self._callback(len(data))
        return data

    def seek(self, offset):
        if self._fp is not None:
            self._fp.seek(offset)

    def __del__(self):
        self._fp.close()
        self._fp = None


class local(blobstore.localblobstore):
    """Local blobstore for large file contents.

    This blobstore is used both as a cache and as a staging area for large blobs
    to be uploaded to the remote blobstore.
    """

    def __init__(self, repo):
        fullpath = repo.svfs.join("lfs/objects")
        vfs = vfsmod.blobvfs(fullpath)
        cachevfs = None
        usercachepath = repo.ui.config("lfs", "usercache")
        if usercachepath:
            cachevfs = vfsmod.blobvfs(usercachepath)
        super(local, self).__init__(vfs, cachevfs)


class _gitlfsremote(object):
    def __init__(self, ui, url):
        self.ui = ui
        self._metrics = {
            "lfs_download_size": 0,
            "lfs_download_time": 0,
            "lfs_upload_size": 0,
            "lfs_upload_time": 0,
        }
        self._timestamp = {"latest_download_timestamp": 0, "latest_upload_timestamp": 0}
        baseurl, authinfo = url.authinfo()
        self.baseurl = baseurl.rstrip("/")
        useragent = ui.config("experimental", "lfs.user-agent")
        if not useragent:
            useragent = "mercurial/%s git/2.15.1" % util.version()
        try:
            self.urlopener = urlmod.opener(ui, authinfo, useragent)
        except TypeError:
            # System hg can have old version of opener() which does not require
            # user agent.
            self.urlopener = urlmod.opener(ui, authinfo)

        self.retry = ui.configint("lfs", "retry")

    def writebatch(self, pointers, fromstore):
        """Batch upload from local to remote blobstore."""
        self._batch(pointers, fromstore, "upload")

    def readbatch(self, pointers, tostore, objectnames=None):
        """Batch download from remote to local blobstore."""
        self._batch(pointers, tostore, "download", objectnames=objectnames)

    def _batchrequest(self, pointers, action):
        """Get metadata about objects pointed by pointers for given action

        Return decoded JSON object like {'objects': [{'oid': '', 'size': 1}]}
        See https://github.com/git-lfs/git-lfs/blob/master/docs/api/batch.md
        """
        self.ui.log("lfs_url", lfs_url=self.baseurl)
        objects = [{"oid": p.oid(), "size": p.size()} for p in pointers]
        requestdata = json.dumps({"objects": objects, "operation": action})
        batchreq = util.urlreq.request(
            "%s/objects/batch" % self.baseurl, data=requestdata
        )
        batchreq.add_header("Accept", "application/vnd.git-lfs+json")
        batchreq.add_header("Content-Type", "application/vnd.git-lfs+json")
        self._addextraheaders(batchreq)
        try:
            res = self.urlopener.open(batchreq)
            server = res.info().get("server")
            self.ui.log("lfs_server", lfs_server=server)
            rawjson = res.read()
        except util.urlerr.httperror as ex:
            raise LfsRemoteError(_("LFS HTTP error: %s (action=%s)") % (ex, action))
        try:
            response = json.loads(rawjson)
        except ValueError:
            raise LfsRemoteError(_("LFS server returns invalid JSON: %s") % rawjson)
        return response

    def _checkforservererror(self, pointers, responses, action):
        """Scans errors from objects

        Returns LfsRemoteError if any object has an error
        """
        for response in responses:
            # The server should return 404 when objects cannot be found. Some
            # server implementation (ex. lfs-test-server)  does not set "error"
            # but just removes "download" from "actions". Treat that case
            # as the same as 404 error.
            notfound = response.get("error", {}).get("code") == 404 or (
                action == "download" and action not in response.get("actions", [])
            )
            if notfound:
                ptrmap = {p.oid(): p for p in pointers}
                p = ptrmap.get(response["oid"], None)
                filename = getattr(p, "filename", "unknown")
                raise LfsRemoteError(
                    _(("LFS server error. Remote object " "for file %s not found: %r"))
                    % (filename, response)
                )
            if "error" in response:
                raise LfsRemoteError(_("LFS server error: %r") % response)

    def _extractobjects(self, response, pointers, action):
        """extract objects from response of the batch API

        response: parsed JSON object returned by batch API
        return response['objects'] filtered by action
        raise if any object has an error
        """
        # Scan errors from objects - fail early
        objects = response.get("objects", [])
        self._checkforservererror(pointers, objects, action)

        # Filter objects with given action. Practically, this skips uploading
        # objects which exist in the server.
        filteredobjects = [o for o in objects if action in o.get("actions", [])]

        return filteredobjects

    def _basictransfer(self, obj, action, localstore):
        """Download or upload a single object using basic transfer protocol

        obj: dict, an object description returned by batch API
        action: string, one of ['upload', 'download']
        localstore: blobstore.local

        See https://github.com/git-lfs/git-lfs/blob/master/docs/api/\
        basic-transfers.md
        """
        oid = str(obj["oid"])

        href = str(obj["actions"][action].get("href"))
        headers = obj["actions"][action].get("header", {}).items()

        request = util.urlreq.request(href)
        self._addextraheaders(request)
        if action == "upload":
            # If uploading blobs, read data from local blobstore.
            request.data = filewithprogress(localstore.vfs(oid), None)
            request.get_method = lambda: "PUT"

        for k, v in headers:
            request.add_header(k, v)

        response = b""
        try:
            req = self.urlopener.open(request)
            while True:
                data = req.read(1048576)
                if not data:
                    break
                response += data
        except util.urlerr.httperror as ex:
            raise LfsRemoteError(
                _("HTTP error: %s (oid=%s, action=%s)") % (ex, oid, action)
            )

        if action == "download":
            # If downloading blobs, store downloaded data to local blobstore
            localstore.write(oid, response)

    def _batch(self, pointers, localstore, action, objectnames=None):
        if action not in ["upload", "download"]:
            raise error.ProgrammingError("invalid Git-LFS action: %s" % action)

        response = self._batchrequest(pointers, action)
        objects = self._extractobjects(response, pointers, action)
        total = sum(x.get("size", 0) for x in objects)
        perftrace.tracebytes("Size", total)
        sizes = {}
        for obj in objects:
            sizes[obj.get("oid")] = obj.get("size", 0)
        topic = {"upload": _("lfs uploading"), "download": _("lfs downloading")}[action]
        if self.ui.verbose and len(objects) > 1:
            self.ui.write(
                _("lfs: need to transfer %d objects (%s)\n")
                % (len(objects), util.bytecount(total))
            )

        def transfer(chunk):
            for obj in chunk:
                objsize = obj.get("size", 0)
                if self.ui.verbose:
                    if action == "download":
                        msg = _("lfs: downloading %s (%s)\n")
                    elif action == "upload":
                        msg = _("lfs: uploading %s (%s)\n")
                    self.ui.write(msg % (obj.get("oid"), util.bytecount(objsize)))
                retry = self.retry
                while True:
                    try:
                        yield 0, obj.get("oid")
                        self._basictransfer(obj, action, localstore)
                        yield 1, obj.get("oid")
                        break
                    except Exception as ex:
                        if retry > 0:
                            if self.ui.verbose:
                                self.ui.write(
                                    _("lfs: failed: %r (remaining retry %d)\n")
                                    % (ex, retry)
                                )
                            retry -= 1
                            continue
                        raise

        starttimestamp = util.timer()
        if action == "download":
            oids = worker.worker(
                self.ui,
                0.1,
                transfer,
                (),
                sorted(objects, key=lambda o: o.get("oid")),
                preferthreads=True,
                callsite="blobstore",
            )
        else:
            oids = transfer(objects)

        transferred = 0
        with progress.bar(
            self.ui, topic, _("bytes"), total=total, formatfunc=util.bytecount
        ) as prog:
            for count, oid in oids:
                if count != 0:
                    transferred += sizes[oid]
                    if self.ui.verbose:
                        self.ui.write(_("lfs: processed: %s\n") % oid)
                if objectnames is not None:
                    prog.value = (transferred, objectnames.get(oid, ""))
                else:
                    prog.value = transferred

        currenttimestamp = util.timer()
        self._metrics["lfs_%s_size" % action] += total
        self._metrics["lfs_%s_time" % action] += (
            currenttimestamp
            - max(self._timestamp["latest_%s_timestamp" % action], starttimestamp)
        ) * 1000
        self._timestamp["latest_%s_timestamp" % action] = currenttimestamp

    def checkblobs(self, pointers):
        response = self._batchrequest(pointers, "download")
        self._extractobjects(response, pointers, "download")

    def getlfsmetrics(self):
        return self._metrics

    def _addextraheaders(self, req):
        headers = {}

        try:
            clienttelemetry = extensions.find("clienttelemetry")
            headers["X-Client-Correlator"] = clienttelemetry.correlator(self.ui)
        except KeyError:
            pass

        for k, v in headers.items():
            req.add_header(k, v)

    def __del__(self):
        # copied from mercurial/httppeer.py
        urlopener = getattr(self, "urlopener", None)
        if urlopener:
            for h in urlopener.handlers:
                h.close()
                getattr(h, "close_all", lambda: None)()


class _dummyremote(object):
    """Dummy store storing blobs to temp directory."""

    def __init__(self, ui, url):
        self.vfs = vfsmod.blobvfs(url.path)

    def writebatch(self, pointers, fromstore):
        for p in pointers:
            content = fromstore.read(p.oid())
            with self.vfs(p.oid(), "wb", atomictemp=True) as fp:
                fp.write(content)

    def readbatch(self, pointers, tostore, objectnames=None):
        for p in pointers:
            content = self.vfs.read(p.oid())
            tostore.write(p.oid(), content)

    def checkblobs(self, pointers):
        for p in pointers:
            if not self.vfs.exists(p.oid()):
                raise LfsRemoteError(
                    _("LFS object %s is not uploaded to remote server") % p.oid()
                )


class _nullremote(object):
    """Null store storing blobs to /dev/null."""

    def __init__(self, ui, url):
        pass

    def writebatch(self, pointers, fromstore):
        pass

    def readbatch(self, pointers, tostore, objectnames=None):
        pass


class _promptremote(object):
    """Prompt user to set lfs.url when accessed."""

    def __init__(self, ui, url):
        pass

    def writebatch(self, pointers, fromstore, ui=None):
        self._prompt()

    def readbatch(self, pointers, tostore, ui=None, objectnames=None):
        self._prompt()

    def _prompt(self):
        raise error.Abort(_("lfs.url needs to be configured"))


_storemap = {
    "https": _gitlfsremote,
    "http": _gitlfsremote,
    "file": _dummyremote,
    "null": _nullremote,
    None: _promptremote,
}


def remote(ui):
    """remotestore factory. return a store in _storemap depending on config"""
    url = util.url(ui.config("lfs", "url"))
    scheme = url.scheme
    if scheme not in _storemap:
        raise error.Abort(_("lfs: unknown url scheme: %s") % scheme)
    return _storemap[scheme](ui, url)


class LfsRemoteError(error.RevlogError):
    pass
