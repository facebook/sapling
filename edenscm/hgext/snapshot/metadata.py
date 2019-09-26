# metadata.py - snapshot metadata
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import hashlib

from edenscm.mercurial import error, pathutil
from edenscm.mercurial.i18n import _
from edenscm.mercurial.utils import cborutil


class filewrapper(object):
    """
    Helper class that links files to oids in the blob storage.
    Also does serialization/deserialization for metadata.
    """

    def __init__(self, path, oid=None, size=None):
        self.path = path
        # TODO(alexeyqu): add possible file content here
        self.oid = oid
        self.size = size

    def todict(self):
        if self.oid is None and self.size is None:
            return None
        return {"oid": self.oid, "size": self.size}

    @classmethod
    def fromdict(cls, path, data):
        return cls(path, oid=data.get("oid"), size=data.get("size"))


class snapshotmetadata(object):
    """
    Main class that contains snapshot metadata representation.
    """

    VERSION = 1

    def __init__(self, deleted=[], unknown=[], localvfsfiles=[]):
        self.deleted = deleted
        self.unknown = unknown
        self.localvfsfiles = localvfsfiles

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
        def _dostore(store, data):
            """
            Util function which uploads data to the local blob storage.
            Returns oid and size of data.
            """
            # TODO(alexeyqu): do we care about metadata?
            oid = hashlib.sha256(data).hexdigest()
            store.write(oid, data)
            return oid, str(len(data))

        wctx = repo[None]
        store = repo.svfs.snapshotstore
        for f in self.unknown:
            f.oid, f.size = _dostore(store, wctx[f.path].data())
        for f in self.localvfsfiles:
            f.oid, f.size = _dostore(store, repo.localvfs.open(path=f.path).read())
        oid, size = _dostore(store, self.serialize())
        return oid, size

    def getauxfilesinfo(self):
        auxfilesinfo = set()
        auxfilesinfo.update(f.oid for f in self.unknown)
        auxfilesinfo.update(f.oid for f in self.localvfsfiles)
        return auxfilesinfo

    def files(self, showlocalvfs=False):
        filelist = []
        filelist += [("?", f.path) for f in self.unknown]
        filelist += [("!", f.path) for f in self.deleted]
        if showlocalvfs:
            filelist += [("?", f.path) for f in self.localvfsfiles]
        return filelist
