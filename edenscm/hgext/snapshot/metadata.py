# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# metadata.py - snapshot metadata

from __future__ import absolute_import

import hashlib

from edenscm.mercurial import error, pathutil
from edenscm.mercurial.i18n import _
from edenscm.mercurial.utils import cborutil


class filewrapper(object):
    """Helper class that links files to oids in the blob storage.
    Also does serialization/deserialization for metadata.
    """

    def __init__(self, path, oid=None, content=None):
        self.path = path
        self.oid = oid
        self.content = content
        self._check()

    def _check(self):
        if self.oid is not None and self.content is not None:
            raise error.Abort("ambiguous file contents: %s" % self.path)

    def todict(self):
        self._check()
        if self.content is not None:
            return {"content": self.content}
        if self.oid is not None:
            return {"oid": self.oid}
        return None

    @classmethod
    def fromdict(cls, path, data):
        return cls(path, oid=data.get("oid"), content=data.get("content"))

    def getcontent(self, store):
        if self.content is not None:
            return self.content
        if self.oid is not None:
            return store.read(self.oid)
        return None


class snapshotmetadata(object):
    """Snapshot metadata.

    Snapshot metadata is the main class of this extension.
    Snapshot metadata consists of
    * missing files (marked with `!` in `hg status` output);
    * unknown files (marked with `?` in `hg status` output);
    * internal files (important for the repo state, located in `.hg` directory).

    The dict representation of metadata looks like this:
    {
        "version": "1",
        "files": {
            "deleted": {
                "path/to/deleted/file": None,
                . . .
            },
            "unknown": {
                "path/to/unknown/file": {
                    "oid": <oid of the blob in the storage>,
                    "size": <size of this blob>
                },
                . . .
            },
            "localvfsfiles": {
                "path/to/aux/file": {
                    "oid": <oid of the blob in the storage>,
                    "size": <size of this blob>
                },
                . . .
            }
        }
    }
    It gets serialized to CBOR for storage/transfer.

    Currently the metadata keeps track of
    * merge state (by preserving the contents of the `.hg/merge` directory);
    * rebase state (by preserving the `.hg/rebasestate` file).
    """

    VERSION = 1

    def __init__(self, deleted=[], unknown=[], localvfsfiles=[]):
        self.deleted = deleted  # missing files
        self.unknown = unknown  # unknown files
        self.localvfsfiles = localvfsfiles  # internal files

    @property
    def empty(self):
        return not (self.deleted or self.unknown or self.localvfsfiles)

    def todict(self):
        files = {}
        files["deleted"] = {d.path: d.todict() for d in self.deleted}
        files["unknown"] = {u.path: u.todict() for u in self.unknown}
        files["localvfsfiles"] = {f.path: f.todict() for f in self.localvfsfiles}
        return {"files": files, "version": str(snapshotmetadata.VERSION)}

    def serialize(self):
        return "".join(cborutil.streamencode(self.todict()))

    @classmethod
    def fromdict(cls, metadatadict):
        # check version of metadata
        try:
            version = int(metadatadict.get("version"))
        except ValueError:
            raise error.Abort(
                "invalid metadata version: %s\n" % (metadatadict.get("version"),)
            )
        if version != snapshotmetadata.VERSION:
            raise error.Abort("invalid version number %d" % (version,))
        try:
            files = metadatadict["files"]
            deleted = [filewrapper(path) for path in sorted(files["deleted"].keys())]
            unknown = [
                filewrapper.fromdict(path, data)
                for path, data in sorted(files["unknown"].items())
            ]
            localvfsfiles = [
                filewrapper.fromdict(path, data)
                for path, data in sorted(files["localvfsfiles"].items())
            ]
            return cls(deleted=deleted, unknown=unknown, localvfsfiles=localvfsfiles)
        except ValueError:
            raise error.Abort("invalid metadata: %s\n" % (metadatadict,))

    @classmethod
    def deserialize(cls, cbor_data):
        try:
            metadatadict = cborutil.decodeall(cbor_data)[0]
        except cborutil.CBORDecodeError:
            raise error.Abort("invalid metadata stream\n")
        return cls.fromdict(metadatadict)

    @classmethod
    def createfromworkingcopy(cls, repo, status=None, include_untracked=True):
        """create a new snapshot from the working copy

        This method gets called from the `hg snapshot create` cmd.
        """
        # populate the metadata
        status = status or repo.status(unknown=include_untracked)
        deleted = [filewrapper(path) for path in status.deleted]
        unknown = [filewrapper(path) for path in status.unknown]
        # check merge and rebase info
        localvfsfiles = []
        ismergestate = len(repo[None].parents()) > 1
        isrebasestate = repo.localvfs.exists("rebasestate")
        if ismergestate or isrebasestate:
            for root, dirs, files in repo.localvfs.walk(path="merge"):
                localvfsfiles += [filewrapper(pathutil.join(root, f)) for f in files]
        if isrebasestate:
            localvfsfiles.append(filewrapper("rebasestate"))
        return cls(deleted=deleted, unknown=unknown, localvfsfiles=localvfsfiles)

    @classmethod
    def getfromlocalstorage(cls, repo, oid):
        """get the existing snapshot from the local storage"""

        def checkfileisstored(store, oid, path):
            if oid is not None and not store.has(oid):
                raise error.Abort(
                    "file %s with oid %s not found in local blobstorage\n" % (path, oid)
                )

        store = repo.svfs.snapshotstore
        checkfileisstored(store, oid, "metadata")
        metadata = cls.deserialize(store.read(oid))
        # validate related files
        for file in metadata.unknown:
            checkfileisstored(store, file.oid, file.path)
        for file in metadata.localvfsfiles:
            checkfileisstored(store, file.oid, file.path)
        return metadata

    def storelocally(self, repo):
        def _dostore(data):
            oid = hashlib.sha256(data).hexdigest()
            store.write(oid, data)
            return oid

        def _storefile(f, data):
            if len(data) > threshold:
                f.oid = _dostore(data)
            else:
                f.content = data

        wctx = repo[None]
        store = repo.svfs.snapshotstore
        threshold = repo.svfs.options["snapshotthreshold"]
        for f in self.unknown:
            _storefile(f, wctx[f.path].data())
        for f in self.localvfsfiles:
            _storefile(f, repo.localvfs.open(path=f.path).read())
        oid = _dostore(self.serialize())
        return oid

    def getauxfilesinfo(self):
        auxfilesinfo = set()
        auxfilesinfo.update(f.oid for f in self.unknown if f.oid)
        auxfilesinfo.update(f.oid for f in self.localvfsfiles if f.oid)
        return auxfilesinfo

    def files(self, showlocalvfs=False):
        filelist = []
        filelist += [("?", f.path) for f in self.unknown]
        filelist += [("!", f.path) for f in self.deleted]
        if showlocalvfs:
            filelist += [("?", f.path) for f in self.localvfsfiles]
        return filelist
